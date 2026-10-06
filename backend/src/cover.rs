//! Covers are compiled from `data/Covers/cover.typ` using the Typst CLI.
//! The template reads the book's metadata via `yaml("book.yaml")`, which is
//! redirected to the book folder's `pandoc.yaml`.

use std::{fs, path::Path, process::Command};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub fn generate(data_dir: &Path, book_root: &Path, out: &Path) -> Result<()> {
    let png_path = out.with_extension("png");
    generate_png(data_dir, book_root, &png_path)?;

    let image = image::load_from_memory(&fs::read(&png_path)?)?.to_rgb8();
    let (width, height) = image.dimensions();
    image::codecs::jpeg::JpegEncoder::new_with_quality(fs::File::create(out)?, 92).encode(
        image.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(())
}

pub fn generate_svg(data_dir: &Path, book_root: &Path, out: &Path) -> Result<()> {
    compile_typst(data_dir, book_root, out, None)
}

pub fn generate_png(data_dir: &Path, book_root: &Path, out: &Path) -> Result<()> {
    compile_typst(data_dir, book_root, out, Some("72"))
}

/// Removes the staged template copy once compilation finishes.
struct StagedTemplate(std::path::PathBuf);

impl Drop for StagedTemplate {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn compile_typst(data_dir: &Path, book_root: &Path, out: &Path, ppi: Option<&str>) -> Result<()> {
    let data_dir = fs::canonicalize(data_dir)?;
    let book_root = fs::canonicalize(book_root)?;
    let relative_book_root = book_root.strip_prefix(&data_dir)?;
    let cover_typ = data_dir.join("Covers/cover.typ");
    let covers_dir = data_dir.join("Covers");
    let metadata_path = Path::new("/").join(relative_book_root).join("pandoc.yaml");
    let metadata = relative_path(&metadata_path);

    // Typst resolves relative paths against the template's own directory, so
    // `book.yaml` would be looked up in Covers. Stage a copy alongside the
    // template that points at the book's pandoc.yaml instead, keeping other
    // Covers-relative assets resolvable.
    let source = fs::read_to_string(&cover_typ)?;
    let rewritten = source.replace("\"book.yaml\"", &format!("{metadata:?}"));
    let staged = (rewritten != source)
        .then(|| -> Result<StagedTemplate> {
            let path = covers_dir.join(format!(".cover-{}.typ", rand::random::<u64>()));
            fs::write(&path, &rewritten)?;
            Ok(StagedTemplate(path))
        })
        .transpose()?;
    let template = staged
        .as_ref()
        .map_or(cover_typ.as_path(), |staged| &staged.0);

    let mut command = Command::new("typst");
    command
        .arg("compile")
        .arg("--root")
        .arg(&data_dir)
        .arg("--font-path")
        .arg(&covers_dir)
        .arg("--ignore-system-fonts");
    if let Some(ppi) = ppi {
        command.args(["--ppi", ppi]);
    }
    command.arg("--input").arg(format!("metadata={metadata}"));

    for name in ["object", "object-front"] {
        let path = book_root.join(format!("{name}.svg"));
        if path.is_file() {
            let relative = Path::new("/")
                .join(relative_book_root)
                .join(format!("{name}.svg"));
            command
                .arg("--input")
                .arg(format!("{name}={}", relative_path(&relative)));
        }
    }

    let output = command
        .arg(template)
        .arg(out)
        .output()
        .map_err(|error| format!("failed to spawn Typst: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Typst exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    #[ignore = "requires the source data repository with Covers template and fonts"]
    fn renders_source_repository_template_with_pinned_fonts() {
        let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let books = crate::books::scan(&data_dir);
        let book = books.first().expect("source repository has no books");
        let output_dir =
            std::env::temp_dir().join(format!("source-cover-{}", rand::random::<u64>()));
        fs::create_dir_all(&output_dir).unwrap();
        let out = output_dir.join("cover.jpg");
        generate(&data_dir, &book.root, &out).unwrap();
        let image = image::load_from_memory(&fs::read(&out).unwrap())
            .unwrap()
            .to_rgb8();
        assert_eq!(image.dimensions(), (1600, 2560));
        assert!(image.pixels().any(|pixel| pixel != image.get_pixel(0, 0)));
        fs::remove_dir_all(output_dir).unwrap();
    }

    #[test]
    fn reads_book_yaml_from_book_folder_pandoc_yaml() {
        let data_dir = std::env::temp_dir().join(format!("book-yaml-{}", rand::random::<u64>()));
        let root = data_dir.join("Books/Test Book");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(data_dir.join("Covers")).unwrap();
        fs::write(root.join("pandoc.yaml"), "metadata:\n  title: Test\n").unwrap();
        fs::write(
            data_dir.join("Covers/cover.typ"),
            "#let meta = yaml(\"book.yaml\").metadata\n#set page(width: 32pt, height: 48pt)\n#text(size: 8pt, meta.title)\n",
        )
        .unwrap();

        let svg = data_dir.join("cover.svg");
        generate_svg(&data_dir, &root, &svg).unwrap();
        assert!(fs::read_to_string(&svg).unwrap().contains("<svg"));
        let leftovers: Vec<_> = fs::read_dir(data_dir.join("Covers"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers, ["cover.typ"]);
        fs::remove_dir_all(data_dir).unwrap();
    }

    #[test]
    fn compiles_typst_to_svg_png_and_jpeg() {
        let data_dir = std::env::temp_dir().join(format!("book-cover-{}", rand::random::<u64>()));
        let root = data_dir.join("Books/Test Book");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(data_dir.join("Covers")).unwrap();
        fs::write(root.join("pandoc.yaml"), "metadata:\n  title: Test\n").unwrap();
        fs::write(
            data_dir.join("Covers/cover.typ"),
            "#let meta = yaml(sys.inputs.at(\"metadata\")).metadata\n#set page(width: 32pt, height: 48pt, fill: red)\n#text(size: 8pt, meta.title)\n",
        )
        .unwrap();

        let svg = data_dir.join("cover.svg");
        generate_svg(&data_dir, &root, &svg).unwrap();
        assert!(fs::read_to_string(&svg).unwrap().contains("<svg"));

        let png = data_dir.join("cover.png");
        generate_png(&data_dir, &root, &png).unwrap();
        assert_eq!(
            image::load_from_memory(&fs::read(&png).unwrap())
                .unwrap()
                .dimensions(),
            (32, 48)
        );

        let jpeg = data_dir.join("cover.jpg");
        generate(&data_dir, &root, &jpeg).unwrap();
        assert_eq!(
            image::load_from_memory(&fs::read(&jpeg).unwrap())
                .unwrap()
                .dimensions(),
            (32, 48)
        );
        fs::remove_dir_all(data_dir).unwrap();
    }
}
