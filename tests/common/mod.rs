#![allow(dead_code)]
//! Helpers shared by the integration tests.

use std::path::{Path, PathBuf};

use matrix_display::gfx::Img;
use matrix_display::state::AppState;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn golden_dir() -> PathBuf {
    root().join("tests/golden")
}

pub fn load_png(path: &Path) -> Img {
    let png = image::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display())).to_rgb8();
    Img {
        w: png.width() as usize,
        h: png.height() as usize,
        data: png.pixels().map(|p| p.0).collect(),
    }
}

pub fn save_png(img: &Img, path: &Path) {
    matrix_display::display::emulator::to_rgb_image(img).save(path).unwrap();
}

/// Bounding box (x0, y0, x1, y1) of the pixels that differ, or None when identical.
pub fn diff_bbox(a: &Img, b: &Img) -> Option<(usize, usize, usize, usize)> {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let mut bbox: Option<(usize, usize, usize, usize)> = None;
    for y in 0..a.h {
        for x in 0..a.w {
            if a.get(x, y) != b.get(x, y) {
                bbox = Some(match bbox {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    bbox
}

/// Bounding box of the pixels that are not black.
pub fn content_bbox(img: &Img) -> Option<(usize, usize, usize, usize)> {
    diff_bbox(img, &Img::black(img.w, img.h))
}

/// Compare with a golden PNG; `UPDATE_GOLDEN=1` rewrites it (or creates a missing one).
pub fn assert_golden(img: &Img, name: &str) {
    let path = golden_dir().join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        save_png(img, &path);
    }
    let want = load_png(&path);
    assert_eq!((img.w, img.h), (want.w, want.h), "{name}: size");
    if let Some(bbox) = diff_bbox(img, &want) {
        let n = img.data.iter().zip(&want.data).filter(|(a, b)| a != b).count();
        panic!("{name}: {n} pixels differ, within {bbox:?}");
    }
}

pub fn variant(name: &str) -> AppState {
    matrix_display::ui::variants::variants()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("no variant {name}"))
        .1
}

pub fn count_distinct(img: &Img) -> usize {
    let mut seen = std::collections::HashSet::new();
    seen.extend(img.data.iter().copied());
    seen.len()
}
