//! Covers use `data/Covers/cover.svg.j2` and the TTF/OTF fonts alongside it.
//! Metadata and optional object.svg/object-front.svg come from the book directory.

use std::{collections::HashMap, fs, path::Path, sync::Arc};

use minijinja::{AutoEscape, Environment, Error, ErrorKind, Value, context, value::Kwargs};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct FontBook {
    faces: HashMap<String, Vec<u8>>,
}

impl FontBook {
    fn load(dir: &Path) -> Result<Self> {
        let mut faces = HashMap::new();
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if !matches!(extension.to_ascii_lowercase().as_str(), "ttf" | "otf") {
                continue;
            }
            let data = fs::read(&path)?;
            let face = rustybuzz::ttf_parser::Face::parse(&data, 0)?;
            if let Some(family) = family_name(&face) {
                faces.entry(family).or_insert(data);
            }
        }
        Ok(Self { faces })
    }

    fn face(&self, family: &str) -> std::result::Result<rustybuzz::Face<'_>, Error> {
        let data = self.faces.get(family).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidOperation,
                format!("font family {family:?} not found in Covers"),
            )
        })?;
        rustybuzz::Face::from_slice(data, 0)
            .ok_or_else(|| Error::new(ErrorKind::InvalidOperation, "unparseable font"))
    }

    fn unit_width(&self, text: &str, family: &str) -> std::result::Result<f64, Error> {
        let face = self.face(family)?;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let advance: i64 = shaped
            .glyph_positions()
            .iter()
            .map(|position| i64::from(position.x_advance))
            .sum();
        Ok(advance as f64 / f64::from(face.units_per_em()))
    }

    fn unit_cap_height(&self, family: &str) -> std::result::Result<f64, Error> {
        let face = self.face(family)?;
        let cap = face
            .capital_height()
            .map(f64::from)
            .unwrap_or(f64::from(face.ascender()) * 0.8);
        Ok(cap / f64::from(face.units_per_em()))
    }
}

fn family_name(face: &rustybuzz::ttf_parser::Face<'_>) -> Option<String> {
    use rustybuzz::ttf_parser::name_id;
    [name_id::TYPOGRAPHIC_FAMILY, name_id::FAMILY]
        .iter()
        .find_map(|&id| {
            face.names()
                .into_iter()
                .filter(|name| name.name_id == id && name.is_unicode())
                .find_map(|name| name.to_string())
        })
}

fn environment(fonts: Arc<FontBook>) -> Environment<'static> {
    let mut environment = Environment::new();
    environment.set_auto_escape_callback(|_| AutoEscape::Html);
    let fit_fonts = fonts.clone();
    environment.add_function(
        "fit_size",
        move |text: String, family: String, width: f64, kwargs: Kwargs| {
            let max: Option<f64> = kwargs.get("max")?;
            let spacing: f64 = kwargs.get::<Option<f64>>("spacing")?.unwrap_or(0.0);
            kwargs.assert_all_used()?;
            let unit = fit_fonts.unit_width(&text, &family)?;
            if unit <= 0.0 {
                return Ok::<f64, Error>(max.unwrap_or(0.0));
            }
            let size = (width - spacing * text.chars().count() as f64) / unit;
            let size = max.map_or(size, |max| size.min(max)).max(0.0);
            Ok((size * 10.0).floor() / 10.0)
        },
    );
    let width_fonts = fonts.clone();
    environment.add_function(
        "text_width",
        move |text: String, family: String, size: f64, kwargs: Kwargs| {
            let spacing: f64 = kwargs.get::<Option<f64>>("spacing")?.unwrap_or(0.0);
            kwargs.assert_all_used()?;
            Ok::<f64, Error>(
                width_fonts.unit_width(&text, &family)? * size
                    + spacing * text.chars().count() as f64,
            )
        },
    );
    environment.add_function("cap_height", move |family: String, size: f64| {
        Ok::<f64, Error>(fonts.unit_cap_height(&family)? * size)
    });
    environment
}

fn read_inline_svg(path: &Path) -> Result<Option<String>> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let document = roxmltree::Document::parse_with_options(
        &source,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err(format!("{} has no SVG root", path.display()).into());
    }
    Ok(Some(source[root.range()].to_owned()))
}

pub fn generate(data_dir: &Path, book_root: &Path, out: &Path) -> Result<()> {
    let svg = generate_svg(data_dir, book_root, &out.with_extension("svg"))?;
    render_jpeg(&svg, &data_dir.join("Covers"), book_root, out)
}

pub fn generate_svg(data_dir: &Path, book_root: &Path, out: &Path) -> Result<String> {
    let covers_dir = data_dir.join("Covers");
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(book_root.join("pandoc.yaml"))?)?;
    let metadata = yaml.get("metadata").ok_or("no `metadata` block")?;
    let art = metadata.get("cover-art").filter(|value| !value.is_null());
    let mut environment = environment(Arc::new(FontBook::load(&covers_dir)?));
    environment.add_template_owned(
        "cover.svg.j2".to_owned(),
        fs::read_to_string(covers_dir.join("cover.svg.j2"))?,
    )?;
    let svg = environment.get_template("cover.svg.j2")?.render(context! {
        meta => Value::from_serialize(metadata),
        art => art.map(Value::from_serialize).unwrap_or_else(|| Value::from_serialize(HashMap::<String, String>::new())),
        object => read_inline_svg(&book_root.join("object.svg"))?,
        object_front => read_inline_svg(&book_root.join("object-front.svg"))?,
    })?;
    fs::write(out, &svg)?;
    Ok(svg)
}

fn render_jpeg(svg: &str, fonts_dir: &Path, book_root: &Path, out: &Path) -> Result<()> {
    let mut fonts = resvg::usvg::fontdb::Database::new();
    fonts.load_fonts_dir(fonts_dir);
    let options = resvg::usvg::Options {
        fontdb: Arc::new(fonts),
        resources_dir: Some(book_root.to_owned()),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options)?;
    let size = tree.size().to_int_size();
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("bad cover size")?;
    pixmap.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let rgb: Vec<u8> = pixmap
        .data()
        .chunks_exact(4)
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect();
    image::codecs::jpeg::JpegEncoder::new_with_quality(fs::File::create(out)?, 92).encode(
        &rgb,
        size.width(),
        size.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the source data repository with Covers template and fonts"]
    fn renders_source_repository_template_with_pinned_fonts() {
        let data_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let books = crate::books::scan(&data_dir);
        let book = books.first().expect("source repository has no books");
        let fonts = FontBook::load(&data_dir.join("Covers")).unwrap();
        assert!(fonts.unit_width("AVATAR", "Anton").unwrap() > 0.0);
        assert!(fonts.unit_cap_height("Yellowtail").unwrap() > 0.0);
        let environment = environment(Arc::new(fonts));
        let size = environment
            .render_str(
                "{{ fit_size('AVATAR', 'Anton', 100, max=200, spacing=2) }}",
                context! {},
            )
            .unwrap()
            .parse::<f64>()
            .unwrap();
        let width = environment
            .render_str(
                "{{ text_width('AVATAR', 'Anton', size, spacing=2) }}",
                context! { size => size },
            )
            .unwrap()
            .parse::<f64>()
            .unwrap();
        assert!(width <= 100.0 && width > 99.0);
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
    fn renders_metadata_and_optional_artwork_to_jpeg() {
        let data_dir = std::env::temp_dir().join(format!("book-cover-{}", rand::random::<u64>()));
        let root = data_dir.join("Books/Test Book");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(data_dir.join("Covers")).unwrap();
        fs::write(
            root.join("pandoc.yaml"),
            "metadata:\n  title: 'Title & <test>'\n  cover-art:\n    color: '#ff0000'\n",
        )
        .unwrap();
        fs::write(data_dir.join("Covers/cover.svg.j2"), r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="48"><title>{{ meta.title }}</title><rect width="32" height="48" fill="{{ art.color or '#ffffff' }}"/>{% if object %}{{ object|safe }}{% endif %}{% if object_front %}{{ object_front|safe }}{% endif %}</svg>"##).unwrap();
        fs::write(root.join("object.svg"), r#"<?xml version="1.0"?><!DOCTYPE svg [<!ELEMENT svg ANY>]><svg xmlns="http://www.w3.org/2000/svg"><rect width="8" height="8" fill="blue"/></svg>"#).unwrap();
        fs::write(root.join("object-front.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="16" cy="24" r="4" fill="green"/></svg>"#).unwrap();
        let out = data_dir.join("cover.jpg");
        generate_svg(&data_dir, &root, &out.with_extension("svg")).unwrap();
        assert!(out.with_extension("svg").exists());
        assert!(!out.exists());
        generate(&data_dir, &root, &out).unwrap();
        let svg = fs::read_to_string(out.with_extension("svg")).unwrap();
        assert!(svg.contains("Title &amp; &lt;test&gt;"));
        assert!(!svg.contains("<?xml"));
        assert!(!svg.contains("<!DOCTYPE"));
        let image = image::load_from_memory(&fs::read(&out).unwrap())
            .unwrap()
            .to_rgb8();
        assert_eq!(image.dimensions(), (32, 48));
        assert!(image.get_pixel(28, 44)[0] > 240);
        assert!(image.get_pixel(3, 3)[2] > 200);
        assert!(image.get_pixel(16, 24)[1] > 90);
        fs::remove_file(root.join("object.svg")).unwrap();
        fs::remove_file(root.join("object-front.svg")).unwrap();
        fs::write(root.join("pandoc.yaml"), "metadata:\n  title: Plain\n").unwrap();
        generate(&data_dir, &root, &out).unwrap();
        fs::remove_dir_all(data_dir).unwrap();
    }
}
