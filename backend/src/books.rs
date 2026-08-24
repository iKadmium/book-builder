use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use git2::Repository;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Chapter {
    pub path: PathBuf,
    pub word_count: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct PandocMetadata {
    title: Option<String>,
    subtitle: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PandocYaml {
    metadata: Option<PandocMetadata>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Book {
    /// Stable identifier: the directory name. Used in API routes and file naming.
    pub folder_name: String,
    /// Display title read from `pandoc.yaml`; falls back to `folder_name`.
    pub title: String,
    /// Optional subtitle read from `pandoc.yaml`.
    pub subtitle: Option<String>,
    /// Raw contents of `Blurb.md`, if present.
    pub blurb: Option<String>,
    pub root: PathBuf,
    pub chapters: Vec<Chapter>,
    /// Time of the most recent commit that touched any file in this book's folder.
    pub last_updated: Option<DateTime<Utc>>,
    /// Time of the last successful build (epub + markdown).
    pub last_built: Option<DateTime<Utc>>,
    /// Time the EPUB was last successfully deployed.
    pub last_deployed: Option<DateTime<Utc>>,
    /// Path to the most recently built EPUB (not serialised to JSON).
    #[serde(skip)]
    pub epub_path: Option<PathBuf>,
    /// Path to the most recently built markdown file (not serialised to JSON).
    #[serde(skip)]
    pub md_path: Option<PathBuf>,
}

/// The full catalogue: the books plus when they were last refreshed from the repo.
#[derive(Debug, Clone)]
pub struct Catalogue {
    pub last_pull: Option<DateTime<Utc>>,
    pub books: Vec<Book>,
}

/// Shared, mutable catalogue threaded through the app.
pub type SharedCatalogue = Arc<RwLock<Catalogue>>;

/// Scan `data_dir` for books.  A book is any immediate subdirectory that
/// contains a `pandoc.yaml` file.  Chapters are `*.md` files inside a
/// `Chapters/` subdirectory of the book root.
pub fn scan(data_dir: &Path) -> Vec<Book> {
    let mut books = Vec::new();
    let repo = Repository::open(data_dir);

    let entries = match fs::read_dir(data_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read data dir {}: {e}", data_dir.display());
            return books;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || !path.join("pandoc.yaml").exists() {
            continue;
        }
        let folder_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let pandoc = read_pandoc_yaml(&path);
        let title = pandoc
            .as_ref()
            .and_then(|p| p.title.clone())
            .unwrap_or_else(|| folder_name.clone());
        let subtitle = pandoc.and_then(|p| p.subtitle);
        let blurb = fs::read_to_string(path.join("Blurb.md")).ok();
        let last_updated = repo
            .as_ref()
            .ok()
            .and_then(|r| last_updated_in_repo(r, &folder_name));
        let chapters = scan_chapters(&path);
        let (epub_path, last_built) = latest_epub(data_dir, &folder_name);
        let md_path = latest_md(data_dir, &folder_name);
        books.push(Book {
            folder_name,
            title,
            subtitle,
            blurb,
            root: path,
            chapters,
            last_updated,
            last_built,
            last_deployed: None,
            epub_path,
            md_path,
        });
    }

    books.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));
    books
}

/// Parse `pandoc.yaml` and return the metadata fields.
fn read_pandoc_yaml(book_root: &Path) -> Option<PandocMetadata> {
    let content = fs::read_to_string(book_root.join("pandoc.yaml")).ok()?;
    let doc: PandocYaml = serde_yaml::from_str(&content).ok()?;
    doc.metadata
}

/// Find the most recently modified `{title}*.epub` in `data_dir/dist/`.
/// Returns `(epub_path, last_built)` where `last_built` is derived from the
/// file's modification time.
fn latest_epub(data_dir: &Path, title: &str) -> (Option<PathBuf>, Option<DateTime<Utc>>) {
    let dist = data_dir.join("dist");
    let entries = match fs::read_dir(&dist) {
        Ok(e) => e,
        Err(_) => return (None, None),
    };

    let prefix = title.to_string();
    let mut best: Option<(PathBuf, DateTime<Utc>)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.starts_with(&prefix) || !name.ends_with(".epub") {
            continue;
        }
        let modified = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| {
                DateTime::from_timestamp(
                    t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64,
                    0,
                )
            });
        if let Some(ts) = modified
            && best.as_ref().is_none_or(|(_, prev)| ts > *prev)
        {
            best = Some((path, ts));
        }
    }

    match best {
        Some((path, ts)) => (Some(path), Some(ts)),
        None => (None, None),
    }
}

fn latest_md(data_dir: &Path, title: &str) -> Option<PathBuf> {
    let dist = data_dir.join("dist");
    let entries = fs::read_dir(&dist).ok()?;
    let prefix = title.to_string();
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str())?.to_string();
        if !name.starts_with(&prefix) || !name.ends_with(".md") {
            continue;
        }
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        if best.as_ref().is_none_or(|(_, prev)| modified > *prev) {
            best = Some((path, modified));
        }
    }
    best.map(|(p, _)| p)
}

/// Returns the timestamp of the most recent commit that touched any file
/// under `subdir` (relative to the repo root).
fn last_updated_in_repo(repo: &Repository, subdir: &str) -> Option<DateTime<Utc>> {
    let mut revwalk = repo.revwalk().ok()?;
    revwalk.push_head().ok()?;
    revwalk.set_sorting(git2::Sort::TIME).ok()?;

    for oid in revwalk.flatten() {
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let tree = match commit.tree() {
            Ok(t) => t,
            Err(_) => continue,
        };

        let touched = if commit.parent_count() == 0 {
            tree.get_path(Path::new(subdir)).is_ok()
        } else {
            let Ok(parent) = commit.parent(0) else {
                continue;
            };
            let Ok(parent_tree) = parent.tree() else {
                continue;
            };
            let Ok(diff) = repo.diff_tree_to_tree(Some(&parent_tree), Some(&tree), None) else {
                continue;
            };
            diff.deltas().any(|delta| {
                delta
                    .new_file()
                    .path()
                    .or_else(|| delta.old_file().path())
                    .map(|p| p.starts_with(subdir))
                    .unwrap_or(false)
            })
        };

        if touched {
            return DateTime::from_timestamp(commit.time().seconds(), 0);
        }
    }

    None
}

fn scan_chapters(book_dir: &Path) -> Vec<Chapter> {
    let mut chapters = Vec::new();

    let entries = match fs::read_dir(book_dir.join("Chapters")) {
        Ok(e) => e,
        Err(_) => return chapters,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let word_count = fs::read_to_string(&path)
            .map(|content| content.split_whitespace().count())
            .unwrap_or(0);
        chapters.push(Chapter { path, word_count });
    }

    chapters.sort_by(|a, b| a.path.cmp(&b.path));
    chapters
}
