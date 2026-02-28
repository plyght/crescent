use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    pub fn to_rgba(self) -> [u8; 4] {
        [self.r, self.g, self.b, 255]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    #[serde(rename = "char")]
    pub ch: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPosition {
    pub row: u16,
    pub col: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridSize {
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Grid {
    pub cells: Vec<Vec<Cell>>,
    pub cursor: CursorPosition,
    pub size: GridSize,
}

impl Grid {
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for (i, row) in self.cells.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let line: String = row
                .iter()
                .map(|c| {
                    if c.ch.is_empty() {
                        ' '
                    } else {
                        c.ch.chars().next().unwrap_or(' ')
                    }
                })
                .collect();
            out.push_str(line.trim_end());
        }
        out
    }
}

const ANSI_STANDARD: [Rgb; 16] = [
    Rgb::new(0, 0, 0),       // 0  black
    Rgb::new(128, 0, 0),     // 1  red
    Rgb::new(0, 128, 0),     // 2  green
    Rgb::new(128, 128, 0),   // 3  yellow
    Rgb::new(0, 0, 128),     // 4  blue
    Rgb::new(128, 0, 128),   // 5  magenta
    Rgb::new(0, 128, 128),   // 6  cyan
    Rgb::new(192, 192, 192), // 7  white
    Rgb::new(128, 128, 128), // 8  bright black
    Rgb::new(255, 0, 0),     // 9  bright red
    Rgb::new(0, 255, 0),     // 10 bright green
    Rgb::new(255, 255, 0),   // 11 bright yellow
    Rgb::new(0, 0, 255),     // 12 bright blue
    Rgb::new(255, 0, 255),   // 13 bright magenta
    Rgb::new(0, 255, 255),   // 14 bright cyan
    Rgb::new(255, 255, 255), // 15 bright white
];

fn idx_to_rgb(idx: u8) -> Rgb {
    if idx < 16 {
        return ANSI_STANDARD[idx as usize];
    }
    if idx >= 232 {
        let gray = 8 + 10 * (idx - 232) as u16;
        let g = gray.min(255) as u8;
        return Rgb::new(g, g, g);
    }
    // 6×6×6 color cube: indices 16..=231
    let idx = idx - 16;
    let b_idx = idx % 6;
    let g_idx = (idx / 6) % 6;
    let r_idx = idx / 36;
    let to_val = |n: u8| if n == 0 { 0u8 } else { 55 + 40 * n };
    Rgb::new(to_val(r_idx), to_val(g_idx), to_val(b_idx))
}

pub fn vt100_color_to_rgb(color: vt100::Color, is_fg: bool) -> Rgb {
    match color {
        vt100::Color::Default => {
            if is_fg {
                Rgb::WHITE
            } else {
                Rgb::BLACK
            }
        }
        vt100::Color::Idx(idx) => idx_to_rgb(idx),
        vt100::Color::Rgb(r, g, b) => Rgb::new(r, g, b),
    }
}

pub fn extract_grid(screen: &vt100::Screen) -> Grid {
    let (rows, cols) = screen.size();
    let (cursor_row, cursor_col) = screen.cursor_position();
    let mut cells = Vec::with_capacity(rows as usize);

    for row in 0..rows {
        let mut row_cells = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            let cell = if let Some(c) = screen.cell(row, col) {
                let ch = c.contents().to_string();
                let (fg_raw, bg_raw) = if c.inverse() {
                    (c.bgcolor(), c.fgcolor())
                } else {
                    (c.fgcolor(), c.bgcolor())
                };
                Cell {
                    ch,
                    fg: vt100_color_to_rgb(fg_raw, true),
                    bg: vt100_color_to_rgb(bg_raw, false),
                    bold: c.bold(),
                    dim: c.dim(),
                    italic: c.italic(),
                    underline: c.underline(),
                    inverse: c.inverse(),
                }
            } else {
                Cell {
                    ch: String::new(),
                    fg: Rgb::WHITE,
                    bg: Rgb::BLACK,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                    inverse: false,
                }
            };
            row_cells.push(cell);
        }
        cells.push(row_cells);
    }

    Grid {
        cells,
        cursor: CursorPosition {
            row: cursor_row,
            col: cursor_col,
        },
        size: GridSize { rows, cols },
    }
}
