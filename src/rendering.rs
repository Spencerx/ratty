use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use ratatui::style::Color as TuiColor;

use crate::terminal::TerminalSurface;

type Rgba = [u8; 4];
const DEBUG_BG: Rgba = [18, 20, 28, 255];
const DEBUG_GRID: Rgba = [33, 36, 48, 255];
const DEBUG_GRID_OUTLINE: Rgba = [51, 57, 72, 255];
const DEBUG_CURSOR: Rgba = [126, 156, 216, 255];
const DEBUG_FG_FALLBACK: Rgba = [220, 215, 186, 255];
const DEBUG_BG_FALLBACK: Rgba = [31, 31, 40, 255];

pub fn sync_terminal_image(terminal: &TerminalSurface, images: &mut Assets<Image>) {
    let Some(handle) = terminal.image_handle.as_ref() else {
        return;
    };
    let Some(image) = images.get_mut(handle) else {
        return;
    };

    let width = terminal.tui.backend().get_pixmap_width() as u32;
    let height = terminal.tui.backend().get_pixmap_height() as u32;
    let rgba_len = width as usize * height as usize * 4;

    image.resize(Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    });

    let data = image.data.get_or_insert_with(Vec::new);
    if data.len() != rgba_len {
        data.resize(rgba_len, 0);
    }

    let rgb = terminal.tui.backend().get_pixmap_data();
    for (dst, src) in data.chunks_exact_mut(4).zip(rgb.chunks_exact(3)) {
        dst[0] = src[0];
        dst[1] = src[1];
        dst[2] = src[2];
        dst[3] = 255;
    }
}

pub fn sync_terminal_debug_image(
    terminal: &TerminalSurface,
    images: &mut Assets<Image>,
    screen: &vt100::Screen,
) {
    let Some(handle) = terminal.back_image_handle.as_ref() else {
        return;
    };
    let Some(image) = images.get_mut(handle) else {
        return;
    };

    let width = terminal.tui.backend().get_pixmap_width() as u32;
    let height = terminal.tui.backend().get_pixmap_height() as u32;
    let rgba_len = width as usize * height as usize * 4;

    image.resize(Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    });

    let data = image.data.get_or_insert_with(Vec::new);
    if data.len() != rgba_len {
        data.resize(rgba_len, 0);
    }

    CellDebugImageRenderer::new(data, width, height, terminal.cols, terminal.rows).render(screen);
}

pub fn sync_plane_texture<'a>(
    image_handle: Option<&Handle<Image>>,
    material_handles: impl IntoIterator<Item = &'a MeshMaterial3d<StandardMaterial>>,
    materials: &mut Assets<StandardMaterial>,
) {
    let Some(image_handle) = image_handle else {
        return;
    };

    for material_handle in material_handles {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color_texture = Some(image_handle.clone());
        }
    }
}

struct CellDebugImageRenderer<'a> {
    data: &'a mut [u8],
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    cell_width: u32,
    cell_height: u32,
}

impl<'a> CellDebugImageRenderer<'a> {
    fn new(data: &'a mut [u8], width: u32, height: u32, cols: u16, rows: u16) -> Self {
        let cols = cols.max(1) as u32;
        let rows = rows.max(1) as u32;
        let cell_width = (width / cols).max(1);
        let cell_height = (height / rows).max(1);
        Self {
            data,
            width,
            height,
            cols,
            rows,
            cell_width,
            cell_height,
        }
    }

    fn render(&mut self, screen: &vt100::Screen) {
        self.fill(DEBUG_BG);

        for row in 0..self.rows {
            for col in 0..self.cols {
                let rect = self.cell_rect(row, col);
                self.draw_rect(rect, DEBUG_GRID);
                self.draw_rect_outline(rect, DEBUG_GRID_OUTLINE);

                let Some(cell) = screen.cell(row as u16, col as u16) else {
                    continue;
                };

                let bg = vt100_debug_color(cell.bgcolor()).unwrap_or(DEBUG_BG_FALLBACK);
                let fg = vt100_debug_color(cell.fgcolor()).unwrap_or(DEBUG_FG_FALLBACK);
                let active = cell.has_contents() && !cell.is_wide_continuation();
                let fill = if active {
                    bg
                } else {
                    blend_rgba(bg, DEBUG_BG, 0.55)
                };

                self.draw_rect(rect.inset(1), fill);

                if active {
                    let indicator = rect
                        .centered_subrect((rect.width() / 2).max(2), (rect.height() / 2).max(2));
                    self.draw_rect(indicator, fg);
                }

                if cell.underline() {
                    let underline = CellRect {
                        x0: rect.x0.saturating_add(2),
                        y0: rect.y1.saturating_sub(2),
                        x1: rect.x1.saturating_sub(2),
                        y1: rect.y1.saturating_sub(1),
                    };
                    self.draw_rect(underline, fg);
                }

                if cell.bold() {
                    self.draw_rect_outline(rect.inset(1), [255, 255, 255, 90]);
                }
            }
        }

        if !screen.hide_cursor() {
            let (cursor_row, cursor_col) = screen.cursor_position();
            self.draw_rect_outline(
                self.cell_rect(cursor_row as u32, cursor_col as u32),
                DEBUG_CURSOR,
            );
        }
    }

    fn cell_rect(&self, row: u32, col: u32) -> CellRect {
        let draw_col = self.cols - 1 - col;
        let x0 = draw_col * self.cell_width;
        let y0 = row * self.cell_height;
        let x1 = if draw_col + 1 == self.cols {
            self.width
        } else {
            ((draw_col + 1) * self.cell_width).min(self.width)
        };
        let y1 = if row + 1 == self.rows {
            self.height
        } else {
            ((row + 1) * self.cell_height).min(self.height)
        };
        CellRect { x0, y0, x1, y1 }
    }

    fn fill(&mut self, color: Rgba) {
        for pixel in self.data.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    fn draw_rect(&mut self, rect: CellRect, color: Rgba) {
        if rect.x0 >= rect.x1 || rect.y0 >= rect.y1 {
            return;
        }

        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                let idx = ((y * self.width + x) * 4) as usize;
                self.data[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }

    fn draw_rect_outline(&mut self, rect: CellRect, color: Rgba) {
        if rect.x0 >= rect.x1 || rect.y0 >= rect.y1 {
            return;
        }

        self.draw_rect(
            CellRect {
                x0: rect.x0,
                y0: rect.y0,
                x1: rect.x1,
                y1: (rect.y0 + 1).min(rect.y1),
            },
            color,
        );
        self.draw_rect(
            CellRect {
                x0: rect.x0,
                y0: rect.y1.saturating_sub(1),
                x1: rect.x1,
                y1: rect.y1,
            },
            color,
        );
        self.draw_rect(
            CellRect {
                x0: rect.x0,
                y0: rect.y0,
                x1: (rect.x0 + 1).min(rect.x1),
                y1: rect.y1,
            },
            color,
        );
        self.draw_rect(
            CellRect {
                x0: rect.x1.saturating_sub(1),
                y0: rect.y0,
                x1: rect.x1,
                y1: rect.y1,
            },
            color,
        );
    }
}

#[derive(Clone, Copy)]
struct CellRect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl CellRect {
    fn inset(self, amount: u32) -> Self {
        Self {
            x0: self.x0.saturating_add(amount),
            y0: self.y0.saturating_add(amount),
            x1: self.x1.saturating_sub(amount),
            y1: self.y1.saturating_sub(amount),
        }
    }

    fn width(self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    fn height(self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }

    fn centered_subrect(self, width: u32, height: u32) -> Self {
        let x0 = self.x0 + self.width().saturating_sub(width) / 2;
        let y0 = self.y0 + self.height().saturating_sub(height) / 2;
        Self {
            x0,
            y0,
            x1: (x0 + width).min(self.x1),
            y1: (y0 + height).min(self.y1),
        }
    }
}

fn blend_rgba(top: Rgba, bottom: Rgba, top_mix: f32) -> Rgba {
    let bottom_mix = 1.0 - top_mix;
    [
        (top[0] as f32 * top_mix + bottom[0] as f32 * bottom_mix) as u8,
        (top[1] as f32 * top_mix + bottom[1] as f32 * bottom_mix) as u8,
        (top[2] as f32 * top_mix + bottom[2] as f32 * bottom_mix) as u8,
        255,
    ]
}

fn vt100_debug_color(color: vt100::Color) -> Option<Rgba> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => Some(tui_color_to_rgba(ansi_index_to_tui(index))),
        vt100::Color::Rgb(r, g, b) => Some([r, g, b, 255]),
    }
}

fn tui_color_to_rgba(color: TuiColor) -> Rgba {
    match color {
        TuiColor::Black => [0, 0, 0, 255],
        TuiColor::Red => [128, 0, 0, 255],
        TuiColor::Green => [0, 128, 0, 255],
        TuiColor::Yellow => [128, 128, 0, 255],
        TuiColor::Blue => [0, 0, 128, 255],
        TuiColor::Magenta => [128, 0, 128, 255],
        TuiColor::Cyan => [0, 128, 128, 255],
        TuiColor::Gray => [192, 192, 192, 255],
        TuiColor::DarkGray => [128, 128, 128, 255],
        TuiColor::LightRed => [255, 0, 0, 255],
        TuiColor::LightGreen => [0, 255, 0, 255],
        TuiColor::LightYellow => [255, 255, 0, 255],
        TuiColor::LightBlue => [0, 0, 255, 255],
        TuiColor::LightMagenta => [255, 0, 255, 255],
        TuiColor::LightCyan => [0, 255, 255, 255],
        TuiColor::White => [255, 255, 255, 255],
        TuiColor::Rgb(r, g, b) => [r, g, b, 255],
        _ => [220, 215, 186, 255],
    }
}

fn ansi_index_to_tui(index: u8) -> TuiColor {
    match index {
        0 => TuiColor::Black,
        1 => TuiColor::Red,
        2 => TuiColor::Green,
        3 => TuiColor::Yellow,
        4 => TuiColor::Blue,
        5 => TuiColor::Magenta,
        6 => TuiColor::Cyan,
        7 => TuiColor::Gray,
        8 => TuiColor::DarkGray,
        9 => TuiColor::LightRed,
        10 => TuiColor::LightGreen,
        11 => TuiColor::LightYellow,
        12 => TuiColor::LightBlue,
        13 => TuiColor::LightMagenta,
        14 => TuiColor::LightCyan,
        15 => TuiColor::White,
        16..=231 => {
            let index = index - 16;
            let r = index / 36;
            let g = (index % 36) / 6;
            let b = index % 6;
            let component = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            TuiColor::Rgb(component(r), component(g), component(b))
        }
        232..=255 => {
            let shade = 8 + (index - 232) * 10;
            TuiColor::Rgb(shade, shade, shade)
        }
    }
}
