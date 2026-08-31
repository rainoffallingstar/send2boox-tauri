use resvg::tiny_skia;
use resvg::usvg::{Options, Tree};
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let (svg_path, png_path) = match args.len() {
        3 => (&args[1], &args[2]),
        _ => return Err("usage: render_svg <input.svg> <output.png>".into()),
    };

    let svg_data = fs::read_to_string(svg_path)?;
    let opt = Options::default();
    let tree = Tree::from_str(&svg_data, &opt)?;

    let pixmap_size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(pixmap_size.width(), pixmap_size.height())
        .ok_or("Failed to create pixmap")?;

    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    let png_data = pixmap.encode_png()?;
    fs::write(png_path, &png_data)?;
    println!("Rendered {} -> {} ({}x{})", svg_path, png_path, pixmap_size.width(), pixmap_size.height());
    Ok(())
}
