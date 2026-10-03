use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::{fs, process::Command};

/// Assembles the book markdown and runs pandoc to produce an EPUB.
/// Returns the path to the generated file.
pub async fn build(
    data_dir: &Path,
    book_root: &Path,
    title: &str,
    version: &str,
) -> Result<PathBuf, String> {
    let title = crate::books::version_title(title, version);
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

    let md = assemble_markdown(&book_root, version).await?;

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
        .current_dir(&book_root)
        .args(["-f", "markdown-yaml_metadata_block"])
        .arg(&tmp_path)
        .args(["-d", "pandoc.yaml"])
        .arg("--metadata")
        .arg(format!("title={title}"))
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

/// Assembles the book as a single markdown file in `dist/`.
/// Returns the path to the generated file.
pub async fn build_markdown(
    data_dir: &Path,
    book_root: &Path,
    title: &str,
    version: &str,
) -> Result<PathBuf, String> {
    let title = crate::books::version_title(title, version);
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
    let md = assemble_markdown(&book_root, version).await?;

    let output_path = dist_dir.join(format!("{title} {date}.md"));
    fs::write(&output_path, &md)
        .await
        .map_err(|e| format!("failed to write markdown file: {e}"))?;

    tracing::info!("Built {output_path:?}");
    Ok(output_path)
}

async fn assemble_markdown(book_root: &Path, version: &str) -> Result<String, String> {
    let mut md = String::new();

    let note_path = book_root.join("Authors Note.md");
    if note_path.exists() {
        let content = fs::read_to_string(&note_path)
            .await
            .map_err(|e| format!("failed to read Authors Note.md: {e}"))?;
        md.push_str("# Author's Note\n\n");
        md.push_str(&content);
        md.push_str("\n\n");
    }

    let chapters_dir = book_root.join(version);
    let mut chapter_files: Vec<PathBuf> = Vec::new();
    let mut entries = fs::read_dir(&chapters_dir)
        .await
        .map_err(|e| format!("failed to read version '{version}': {e}"))?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            chapter_files.push(path);
        }
    }
    chapter_files.sort();

    let mut chapter_num = 0;
    for path in &chapter_files {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let heading = if stem == "Epilogue" {
            "# Epilogue".to_string()
        } else {
            chapter_num += 1;
            format!("# Chapter {chapter_num}")
        };
        md.push_str(&heading);
        md.push_str("\n\n");
        let content = fs::read_to_string(path)
            .await
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        md.push_str(&content);
        md.push_str("\n\n");
    }

    Ok(md)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires pandoc and unzip"]
    async fn epub_title_and_filename_include_selected_version() {
        let data_dir = std::env::temp_dir().join(format!("book-epub-{}", rand::random::<u64>()));
        let root = data_dir.join("Folder Name");
        fs::create_dir_all(root.join("Draft v1")).await.unwrap();
        fs::create_dir_all(root.join("Chapters")).await.unwrap();
        fs::write(
            root.join("pandoc.yaml"),
            "metadata:\n  title: Display Title\n  author: Test Author\n",
        )
        .await
        .unwrap();
        fs::write(data_dir.join("pandoc.css"), "body { color: black; }")
            .await
            .unwrap();
        fs::write(root.join("Draft v1/01.md"), "selected manuscript")
            .await
            .unwrap();
        fs::write(root.join("Chapters/01.md"), "unselected manuscript")
            .await
            .unwrap();
        let path = build(&data_dir, &root, "Display Title", "Draft v1")
            .await
            .unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!(
                "Display Title (Draft v1) {}.epub",
                Utc::now().format("%Y-%m-%d")
            )
        );
        let metadata = Command::new("unzip")
            .arg("-p")
            .arg(&path)
            .arg("*.opf")
            .output()
            .await
            .unwrap();
        assert!(metadata.status.success());
        assert!(
            String::from_utf8(metadata.stdout)
                .unwrap()
                .contains(">Display Title (Draft v1)</dc:title>")
        );
        let content = Command::new("unzip")
            .arg("-p")
            .arg(&path)
            .arg("*.xhtml")
            .output()
            .await
            .unwrap();
        assert!(content.status.success());
        let content = String::from_utf8(content.stdout).unwrap();
        assert!(content.contains("selected manuscript"));
        assert!(!content.contains("unselected manuscript"));
        fs::remove_dir_all(data_dir).await.unwrap();
    }

    #[tokio::test]
    async fn builds_only_selected_version_with_shared_note() {
        let data_dir = std::env::temp_dir().join(format!("book-build-{}", rand::random::<u64>()));
        let root = data_dir.join("Folder Name");
        fs::create_dir_all(root.join("Chapters")).await.unwrap();
        fs::create_dir_all(root.join("Draft v1")).await.unwrap();
        fs::write(root.join("Authors Note.md"), "shared note")
            .await
            .unwrap();
        fs::write(root.join("Chapters/01.md"), "current manuscript")
            .await
            .unwrap();
        fs::write(root.join("Draft v1/02.md"), "second draft chapter")
            .await
            .unwrap();
        fs::write(root.join("Draft v1/01.md"), "first draft chapter")
            .await
            .unwrap();
        fs::write(root.join("Draft v1/Epilogue.md"), "draft ending")
            .await
            .unwrap();

        let path = build_markdown(&data_dir, &root, "Display Title", "Draft v1")
            .await
            .unwrap();
        let date = Utc::now().format("%Y-%m-%d");
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            format!("Display Title (Draft v1) {date}.md")
        );
        let markdown = fs::read_to_string(path).await.unwrap();
        assert!(markdown.starts_with("# Author's Note\n\nshared note"));
        assert!(markdown.contains("# Chapter 1\n\nfirst draft chapter"));
        assert!(markdown.contains("# Chapter 2\n\nsecond draft chapter"));
        assert!(markdown.contains("# Epilogue\n\ndraft ending"));
        assert!(!markdown.contains("current manuscript"));
        fs::remove_dir_all(data_dir).await.unwrap();
    }
}
