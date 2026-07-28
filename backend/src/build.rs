use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::{fs, process::Command};

/// Assembles the book markdown and runs pandoc to produce an EPUB.
/// Returns the path to the generated file.
pub async fn build(data_dir: &Path, book_root: &Path, title: &str) -> Result<PathBuf, String> {
    // Resolve to absolute paths up front — pandoc runs with a different cwd
    // (the book root), so any relative path we pass it would be misinterpreted.
    let data_dir = fs::canonicalize(data_dir)
        .await
        .map_err(|e| format!("failed to resolve data_dir: {e}"))?;
    let book_root = fs::canonicalize(book_root)
        .await
        .map_err(|e| format!("failed to resolve book_root: {e}"))?;

    let dist_dir = data_dir.join("dist");
    fs::create_dir_all(&dist_dir)
        .await
        .map_err(|e| format!("failed to create dist dir: {e}"))?;

    let date = Utc::now().format("%Y-%m-%d").to_string();

    // ── Assemble markdown ─────────────────────────────────────────────────

    let mut md = String::new();

    // Optional author's note before the chapters
    let note_path = book_root.join("Authors Note.md");
    if note_path.exists() {
        let content = fs::read_to_string(&note_path)
            .await
            .map_err(|e| format!("failed to read Authors Note.md: {e}"))?;
        md.push_str("# Author's Note\n\n");
        md.push_str(&content);
        md.push_str("\n\n");
    }

    // Collect and sort chapter files
    let chapters_dir = book_root.join("Chapters");
    let mut chapter_files: Vec<PathBuf> = Vec::new();
    let mut entries = fs::read_dir(&chapters_dir)
        .await
        .map_err(|e| format!("failed to read Chapters dir: {e}"))?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            chapter_files.push(path);
        }
    }
    chapter_files.sort();

    for (i, path) in chapter_files.iter().enumerate() {
        md.push_str(&format!("# Chapter {}\n\n", i + 1));
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        md.push_str(&content);
        md.push_str("\n\n");
    }

    // ── Write temp file ───────────────────────────────────────────────────

    // Use a per-book temp filename so concurrent builds don't collide.
    let tmp_path = data_dir.join(format!(".build_{title}.md"));
    fs::write(&tmp_path, &md)
        .await
        .map_err(|e| format!("failed to write temp file: {e}"))?;

    // ── Run pandoc ────────────────────────────────────────────────────────

    let output_path = dist_dir.join(format!("{title} {date}.epub"));
    let css_path = data_dir.join("pandoc.css");

    let result = Command::new("pandoc")
        .current_dir(book_root)
        .args(["-f", "markdown-yaml_metadata_block"])
        .arg(&tmp_path)
        .args(["-d", "pandoc.yaml"])
        .arg("-V")
        .arg(format!("date={date}"))
        .arg("-o")
        .arg(&output_path)
        .arg(format!("--css={}", css_path.display()))
        .arg("--top-level-division=chapter")
        .output()
        .await
        .map_err(|e| format!("failed to spawn pandoc: {e}"))?;

    fs::remove_file(&tmp_path).await.ok();

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("pandoc exited with {}: {stderr}", result.status));
    }

    tracing::info!("Built {output_path:?}");
    Ok(output_path)
}
