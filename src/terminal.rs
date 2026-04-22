use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use ratatui::Terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color as TuiColor, Modifier, Style};
use ratatui::widgets::Widget;
use soft_ratatui::{ParleyText, SoftBackend};

use crate::config::{TERMINAL_FONT_SIZE, THEME_BG, THEME_FG};
use crate::mouse::TerminalSelection;

static TERMINAL_FONT_DATA: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/fonts/JetBrainsMonoNerdFontCompleteMono.ttf"
));

#[derive(Resource)]
pub struct TerminalRedrawState {
    needs_redraw: bool,
}

impl Default for TerminalRedrawState {
    fn default() -> Self {
        Self { needs_redraw: true }
    }
}

impl TerminalRedrawState {
    pub fn request(&mut self) {
        self.needs_redraw = true;
    }

    pub fn take(&mut self) -> bool {
        std::mem::take(&mut self.needs_redraw)
    }
}

pub struct TerminalSurface {
    pub tui: Terminal<SoftBackend<ParleyText>>,
    pub image_handle: Option<Handle<Image>>,
    pub cols: u16,
    pub rows: u16,
}

impl TerminalSurface {
    pub fn new(cols: u16, rows: u16) -> Self {
        let backend =
            SoftBackend::<ParleyText>::new(cols, rows, TERMINAL_FONT_SIZE, TERMINAL_FONT_DATA);

        let mut tui =
            Terminal::new(backend).expect("soft_ratatui backend is infallible for Terminal::new");
        let _ = tui.clear();
        tui.backend_mut().cursor = false;

        Self {
            tui,
            image_handle: None,
            cols,
            rows,
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }

        self.tui.backend_mut().resize(cols, rows);
        let _ = self.tui.resize(Rect::new(0, 0, cols, rows));
        self.tui.backend_mut().cursor = false;
        self.cols = cols;
        self.rows = rows;
    }

    pub fn sync_image(&self, images: &mut Assets<Image>) {
        let Some(handle) = self.image_handle.as_ref() else {
            return;
        };
        let Some(image) = images.get_mut(handle) else {
            return;
        };

        let width = self.tui.backend().get_pixmap_width() as u32;
        let height = self.tui.backend().get_pixmap_height() as u32;
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

        let rgb = self.tui.backend().get_pixmap_data();
        for (dst, src) in data.chunks_exact_mut(4).zip(rgb.chunks_exact(3)) {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2] = src[2];
            dst[3] = 255;
        }
    }
}

pub struct TerminalWidget<'a> {
    pub screen: &'a vt100::Screen,
    pub selection: &'a TerminalSelection,
}

impl Widget for TerminalWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        buf.set_style(area, Style::default().fg(THEME_FG).bg(THEME_BG));

        let selection = self.selection.normalized_bounds();
        let (rows, cols) = self.screen.size();
        let draw_rows = rows.min(area.height);
        let draw_cols = cols.min(area.width);

        for row in 0..draw_rows {
            for col in 0..draw_cols {
                let Some(vt_cell) = self.screen.cell(row, col) else {
                    continue;
                };
                if vt_cell.is_wide_continuation() {
                    continue;
                }

                let mut style = vt100_cell_style(vt_cell);
                let symbol = if vt_cell.has_contents() {
                    vt_cell.contents()
                } else {
                    " "
                };

                if selection.is_some_and(|bounds| bounds.contains(row, col)) {
                    style = style.add_modifier(Modifier::REVERSED);
                }

                buf[(area.x + col, area.y + row)]
                    .set_symbol(symbol)
                    .set_style(style);
            }
        }
    }
}

fn vt100_cell_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default()
        .fg(vt100_color_to_tui(cell.fgcolor()).unwrap_or(THEME_FG))
        .bg(vt100_color_to_tui(cell.bgcolor()).unwrap_or(THEME_BG));

    let mut modifiers = Modifier::empty();
    if cell.bold() {
        modifiers |= Modifier::BOLD;
    }
    if cell.dim() {
        modifiers |= Modifier::DIM;
    }
    if cell.italic() {
        modifiers |= Modifier::ITALIC;
    }
    if cell.underline() {
        modifiers |= Modifier::UNDERLINED;
    }
    if cell.inverse() {
        modifiers |= Modifier::REVERSED;
    }

    style = style.add_modifier(modifiers);
    style
}

fn vt100_color_to_tui(color: vt100::Color) -> Option<TuiColor> {
    match color {
        vt100::Color::Default => None,
        vt100::Color::Idx(index) => Some(ansi_index_to_tui(index)),
        vt100::Color::Rgb(r, g, b) => Some(TuiColor::Rgb(r, g, b)),
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
