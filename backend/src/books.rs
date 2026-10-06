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

#[derive(Debug, Clone, Serialize)]
pub struct Version {
    pub name: String,
    pub chapters: Vec<Chapter>,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_built: Option<DateTime<Utc>>,
    pub last_deployed: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub epub_path: Option<PathBuf>,
    #[serde(skip)]
    pub md_path: Option<PathBuf>,
}

pub fn version_title(title: &str, version: &str) -> String {
    format!("{title} ({version})")
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
    pub versions: Vec<Version>,
    /// Time of the most recent commit that touched any file in this book's folder.
    pub last_updated: Option<DateTime<Utc>>,
}

/// The full catalogue: the books plus when they were last refreshed from the repo.
#[derive(Debug, Clone)]
pub struct Catalogue {
    pub last_pull: Option<DateTime<Utc>>,
    pub books: Vec<Book>,
}

/// Shared, mutable catalogue threaded through the app.
pub type SharedCatalogue = Arc<RwLock<Catalogue>>;

/// Scan `data_dir/Books` for books. A book is any immediate subdirectory that
/// contains a `pandoc.yaml` file. Versions are immediate subdirectories
/// containing Markdown files.
pub fn scan(data_dir: &Path) -> Vec<Book> {
    let mut books = Vec::new();
    let repo = Repository::open(data_dir);
    let books_dir = data_dir.join("Books");
    let cover_last_updated = repo
        .as_ref()
        .ok()
        .and_then(|repo| last_updated_in_repo(repo, "Covers"));

    let entries = match fs::read_dir(&books_dir) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("Failed to read books dir {}: {e}", books_dir.display());
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
        let repo_path = format!("Books/{folder_name}");
        let last_updated = repo
            .as_ref()
            .ok()
            .and_then(|r| last_updated_in_repo(r, &repo_path));
        let shared_last_updated = repo
            .as_ref()
            .ok()
            .and_then(|repo| {
                [
                    "pandoc.yaml",
                    "Authors Note.md",
                    "object.svg",
                    "object-front.svg",
                ]
                .iter()
                .filter_map(|name| last_updated_in_repo(repo, &format!("{repo_path}/{name}")))
                .max()
            })
            .max(cover_last_updated);
        let mut versions = Vec::new();
        if let Ok(entries) = fs::read_dir(&path) {
            for entry in entries.flatten() {
                let version_path = entry.path();
                if !version_path.is_dir() {
                    continue;
                }
                let chapters = scan_chapters(&version_path);
                if chapters.is_empty() {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let output_title = version_title(&title, &name);
                let (epub_path, last_built) = latest_epub(data_dir, &output_title);
                let md_path = latest_md(data_dir, &output_title);
                let last_updated = repo
                    .as_ref()
                    .ok()
                    .and_then(|repo| last_updated_in_repo(repo, &format!("{repo_path}/{name}")))
                    .max(shared_last_updated);
                versions.push(Version {
                    name,
                    chapters,
                    last_updated,
                    last_built,
                    last_deployed: None,
                    epub_path,
                    md_path,
                });
            }
        }
        versions.sort_by(|left, right| left.name.cmp(&right.name));
        books.push(Book {
            folder_name,
            title,
            subtitle,
            blurb,
            root: path,
            versions,
            last_updated,
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
        if !matches_artifact(&name, &prefix, ".epub") {
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
        if !matches_artifact(&name, &prefix, ".md") {
            continue;
        }
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        if best.as_ref().is_none_or(|(_, prev)| modified > *prev) {
            best = Some((path, modified));
        }
    }
    best.map(|(p, _)| p)
}

fn matches_artifact(name: &str, title: &str, extension: &str) -> bool {
    name.strip_prefix(title)
        .and_then(|suffix| suffix.strip_prefix(' '))
        .and_then(|suffix| suffix.strip_suffix(extension))
        .is_some_and(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
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

    let entries = match fs::read_dir(book_dir) {
        Ok(e) => e,
        Err(_) => return chapters,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("md") {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_nested_versions_and_shared_cover_changes() {
        let data_dir = std::env::temp_dir().join(format!("book-history-{}", rand::random::<u64>()));
        let root = data_dir.join("Books/Test Book");
        fs::create_dir_all(root.join("Draft")).unwrap();
        fs::create_dir_all(root.join("Final")).unwrap();
        fs::create_dir_all(data_dir.join("Covers")).unwrap();
        fs::write(root.join("pandoc.yaml"), "metadata:\n  title: Test Book\n").unwrap();
        fs::write(root.join("Draft/01.md"), "draft").unwrap();
        fs::write(root.join("Final/01.md"), "final").unwrap();
        fs::write(data_dir.join("Covers/cover.svg.j2"), "initial template").unwrap();
        let repo = Repository::init(&data_dir).unwrap();
        let commit = |seconds| {
            let mut index = repo.index().unwrap();
            index
                .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
                .unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let signature =
                git2::Signature::new("Test", "test@example.com", &git2::Time::new(seconds, 0))
                    .unwrap();
            let parent = repo.head().ok().map(|head| head.peel_to_commit().unwrap());
            let parents: Vec<_> = parent.iter().collect();
            repo.commit(
                Some("HEAD"),
                &signature,
                &signature,
                "update",
                &tree,
                &parents,
            )
            .unwrap();
        };
        commit(1_700_000_000);
        fs::write(root.join("Draft/01.md"), "updated draft").unwrap();
        commit(1_700_000_100);
        let books = scan(&data_dir);
        assert_eq!(
            books[0].last_updated,
            DateTime::from_timestamp(1_700_000_100, 0)
        );
        assert_eq!(
            books[0].versions[0].last_updated,
            DateTime::from_timestamp(1_700_000_100, 0)
        );
        assert_eq!(
            books[0].versions[1].last_updated,
            DateTime::from_timestamp(1_700_000_000, 0)
        );
        fs::write(data_dir.join("Covers/cover.svg.j2"), "updated template").unwrap();
        commit(1_700_000_200);
        assert!(
            scan(&data_dir)[0]
                .versions
                .iter()
                .all(|version| version.last_updated == DateTime::from_timestamp(1_700_000_200, 0))
        );
        fs::write(root.join("object.svg"), "updated artwork").unwrap();
        commit(1_700_000_300);
        assert!(
            scan(&data_dir)[0]
                .versions
                .iter()
                .all(|version| version.last_updated == DateTime::from_timestamp(1_700_000_300, 0))
        );
        drop(repo);
        fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn discovers_versions_and_isolates_artifacts() {
        let data_dir =
            std::env::temp_dir().join(format!("book-versions-{}", rand::random::<u64>()));
        let root = data_dir.join("Books/Folder Name");
        fs::create_dir_all(data_dir.join("Archived/Old Book")).unwrap();
        fs::write(
            data_dir.join("Archived/Old Book/pandoc.yaml"),
            "metadata: {}",
        )
        .unwrap();
        fs::write(data_dir.join("pandoc.yaml"), "metadata: {}").unwrap();
        for name in ["Chapters", "Draft v1", "Draft v2", "Empty"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(
            root.join("pandoc.yaml"),
            "metadata:\n  title: Display Title\n",
        )
        .unwrap();
        fs::write(root.join("Chapters/01.md"), "current chapter").unwrap();
        fs::write(root.join("Draft v1/02.md"), "second").unwrap();
        fs::write(root.join("Draft v1/01.md"), "first draft chapter").unwrap();
        fs::write(root.join("Draft v2/01.md"), "another draft").unwrap();
        fs::create_dir_all(data_dir.join("dist")).unwrap();
        let epub = data_dir.join("dist/Display Title (Draft v1) 2026-10-03.epub");
        fs::write(&epub, "epub").unwrap();
        fs::write(
            data_dir.join("dist/Display Title (Draft v1) 2026-10-03.md"),
            "markdown",
        )
        .unwrap();

        let books = scan(&data_dir);
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].root, root);
        let versions = &books[0].versions;
        assert_eq!(
            versions
                .iter()
                .map(|version| version.name.as_str())
                .collect::<Vec<_>>(),
            ["Chapters", "Draft v1", "Draft v2"]
        );
        assert_eq!(versions[1].chapters.len(), 2);
        assert_eq!(versions[1].chapters[0].word_count, 3);
        assert_eq!(versions[1].epub_path.as_ref(), Some(&epub));
        assert!(versions[1].md_path.is_some());
        assert!(versions[0].epub_path.is_none());
        assert!(versions[2].epub_path.is_none());
        assert!(!matches_artifact(
            "Display Title (Draft v1 extra) 2026-10-03.epub",
            "Display Title (Draft v1)",
            ".epub"
        ));
        fs::remove_dir_all(data_dir).unwrap();
    }
}
