use resvg::tiny_skia;
use resvg::usvg::{Options, Tree};
use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let svg_data = fs::read_to_string("src-tauri/icons/icon.svg")?;
    let opt = Options::default();
    let tree = Tree::from_str(&svg_data, &opt)?;

    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or("Failed to create pixmap")?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    let png_data = pixmap.encode_png()?;
    fs::write("src-tauri/icons/icon-1024.png", &png_data)?;
    fs::write("src-tauri/icons/icon.png", &png_data)?;
    println!("Successfully rendered SVG to 1024x1024 PNG ({} bytes)!", png_data.len());
    Ok(())
}
