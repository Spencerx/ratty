use ratatui::style::Color as TuiColor;

pub const WINDOW_WIDTH: f32 = 960.0;
pub const WINDOW_HEIGHT: f32 = 620.0;
pub const DEFAULT_COLS: u16 = 104;
pub const DEFAULT_ROWS: u16 = 32;
pub const TERMINAL_SCROLLBACK: usize = 10_000;
pub const CURSOR_DEPTH: f32 = 10.0;
pub const CURSOR_SCALE_FACTOR: f32 = 6.;
pub const CURSOR_X_OFFSET_CELLS: f32 = 0.1;
pub const CURSOR_PLANE_OFFSET: f32 = 18.0;
pub const TERMINAL_FONT_SIZE: i32 = 14;
pub const TERMINAL_FONT_FAMILY_NAME: &str = "Ratty Mono";
pub const TERMINAL_TEXTURE_LABEL: &str = "ratty.parley_ratatui";

pub const THEME_BG_RGB: (u8, u8, u8) = (31, 31, 40);
pub const THEME_FG_RGB: (u8, u8, u8) = (220, 215, 186);
pub const THEME_CURSOR_RGB: (u8, u8, u8) = (126, 156, 216);

pub const THEME_BG: TuiColor = TuiColor::Rgb(THEME_BG_RGB.0, THEME_BG_RGB.1, THEME_BG_RGB.2);
pub const THEME_FG: TuiColor = TuiColor::Rgb(THEME_FG_RGB.0, THEME_FG_RGB.1, THEME_FG_RGB.2);
