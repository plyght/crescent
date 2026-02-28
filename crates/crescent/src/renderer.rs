use crate::grid::{Grid, Rgb};
use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use anyhow::{Context, Result};
use image::{ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Mutex;

static FONT_DATA: Mutex<Option<Vec<u8>>> = Mutex::new(None);

fn find_system_font() -> Option<PathBuf> {
    let candidates = [
        // macOS
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/SFMono-Regular.otf",
        "/Library/Fonts/SF-Mono-Regular.otf",
        "/System/Library/Fonts/Monaco.dfont",
        // Linux
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
        "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
    ];
    for path in &candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn load_font_data() -> Result<Vec<u8>> {
    let mut guard = FONT_DATA.lock().unwrap();
    if let Some(ref data) = *guard {
        return Ok(data.clone());
    }
    let data = if let Ok(custom) = std::env::var("CRESCENT_FONT") {
        std::fs::read(&custom).with_context(|| format!("failed to read font at {custom}"))?
    } else if let Some(path) = find_system_font() {
        std::fs::read(&path)
            .with_context(|| format!("failed to read font at {}", path.display()))?
    } else {
        anyhow::bail!("no monospace font found. Set CRESCENT_FONT env var to a .ttf/.otf path")
    };
    *guard = Some(data.clone());
    Ok(data)
}

pub struct RendererConfig {
    pub font_size: f32,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self { font_size: 16.0 }
    }
}

pub fn render_grid_to_png(grid: &Grid, config: &RendererConfig) -> Result<Vec<u8>> {
    let font_data = load_font_data()?;
    let font = FontRef::try_from_slice(&font_data)
        .or_else(|_| {
            // .ttc files: try index 0
            ab_glyph::FontRef::try_from_slice_and_index(&font_data, 0)
        })
        .context("failed to parse font")?;

    let scale = PxScale::from(config.font_size);
    let scaled = font.as_scaled(scale);

    let cell_width = scaled.h_advance(scaled.glyph_id('M')).ceil() as u32;
    let cell_height = scaled.height().ceil() as u32;
    let ascent = scaled.ascent();

    if cell_width == 0 || cell_height == 0 {
        anyhow::bail!("font produced zero-size cells");
    }

    let img_w = cell_width * grid.size.cols as u32;
    let img_h = cell_height * grid.size.rows as u32;
    let mut img = RgbaImage::new(img_w, img_h);

    for (row_idx, row) in grid.cells.iter().enumerate() {
        for (col_idx, cell) in row.iter().enumerate() {
            let cx = col_idx as u32 * cell_width;
            let cy = row_idx as u32 * cell_height;

            fill_rect(&mut img, cx, cy, cell_width, cell_height, cell.bg);

            let ch = if cell.ch.is_empty() {
                continue;
            } else {
                cell.ch.chars().next().unwrap_or(' ')
            };
            if ch == ' ' {
                continue;
            }

            let glyph_id = scaled.glyph_id(ch);
            let glyph =
                glyph_id.with_scale_and_position(scale, point(cx as f32, cy as f32 + ascent));

            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                let fg = cell.fg;
                outlined.draw(|gx, gy, coverage| {
                    let px = gx as i32 + bounds.min.x as i32;
                    let py = gy as i32 + bounds.min.y as i32;
                    if px < 0 || py < 0 {
                        return;
                    }
                    let (px, py) = (px as u32, py as u32);
                    if px >= img_w || py >= img_h {
                        return;
                    }
                    let coverage = coverage.clamp(0.0, 1.0);
                    let bg_pixel = *img.get_pixel(px, py);
                    let blended = blend(bg_pixel, fg, coverage);
                    img.put_pixel(px, py, blended);
                });
            }
        }
    }

    let mut buf: Vec<u8> = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .context("failed to encode PNG")?;
    Ok(buf)
}

fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgb) {
    let rgba = Rgba(color.to_rgba());
    let max_x = (x + w).min(img.width());
    let max_y = (y + h).min(img.height());
    for py in y..max_y {
        for px in x..max_x {
            img.put_pixel(px, py, rgba);
        }
    }
}

fn blend(bg: Rgba<u8>, fg: Rgb, coverage: f32) -> Rgba<u8> {
    let inv = 1.0 - coverage;
    Rgba([
        (bg[0] as f32 * inv + fg.r as f32 * coverage) as u8,
        (bg[1] as f32 * inv + fg.g as f32 * coverage) as u8,
        (bg[2] as f32 * inv + fg.b as f32 * coverage) as u8,
        255,
    ])
}
