use bevy::prelude::*;
use parley_ratatui::ratatui::Terminal;
use parley_ratatui::ratatui::buffer::Buffer;
use parley_ratatui::ratatui::layout::Rect;
use parley_ratatui::ratatui::style::{Color as TuiColor, Modifier, Style};
use parley_ratatui::ratatui::widgets::Widget;
use parley_ratatui::vello::wgpu;
use parley_ratatui::{
    BundledFont, FontOptions, GpuRenderer, ParleyBackend, TerminalRenderer, TextureReadback,
    TextureTarget, Theme,
};

use crate::config::{
    TERMINAL_FONT_FAMILY_NAME, TERMINAL_FONT_SIZE, TERMINAL_TEXTURE_LABEL, THEME_BG,
    THEME_BG_RGB, THEME_CURSOR_RGB, THEME_FG, THEME_FG_RGB,
};
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
    pub tui: Terminal<ParleyBackend>,
    pub image_handle: Option<Handle<Image>>,
    pub back_image_handle: Option<Handle<Image>>,
    pub cols: u16,
    pub rows: u16,
    font_size: i32,
    renderer: TerminalRenderer,
    gpu: OffscreenGpu,
}

struct OffscreenGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: GpuRenderer,
    target: TextureTarget,
    readback: TextureReadback,
    rgba: Vec<u8>,
}

impl OffscreenGpu {
    async fn new(width: u32, height: u32) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|_| anyhow::anyhow!("failed to request wgpu adapter for parley_ratatui"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let target = TextureTarget::new(
            &device,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(TERMINAL_TEXTURE_LABEL),
        );
        let renderer = GpuRenderer::new(&device)?;
        Ok(Self {
            device,
            queue,
            renderer,
            target,
            readback: TextureReadback::new(),
            rgba: Vec::new(),
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if self.target.width == width && self.target.height == height {
            return;
        }

        self.target = TextureTarget::new(
            &self.device,
            width,
            height,
            self.target.format,
            Some(TERMINAL_TEXTURE_LABEL),
        );
    }
}

impl TerminalSurface {
    pub fn new(cols: u16, rows: u16) -> anyhow::Result<Self> {
        let backend = ParleyBackend::new(cols, rows);
        let mut tui = Terminal::new(backend)?;
        let _ = tui.clear();
        tui.hide_cursor()?;
        let renderer = build_terminal_renderer(TERMINAL_FONT_SIZE);
        let (width, height) = renderer.texture_size_for_buffer(tui.backend().buffer());
        let gpu = pollster::block_on(OffscreenGpu::new(width, height))?;

        Ok(Self {
            tui,
            image_handle: None,
            back_image_handle: None,
            cols,
            rows,
            font_size: TERMINAL_FONT_SIZE,
            renderer,
            gpu,
        })
    }

    pub fn adjust_font_size(&mut self, delta: i32) -> bool {
        let new_size = self.font_size + delta;
        if new_size == self.font_size {
            return false;
        }

        self.font_size = new_size;
        self.renderer = build_terminal_renderer(self.font_size);
        let (width, height) = self
            .renderer
            .texture_size_for_buffer(self.tui.backend().buffer());
        self.gpu.resize(width, height);
        true
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }

        self.tui.backend_mut().resize(cols, rows);
        let _ = self.tui.resize(Rect::new(0, 0, cols, rows));
        let _ = self.tui.hide_cursor();
        self.cols = cols;
        self.rows = rows;

        let (width, height) = self
            .renderer
            .texture_size_for_buffer(self.tui.backend().buffer());
        self.gpu.resize(width, height);
    }

    pub fn char_dimensions(&self) -> UVec2 {
        let metrics = self.renderer.metrics();
        UVec2::new(
            metrics.cell_width.ceil().max(1.0) as u32,
            metrics.cell_height.ceil().max(1.0) as u32,
        )
    }

    pub fn pixmap_dimensions(&self) -> UVec2 {
        let (width, height) = self
            .renderer
            .texture_size_for_buffer(self.tui.backend().buffer());
        UVec2::new(width, height)
    }

    pub fn sync_image(
        &mut self,
        images: &mut Assets<Image>,
        elapsed_secs: f32,
    ) -> anyhow::Result<()> {
        let Some(handle) = self.image_handle.as_ref() else {
            return Ok(());
        };
        let Some(image) = images.get_mut(handle) else {
            return Ok(());
        };

        let (width, height) = self
            .renderer
            .texture_size_for_buffer(self.tui.backend().buffer());
        self.gpu.resize(width, height);

        let buffer = self.tui.backend().buffer();
        let cursor = Some(self.tui.backend().cursor_position());
        let cursor_visible = self.tui.backend().cursor_visible();

        self.gpu.renderer.render_to_rgba8_with_elapsed_into(
            &mut self.renderer,
            &mut self.gpu.readback,
            &self.gpu.device,
            &self.gpu.queue,
            &self.gpu.target,
            buffer,
            cursor,
            cursor_visible,
            elapsed_secs,
            &mut self.gpu.rgba,
        )?;

        image.resize(bevy::render::render_resource::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        });
        let data = image.data.get_or_insert_with(Vec::new);
        let target_len = width as usize * height as usize * 4;
        if data.len() != target_len {
            data.resize(target_len, 0);
        }
        if self.gpu.rgba.len() == target_len {
            data.copy_from_slice(&self.gpu.rgba);
        }

        Ok(())
    }
}

fn build_terminal_renderer(font_size: i32) -> TerminalRenderer {
    let theme = Theme {
        foreground: parley_ratatui::Rgba::rgb(THEME_FG_RGB.0, THEME_FG_RGB.1, THEME_FG_RGB.2),
        background: parley_ratatui::Rgba::rgb(THEME_BG_RGB.0, THEME_BG_RGB.1, THEME_BG_RGB.2),
        cursor: parley_ratatui::Rgba::rgb(
            THEME_CURSOR_RGB.0,
            THEME_CURSOR_RGB.1,
            THEME_CURSOR_RGB.2,
        ),
        ..Theme::default()
    };
    let font = BundledFont::from_static(TERMINAL_FONT_DATA).with_family_name(TERMINAL_FONT_FAMILY_NAME);
    let font_options = FontOptions::default().with_font_stack(
        parley_ratatui::FontStack::new(font.clone())
            .with_bold(font.clone())
            .with_italic(font.clone())
            .with_bold_italic(font),
    );
    TerminalRenderer::new(
        FontOptions {
            size: font_size as f32,
            ..font_options
        },
        theme,
    )
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
