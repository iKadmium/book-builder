use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

use crate::{AppState, books, build, deploy, git};

#[derive(Default, Deserialize)]
struct VersionQuery {
    version: Option<String>,
}

fn selected_version<'book>(
    book: &'book books::Book,
    query: &VersionQuery,
) -> Result<&'book books::Version, (StatusCode, String)> {
    let version = match query.version.as_deref() {
        Some(name) => book.versions.iter().find(|version| version.name == name),
        None => book
            .versions
            .iter()
            .find(|version| version.name == "Chapters")
            .or_else(|| book.versions.first()),
    };
    version.ok_or_else(|| (StatusCode::NOT_FOUND, "version not found".into()))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/pull", post(pull))
        .route("/build/{title}", post(build_book))
        .route("/deploy/kindle/{title}", post(deploy_kindle))
        .route("/deploy/openwebui/{title}", post(deploy_openwebui))
        .route("/download/{title}/epub", get(download_epub))
        .route("/download/{title}/md", get(download_md))
        .route("/status", get(status))
}

// ── pull ────────────────────────────────────────────────────────────────────

async fn pull(State(state): State<AppState>) -> Result<StatusCode, StatusCode> {
    let data_dir = state.data_dir.clone();

    let (repo_url, token_endpoint, creds) = {
        let cfg = state
            .config
            .read()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let repo_url = format!(
            "{}/{}",
            cfg.forgejo.url.trim_end_matches('/'),
            cfg.forgejo.repo
        );
        let token_endpoint = format!(
            "{}/login/oauth/access_token",
            cfg.forgejo.url.trim_end_matches('/')
        );
        let creds = state.forgejo_creds.clone();
        (repo_url, token_endpoint, creds)
        // cfg (RwLockReadGuard) is dropped here
    };

    let token = state
        .oauth
        .token(crate::oauth::Provider::Forgejo, &creds, &token_endpoint)
        .await
        .ok_or_else(|| {
            tracing::error!(
                "No valid Forgejo token — authorize first at /api/oauth/forgejo/authorize"
            );
            StatusCode::UNAUTHORIZED
        })?;

    let catalogue = Arc::clone(&state.catalogue);

    tokio::task::spawn_blocking(move || -> Result<(), git2::Error> {
        // sync_repo clones on first run, pulls on subsequent runs.
        git::sync_repo(&repo_url, &token, &data_dir)?;

        let prev: HashMap<String, Vec<books::Version>> = catalogue
            .read()
            .map(|g| {
                g.books
                    .iter()
                    .map(|b| (b.folder_name.clone(), b.versions.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let mut updated = books::scan(&data_dir);
        for book in &mut updated {
            if let Some(p) = prev.get(&book.folder_name) {
                for version in &mut book.versions {
                    if let Some(previous) = p.iter().find(|previous| previous.name == version.name)
                    {
                        version.last_deployed = previous.last_deployed;
                    }
                }
            }
        }

        tracing::info!("Refreshed {} book(s) after pull", updated.len());
        if let Ok(mut guard) = catalogue.write() {
            guard.books = updated;
            guard.last_pull = Some(chrono::Utc::now());
        }
        Ok(())
    })
    .await
    .map_err(|e| {
        tracing::error!("pull task panicked: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .map_err(|e| {
        tracing::error!("git pull failed: {e}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::NO_CONTENT)
}

// ── build ────────────────────────────────────────────────────────────────────

async fn build_book(
    State(state): State<AppState>,
    Path(title): Path<String>,
    Query(query): Query<VersionQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    let (book_root, book_title, version_name) = {
        let catalogue = state
            .catalogue
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        let book = catalogue
            .books
            .iter()
            .find(|book| book.folder_name == title)
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?;
        let version = selected_version(book, &query)?;
        (book.root.clone(), book.title.clone(), version.name.clone())
    };

    let (epub_path, md_path) = tokio::try_join!(
        build::build(&state.data_dir, &book_root, &book_title, &version_name),
        build::build_markdown(&state.data_dir, &book_root, &book_title, &version_name),
    )
    .map_err(|e| {
        tracing::error!("build failed for '{title}': {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    if let Ok(mut guard) = state.catalogue.write()
        && let Some(book) = guard.books.iter_mut().find(|b| b.folder_name == title)
        && let Some(version) = book
            .versions
            .iter_mut()
            .find(|version| version.name == version_name)
    {
        version.last_built = Some(chrono::Utc::now());
        version.epub_path = Some(epub_path);
        version.md_path = Some(md_path);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── deploy kindle ────────────────────────────────────────────────────────────

async fn deploy_kindle(
    State(state): State<AppState>,
    Path(title): Path<String>,
    Query(query): Query<VersionQuery>,
) -> Result<StatusCode, (StatusCode, String)> {
    let (epub_path, version_name, output_title, from, to, token_endpoint, google_creds) = {
        let catalogue = state
            .catalogue
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        let book = catalogue
            .books
            .iter()
            .find(|b| b.folder_name == title)
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?;
        let version = selected_version(book, &query)?;
        let epub_path = version.epub_path.clone().ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                format!(
                    "'{title}' version '{}' has not been built yet",
                    version.name
                ),
            )
        })?;
        let cfg = state
            .config
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        (
            epub_path,
            version.name.clone(),
            books::version_title(&book.title, &version.name),
            cfg.email.from.clone(),
            cfg.email.to.clone(),
            "https://oauth2.googleapis.com/token".to_string(),
            state.google_creds.clone(),
        )
    };

    let token = state
        .oauth
        .token(
            crate::oauth::Provider::Google,
            &google_creds,
            &token_endpoint,
        )
        .await
        .ok_or_else(|| {
            tracing::error!("No Google token — authorize at /api/oauth/google/authorize");
            (
                StatusCode::UNAUTHORIZED,
                "Google not connected — visit /api/oauth/google/authorize".into(),
            )
        })?;

    deploy::deploy_book(&from, &to, &token, &epub_path, &output_title)
        .await
        .map_err(|e| {
            tracing::error!("deploy failed for '{title}': {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e)
        })?;

    if let Ok(mut guard) = state.catalogue.write()
        && let Some(book) = guard.books.iter_mut().find(|b| b.folder_name == title)
        && let Some(version) = book
            .versions
            .iter_mut()
            .find(|version| version.name == version_name)
    {
        version.last_deployed = Some(chrono::Utc::now());
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── deploy open webui ────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OpenWebUIResponse {
    url: String,
}

async fn deploy_openwebui(
    State(state): State<AppState>,
    Path(title): Path<String>,
    Query(query): Query<VersionQuery>,
) -> Result<Json<OpenWebUIResponse>, (StatusCode, String)> {
    let (md_path, output_title) = selected_artifact(&state, &title, &query, false)?;

    let url = deploy::deploy_to_openwebui(
        &state.open_webui_endpoint,
        &state.open_webui_api_key,
        &md_path,
        &output_title,
    )
    .await
    .map_err(|e| {
        tracing::error!("Open WebUI deploy failed for '{title}': {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    Ok(Json(OpenWebUIResponse { url }))
}

fn selected_artifact(
    state: &AppState,
    title: &str,
    query: &VersionQuery,
    epub: bool,
) -> Result<(std::path::PathBuf, String), (StatusCode, String)> {
    let catalogue = state
        .catalogue
        .read()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
    let book = catalogue
        .books
        .iter()
        .find(|b| b.folder_name == title)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?;
    let version = selected_version(book, query)?;
    let path = if epub {
        &version.epub_path
    } else {
        &version.md_path
    };
    let path = path.clone().ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            format!(
                "'{title}' version '{}' has not been built yet",
                version.name
            ),
        )
    })?;
    Ok((path, books::version_title(&book.title, &version.name)))
}

// ── downloads ────────────────────────────────────────────────────────────────

async fn download_epub(
    State(state): State<AppState>,
    Path(title): Path<String>,
    Query(query): Query<VersionQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (epub_path, _) = selected_artifact(&state, &title, &query, true)?;

    serve_file(epub_path, "application/epub+zip").await
}

async fn download_md(
    State(state): State<AppState>,
    Path(title): Path<String>,
    Query(query): Query<VersionQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let (md_path, _) = selected_artifact(&state, &title, &query, false)?;

    serve_file(md_path, "text/markdown").await
}

async fn serve_file(
    path: std::path::PathBuf,
    content_type: &'static str,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download")
        .to_string();
    let bytes = tokio::fs::read(&path).await.map_err(|e| {
        tracing::error!("Failed to read {}: {e}", path.display());
        (StatusCode::NOT_FOUND, format!("File not found: {e}"))
    })?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        Body::from(bytes),
    ))
}

// ── status ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    #[serde(rename = "lastPull")]
    last_pull: Option<String>,
    #[serde(flatten)]
    books: HashMap<String, BookStatus>,
}

#[derive(Serialize)]
struct BookStatus {
    title: String,
    subtitle: Option<String>,
    blurb: Option<String>,
    versions: HashMap<String, VersionStatus>,
    #[serde(flatten)]
    default_version: VersionStatus,
}

#[derive(Serialize)]
struct VersionStatus {
    chapters: Vec<ChapterStatus>,
    #[serde(rename = "wordCount")]
    word_count: usize,
    #[serde(rename = "lastUpdated")]
    last_updated: Option<String>,
    #[serde(rename = "lastBuilt")]
    last_built: Option<String>,
    #[serde(rename = "lastDeployed")]
    last_deployed: Option<String>,
}

#[derive(Serialize)]
struct ChapterStatus {
    path: String,
    #[serde(rename = "wordCount")]
    word_count: usize,
}

fn version_status(version: Option<&books::Version>) -> VersionStatus {
    VersionStatus {
        chapters: version
            .map(|version| {
                version
                    .chapters
                    .iter()
                    .map(|chapter| ChapterStatus {
                        path: chapter
                            .path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("")
                            .to_string(),
                        word_count: chapter.word_count,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        word_count: version
            .map(|version| {
                version
                    .chapters
                    .iter()
                    .map(|chapter| chapter.word_count)
                    .sum()
            })
            .unwrap_or(0),
        last_updated: version
            .and_then(|version| version.last_updated)
            .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        last_built: version
            .and_then(|version| version.last_built)
            .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        last_deployed: version
            .and_then(|version| version.last_deployed)
            .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    }
}

async fn status(State(state): State<AppState>) -> Result<Json<StatusResponse>, StatusCode> {
    let catalogue = state.catalogue.read().map_err(|_| {
        tracing::error!("catalogue lock poisoned");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let last_pull = catalogue
        .last_pull
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));

    let books = catalogue
        .books
        .iter()
        .map(|book| {
            let mut default_version =
                version_status(selected_version(book, &VersionQuery::default()).ok());
            default_version.last_updated = book
                .last_updated
                .map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            (
                book.folder_name.clone(),
                BookStatus {
                    title: book.title.clone(),
                    subtitle: book.subtitle.clone(),
                    blurb: book.blurb.clone(),
                    versions: book
                        .versions
                        .iter()
                        .map(|version| (version.name.clone(), version_status(Some(version))))
                        .collect(),
                    default_version,
                },
            )
        })
        .collect();

    Ok(Json(StatusResponse { last_pull, books }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_defaults_to_chapters_and_never_falls_back_for_unknown_versions() {
        let version = |name: &str, epub_path| books::Version {
            name: name.into(),
            chapters: Vec::new(),
            last_updated: None,
            last_built: None,
            last_deployed: None,
            epub_path,
            md_path: None,
        };
        let mut book = books::Book {
            folder_name: "Folder Name".into(),
            title: "Display Title".into(),
            subtitle: None,
            blurb: None,
            root: "book".into(),
            versions: vec![
                version("Draft v1", Some("draft.epub".into())),
                version("Chapters", None),
            ],
            last_updated: None,
        };
        assert_eq!(
            selected_version(&book, &VersionQuery::default())
                .unwrap()
                .name,
            "Chapters"
        );
        let query = VersionQuery {
            version: Some("Draft v1".into()),
        };
        assert_eq!(
            selected_version(&book, &query)
                .unwrap()
                .epub_path
                .as_deref(),
            Some(std::path::Path::new("draft.epub"))
        );
        let status = serde_json::to_value(version_status(Some(&book.versions[0]))).unwrap();
        assert!(status.get("lastUpdated").is_some());
        assert!(status.get("lastBuilt").is_some());
        assert!(status.get("lastDeployed").is_some());
        let query = VersionQuery {
            version: Some("Chapters".into()),
        };
        assert!(selected_version(&book, &query).unwrap().epub_path.is_none());
        let query = VersionQuery {
            version: Some("../Draft v1".into()),
        };
        assert_eq!(
            selected_version(&book, &query).unwrap_err().0,
            StatusCode::NOT_FOUND
        );
        book.versions.pop();
        assert_eq!(
            selected_version(&book, &VersionQuery::default())
                .unwrap()
                .name,
            "Draft v1"
        );
        book.versions.clear();
        assert_eq!(
            selected_version(&book, &VersionQuery::default())
                .unwrap_err()
                .0,
            StatusCode::NOT_FOUND
        );
    }
}
