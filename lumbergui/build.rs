//! Give the executable itself an icon and a name Windows can show.
//!
//! The window icon is set at run time from the same SVG the interface draws.
//! This is the other one: the icon Explorer, the desktop and a shortcut use,
//! which has to be compiled into the binary as a Windows resource before the
//! program ever runs.
//!
//! The `.ico` is rendered here rather than kept beside the source, so there is
//! only ever one drawing of the logo. A committed `.ico` would be a second
//! copy, and a second copy is one that goes stale the first time the first one
//! changes.

use std::path::{Path, PathBuf};

/// The sizes Windows asks for, and which drawing of the logo suits each.
///
/// Below about thirty pixels the stair glyph inside the full mark has no room
/// to be anything but noise, so the small entries use the plain mark instead.
/// This is what a multi size icon is for: the same identity, drawn for the
/// space it is given, rather than one drawing shrunk until it is mush.
const SIZES: [(u32, Mark); 6] = [
    (16, Mark::Plain),
    (24, Mark::Plain),
    (32, Mark::Full),
    (48, Mark::Full),
    (64, Mark::Full),
    (256, Mark::Full),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Full,
    Plain,
}

impl Mark {
    fn file(self) -> &'static str {
        match self {
            Mark::Full => "../assets/Lumberjack.svg",
            Mark::Plain => "../assets/LumberjackBasic.svg",
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=../assets/Lumberjack.svg");
    println!("cargo:rerun-if-changed=../assets/LumberjackBasic.svg");
    println!("cargo:rerun-if-changed=build.rs");

    // Only Windows has a resource to embed. Everywhere else this does nothing
    // and must not fail the build for it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let icon = out.join("lumberjack.ico");

    match render_icon(&icon) {
        Ok(()) => {}
        Err(problem) => {
            // A missing icon is not worth failing a build over: the program
            // runs perfectly well with the system's default one.
            println!("cargo:warning=could not build the application icon: {}", problem);
            return;
        }
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(&icon.to_string_lossy());
    resource.set("FileDescription", "Lumberjack data acquisition");
    resource.set("ProductName", "Lumberjack");

    if let Err(problem) = resource.compile() {
        println!("cargo:warning=could not embed the application icon: {}", problem);
    }
}

/// Draw the logo at every size an icon holds, and pack them into one file.
fn render_icon(into: &Path) -> Result<(), String> {
    let mut images = Vec::new();

    for (size, mark) in SIZES {
        let svg = Path::new(mark.file());
        let data = std::fs::read(svg).map_err(|error| format!("{}: {}", svg.display(), error))?;
        let tree = resvg::usvg::Tree::from_data(&data, &resvg::usvg::Options::default())
            .map_err(|error| error.to_string())?;

        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
            .ok_or_else(|| format!("no pixmap at {}px", size))?;

        let scale = size as f32 / tree.size().width();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );

        images.push((size, pixmap.encode_png().map_err(|error| error.to_string())?));
    }

    std::fs::write(into, ico(&images)).map_err(|error| error.to_string())?;
    Ok(())
}

/// Pack rendered images into an `.ico`.
///
/// A directory of fixed size entries followed by the images themselves. The
/// images are PNGs rather than bitmaps, which every Windows since Vista reads
/// and which saves writing a bitmap encoder for the sake of one file.
fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();

    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // 1 means icon rather than cursor
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());

    // Every entry is sixteen bytes, and the images follow the last of them.
    let mut offset = 6 + 16 * images.len() as u32;

    for (size, png) in images {
        // 256 is written as zero: the field is one byte and 256 will not fit.
        let dimension = if *size >= 256 { 0u8 } else { *size as u8 };

        out.push(dimension); // width
        out.push(dimension); // height
        out.push(0); // colours in the palette; none, being full colour
        out.push(0); // reserved
        out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());

        offset += png.len() as u32;
    }

    for (_, png) in images {
        out.extend_from_slice(png);
    }
    out
}
