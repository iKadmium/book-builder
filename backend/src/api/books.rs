use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Serialize;

use crate::{AppState, books, build, deploy, git};

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

        // Snapshot per-book state that survives a rescan.
        struct Prev {
            last_built: Option<chrono::DateTime<chrono::Utc>>,
            last_deployed: Option<chrono::DateTime<chrono::Utc>>,
            epub_path: Option<std::path::PathBuf>,
        }
        let prev: HashMap<String, Prev> = catalogue
            .read()
            .map(|g| {
                g.books
                    .iter()
                    .map(|b| {
                        (
                            b.folder_name.clone(),
                            Prev {
                                last_built: b.last_built,
                                last_deployed: b.last_deployed,
                                epub_path: b.epub_path.clone(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut updated = books::scan(&data_dir);
        for book in &mut updated {
            if let Some(p) = prev.get(&book.folder_name) {
                book.last_built = p.last_built;
                book.last_deployed = p.last_deployed;
                book.epub_path = p.epub_path.clone();
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
) -> Result<StatusCode, (StatusCode, String)> {
    let (book_root, folder_name) = state
        .catalogue
        .read()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?
        .books
        .iter()
        .find(|b| b.folder_name == title)
        .map(|b| (b.root.clone(), b.folder_name.clone()))
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?;

    let (epub_path, md_path) = tokio::try_join!(
        build::build(&state.data_dir, &book_root, &folder_name),
        build::build_markdown(&state.data_dir, &book_root, &folder_name),
    )
    .map_err(|e| {
        tracing::error!("build failed for '{title}': {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    if let Ok(mut guard) = state.catalogue.write()
        && let Some(book) = guard.books.iter_mut().find(|b| b.folder_name == title)
    {
        book.last_built = Some(chrono::Utc::now());
        book.epub_path = Some(epub_path);
        book.md_path = Some(md_path);
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── deploy kindle ────────────────────────────────────────────────────────────

async fn deploy_kindle(
    State(state): State<AppState>,
    Path(title): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    let (epub_path, from, to, token_endpoint, google_creds) = {
        let catalogue = state
            .catalogue
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        let book = catalogue
            .books
            .iter()
            .find(|b| b.folder_name == title)
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?;
        let epub_path = book.epub_path.clone().ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                format!("'{title}' has not been built yet"),
            )
        })?;
        let cfg = state
            .config
            .read()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?;
        (
            epub_path,
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

    deploy::deploy_book(&from, &to, &token, &epub_path, &title)
        .await
        .map_err(|e| {
            tracing::error!("deploy failed for '{title}': {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, e)
        })?;

    if let Ok(mut guard) = state.catalogue.write()
        && let Some(book) = guard.books.iter_mut().find(|b| b.folder_name == title)
    {
        book.last_deployed = Some(chrono::Utc::now());
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
) -> Result<Json<OpenWebUIResponse>, (StatusCode, String)> {
    let md_path = state
        .catalogue
        .read()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?
        .books
        .iter()
        .find(|b| b.folder_name == title)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?
        .md_path
        .clone()
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                format!("'{title}' has not been built yet"),
            )
        })?;

    let url = deploy::deploy_to_openwebui(
        &state.open_webui_endpoint,
        &state.open_webui_api_key,
        &md_path,
        &title,
    )
    .await
    .map_err(|e| {
        tracing::error!("Open WebUI deploy failed for '{title}': {e}");
        (StatusCode::INTERNAL_SERVER_ERROR, e)
    })?;

    Ok(Json(OpenWebUIResponse { url }))
}

// ── downloads ────────────────────────────────────────────────────────────────

async fn download_epub(
    State(state): State<AppState>,
    Path(title): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let epub_path = state
        .catalogue
        .read()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?
        .books
        .iter()
        .find(|b| b.folder_name == title)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?
        .epub_path
        .clone()
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                format!("'{title}' has not been built yet"),
            )
        })?;

    serve_file(epub_path, "application/epub+zip").await
}

async fn download_md(
    State(state): State<AppState>,
    Path(title): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let md_path = state
        .catalogue
        .read()
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock poisoned".into()))?
        .books
        .iter()
        .find(|b| b.folder_name == title)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("book '{title}' not found")))?
        .md_path
        .clone()
        .ok_or_else(|| {
            (
                StatusCode::CONFLICT,
                format!("'{title}' has not been built yet"),
            )
        })?;

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
            let word_count = book.chapters.iter().map(|c| c.word_count).sum();
            let chapters = book
                .chapters
                .iter()
                .map(|c| ChapterStatus {
                    path: c
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string(),
                    word_count: c.word_count,
                })
                .collect();
            (
                book.folder_name.clone(),
                BookStatus {
                    title: book.title.clone(),
                    chapters,
                    word_count,
                    last_updated: book
                        .last_updated
                        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    last_built: book
                        .last_built
                        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                    last_deployed: book
                        .last_deployed
                        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                },
            )
        })
        .collect();

    Ok(Json(StatusResponse { last_pull, books }))
}
