use std::env;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, ensure};
use bevy::app::AppExit;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::texture::ImageSampler;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::Terminal;
use ratatui::widgets::Paragraph;
use soft_ratatui::embedded_graphics_unicodefonts::{
    mono_8x13_atlas, mono_8x13_bold_atlas, mono_8x13_italic_atlas,
};
use soft_ratatui::{EmbeddedGraphics, SoftBackend};
use vte::{Params, Parser, Perform};

const WINDOW_WIDTH: f32 = 1400.0;
const WINDOW_HEIGHT: f32 = 860.0;
const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 36;
const TERMINAL_WORLD_WIDTH: f32 = 14.0;
const CURSOR_DEPTH: f32 = 0.25;

fn main() -> anyhow::Result<()> {
    let runtime = TerminalRuntime::spawn(DEFAULT_COLS, DEFAULT_ROWS)?;
    let soft_terminal = SoftTerminal::new(DEFAULT_COLS, DEFAULT_ROWS);

    App::new()
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(AmbientLight {
            color: Color::WHITE,
            brightness: 150.0,
        })
        .insert_non_send_resource(runtime)
        .insert_non_send_resource(soft_terminal)
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ratterm".into(),
                resolution: (WINDOW_WIDTH, WINDOW_HEIGHT).into(),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(TerminalPlugin)
        .run();

    Ok(())
}

struct TerminalPlugin;

impl Plugin for TerminalPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_scene)
            .add_systems(Update, pump_pty_output)
            .add_systems(Update, handle_keyboard_input.after(pump_pty_output))
            .add_systems(Update, redraw_soft_terminal.after(pump_pty_output))
            .add_systems(Update, sync_cursor_model_transform.after(redraw_soft_terminal));
    }
}

#[derive(Component)]
struct CursorModel;

struct TerminalRuntime {
    rx: Receiver<Vec<u8>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    _master: Box<dyn MasterPty + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    pty_disconnected: bool,
    parser: Parser,
    state: TerminalState,
}

impl TerminalRuntime {
    fn spawn(cols: u16, rows: u16) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to create PTY pair")?;

        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn shell")?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to create PTY writer")?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut buf = [0_u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(size) => {
                        if tx.send(buf[..size].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            rx,
            writer: Arc::new(Mutex::new(writer)),
            _master: pair.master,
            _child: child,
            pty_disconnected: false,
            parser: Parser::new(),
            state: TerminalState::new(cols, rows),
        })
    }

    fn write_input(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
    }
}

struct SoftTerminal {
    terminal: Terminal<SoftBackend<EmbeddedGraphics>>,
    image_handle: Option<Handle<Image>>,
    cols: u16,
    rows: u16,
    plane_size: Vec2,
}

impl SoftTerminal {
    fn new(cols: u16, rows: u16) -> Self {
        let font_regular = mono_8x13_atlas();
        let font_bold = mono_8x13_bold_atlas();
        let font_italic = mono_8x13_italic_atlas();
        let backend = SoftBackend::<EmbeddedGraphics>::new(
            cols,
            rows,
            font_regular,
            Some(font_bold),
            Some(font_italic),
        );
        let pixmap_width = backend.get_pixmap_width() as f32;
        let pixmap_height = backend.get_pixmap_height() as f32;
        let plane_height = TERMINAL_WORLD_WIDTH * (pixmap_height / pixmap_width);

        let mut terminal =
            Terminal::new(backend).expect("soft_ratatui backend is infallible for Terminal::new");
        let _ = terminal.clear();
        terminal.backend_mut().cursor = false;

        Self {
            terminal,
            image_handle: None,
            cols,
            rows,
            plane_size: Vec2::new(TERMINAL_WORLD_WIDTH, plane_height),
        }
    }
}

#[derive(Clone)]
struct TerminalState {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<char>>,
    cursor_x: usize,
    cursor_y: usize,
}

impl TerminalState {
    fn new(cols: u16, rows: u16) -> Self {
        let cols = cols as usize;
        let rows = rows as usize;
        let grid = vec![vec![' '; cols]; rows];

        Self {
            cols,
            rows,
            grid,
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    fn to_multiline_string(&self) -> String {
        let mut output = String::with_capacity((self.cols + 1) * self.rows);
        for row_idx in 0..self.rows {
            for cell in &self.grid[row_idx] {
                output.push(*cell);
            }
            if row_idx + 1 != self.rows {
                output.push('\n');
            }
        }
        output
    }

    fn print(&mut self, ch: char) {
        if self.cols == 0 || self.rows == 0 {
            return;
        }

        if self.cursor_x >= self.cols {
            self.newline();
        }

        if self.cursor_y >= self.rows {
            self.cursor_y = self.rows - 1;
        }

        self.grid[self.cursor_y][self.cursor_x] = ch;
        self.cursor_x += 1;

        if self.cursor_x >= self.cols {
            self.newline();
        }
    }

    fn newline(&mut self) {
        self.cursor_x = 0;

        if self.cursor_y + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_y += 1;
        }
    }

    fn carriage_return(&mut self) {
        self.cursor_x = 0;
    }

    fn backspace(&mut self) {
        if self.cursor_x > 0 {
            self.cursor_x -= 1;
        }
    }

    fn tab(&mut self) {
        let next_tab = ((self.cursor_x / 8) + 1) * 8;
        while self.cursor_x < next_tab {
            self.print(' ');
        }
    }

    fn scroll_up(&mut self) {
        self.grid.remove(0);
        self.grid.push(vec![' '; self.cols]);
    }

    fn move_cursor(&mut self, row: usize, col: usize) {
        self.cursor_y = row.min(self.rows.saturating_sub(1));
        self.cursor_x = col.min(self.cols.saturating_sub(1));
    }

    fn clear_screen(&mut self) {
        for row in &mut self.grid {
            for cell in row {
                *cell = ' ';
            }
        }
        self.move_cursor(0, 0);
    }

    fn erase_display(&mut self, mode: usize) {
        match mode {
            0 => {
                self.erase_line_from_cursor(0);
                for row in (self.cursor_y + 1)..self.rows {
                    self.grid[row].fill(' ');
                }
            }
            1 => {
                self.erase_line_from_cursor(1);
                for row in 0..self.cursor_y {
                    self.grid[row].fill(' ');
                }
            }
            2 => self.clear_screen(),
            _ => {}
        }
    }

    fn erase_line_from_cursor(&mut self, mode: usize) {
        match mode {
            0 => {
                for col in self.cursor_x..self.cols {
                    self.grid[self.cursor_y][col] = ' ';
                }
            }
            1 => {
                for col in 0..=self.cursor_x.min(self.cols.saturating_sub(1)) {
                    self.grid[self.cursor_y][col] = ' ';
                }
            }
            2 => {
                self.grid[self.cursor_y].fill(' ');
            }
            _ => {}
        }
    }
}

struct TerminalPerformer<'a> {
    state: &'a mut TerminalState,
}

impl<'a> TerminalPerformer<'a> {
    fn new(state: &'a mut TerminalState) -> Self {
        Self { state }
    }
}

impl Perform for TerminalPerformer<'_> {
    fn print(&mut self, c: char) {
        self.state.print(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.state.newline(),
            b'\r' => self.state.carriage_return(),
            b'\t' => self.state.tab(),
            0x08 => self.state.backspace(),
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let p0 = param(params, 0).unwrap_or(0);
        let p1 = param(params, 1).unwrap_or(1);

        match action {
            'A' => {
                let amount = p0.max(1);
                let row = self.state.cursor_y.saturating_sub(amount);
                self.state.move_cursor(row, self.state.cursor_x);
            }
            'B' => {
                let amount = p0.max(1);
                self.state
                    .move_cursor(self.state.cursor_y + amount, self.state.cursor_x);
            }
            'C' => {
                let amount = p0.max(1);
                self.state
                    .move_cursor(self.state.cursor_y, self.state.cursor_x + amount);
            }
            'D' => {
                let amount = p0.max(1);
                let col = self.state.cursor_x.saturating_sub(amount);
                self.state.move_cursor(self.state.cursor_y, col);
            }
            'H' | 'f' => {
                let row = p0.max(1) - 1;
                let col = p1.max(1) - 1;
                self.state.move_cursor(row, col);
            }
            'J' => self.state.erase_display(p0),
            'K' => self.state.erase_line_from_cursor(p0),
            'm' => {}
            _ => {}
        }
    }
}

fn param(params: &Params, index: usize) -> Option<usize> {
    params
        .iter()
        .nth(index)
        .and_then(|values| values.first())
        .map(|value| *value as usize)
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut soft_terminal: NonSendMut<SoftTerminal>,
) {
    commands.spawn(Camera3dBundle {
        transform: Transform::from_xyz(0.0, 0.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
        ..default()
    });

    commands.spawn(DirectionalLightBundle {
        directional_light: DirectionalLight {
            illuminance: 12_000.0,
            ..default()
        },
        transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, -0.7, 0.0)),
        ..default()
    });

    let pixmap_width = soft_terminal.terminal.backend().get_pixmap_width() as u32;
    let pixmap_height = soft_terminal.terminal.backend().get_pixmap_height() as u32;
    let initial_rgba = soft_terminal.terminal.backend().get_pixmap_data_as_rgba();

    let mut image = Image::new_fill(
        Extent3d {
            width: pixmap_width,
            height: pixmap_height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.data = initial_rgba;
    image.sampler = ImageSampler::nearest();

    let image_handle = images.add(image);
    soft_terminal.image_handle = Some(image_handle.clone());

    let terminal_plane = meshes.add(
        Plane3d::default()
            .mesh()
            .size(soft_terminal.plane_size.x, soft_terminal.plane_size.y),
    );
    let terminal_material = materials.add(StandardMaterial {
        base_color_texture: Some(image_handle),
        unlit: true,
        cull_mode: None,
        ..default()
    });

    commands.spawn(PbrBundle {
        mesh: terminal_plane,
        material: terminal_material,
        transform: Transform::from_rotation(Quat::from_rotation_x(
            std::f32::consts::FRAC_PI_2,
        )),
        ..default()
    });

    spawn_cursor_model(&mut commands, &mut meshes, &mut materials);
}

fn spawn_cursor_model(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let root = commands.spawn((CursorModel, SpatialBundle::default())).id();
    let model_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.80, 0.30),
        metallic: 0.08,
        perceptual_roughness: 0.48,
        ..default()
    });

    let maybe_obj_path = discover_obj_model_path();
    let maybe_meshes = maybe_obj_path
        .as_ref()
        .map(|path| load_obj_meshes(path).map(|meshes| (path, meshes)));

    match maybe_meshes {
        Some(Ok((path, loaded_meshes))) if !loaded_meshes.is_empty() => {
            info!(
                "loaded cursor model from {} ({} mesh parts)",
                path.display(),
                loaded_meshes.len()
            );
            commands.entity(root).with_children(|parent| {
                for mesh in loaded_meshes {
                    parent.spawn(PbrBundle {
                        mesh: meshes.add(mesh),
                        material: model_material.clone(),
                        ..default()
                    });
                }
            });
        }
        Some(Ok((_path, _))) => {
            warn!("model directory contains an OBJ with no mesh data, using a fallback cursor cube");
            spawn_fallback_cursor_cube(commands, root, meshes, model_material);
        }
        Some(Err(error)) => {
            warn!("failed to load OBJ cursor model: {error:#}");
            spawn_fallback_cursor_cube(commands, root, meshes, model_material);
        }
        None => {
            warn!("no OBJ file found in model/; using a fallback cursor cube");
            spawn_fallback_cursor_cube(commands, root, meshes, model_material);
        }
    }
}

fn spawn_fallback_cursor_cube(
    commands: &mut Commands,
    root: Entity,
    meshes: &mut Assets<Mesh>,
    material: Handle<StandardMaterial>,
) {
    commands.entity(root).with_children(|parent| {
        parent.spawn(PbrBundle {
            mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            material,
            ..default()
        });
    });
}

fn discover_obj_model_path() -> Option<PathBuf> {
    let entries = fs::read_dir("model").ok()?;
    let mut candidates = Vec::new();

    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        let is_obj = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("obj"))
            .unwrap_or(false);
        if is_obj {
            candidates.push(path);
        }
    }

    candidates.sort();
    candidates.into_iter().next()
}

fn load_obj_meshes(path: &Path) -> anyhow::Result<Vec<Mesh>> {
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..default()
    };
    let (models, _materials) =
        tobj::load_obj(path, &options).with_context(|| format!("failed to read {}", path.display()))?;

    let mut output = Vec::new();
    for model in models {
        let source_mesh = model.mesh;
        if source_mesh.positions.is_empty() {
            continue;
        }

        let mut positions = Vec::<[f32; 3]>::with_capacity(source_mesh.positions.len() / 3);
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);

        for pos in source_mesh.positions.chunks_exact(3) {
            let point = Vec3::new(pos[0], pos[1], pos[2]);
            min = min.min(point);
            max = max.max(point);
            positions.push([point.x, point.y, point.z]);
        }

        let center = (min + max) * 0.5;
        let extent = max - min;
        let max_extent = extent.max_element().max(1e-6);

        for p in &mut positions {
            p[0] = (p[0] - center.x) / max_extent;
            p[1] = (p[1] - center.y) / max_extent;
            p[2] = (p[2] - center.z) / max_extent;
        }

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);

        if !source_mesh.normals.is_empty() {
            let normals = source_mesh
                .normals
                .chunks_exact(3)
                .map(|normal| [normal[0], normal[1], normal[2]])
                .collect::<Vec<[f32; 3]>>();
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        }

        if !source_mesh.texcoords.is_empty() {
            let uvs = source_mesh
                .texcoords
                .chunks_exact(2)
                .map(|uv| [uv[0], 1.0 - uv[1]])
                .collect::<Vec<[f32; 2]>>();
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        }

        mesh.insert_indices(Indices::U32(source_mesh.indices));
        output.push(mesh);
    }

    ensure!(!output.is_empty(), "no mesh content inside {}", path.display());
    Ok(output)
}

fn pump_pty_output(mut runtime: NonSendMut<TerminalRuntime>, mut app_exit: EventWriter<AppExit>) {
    let TerminalRuntime {
        rx,
        parser,
        state,
        pty_disconnected,
        ..
    } = &mut *runtime;

    loop {
        match rx.try_recv() {
            Ok(chunk) => {
                let mut performer = TerminalPerformer::new(state);
                for byte in chunk {
                    parser.advance(&mut performer, byte);
                }
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                if !*pty_disconnected {
                    *pty_disconnected = true;
                    app_exit.send(AppExit::Success);
                }
                break;
            }
        }
    }
}

fn handle_keyboard_input(
    mut keyboard_events: EventReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    runtime: NonSend<TerminalRuntime>,
) {
    let mut input = Vec::new();

    for event in keyboard_events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }

        let ctrl_pressed =
            keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        match &event.logical_key {
            Key::Character(chars) => {
                if ctrl_pressed {
                    if let Some(byte) = ctrl_character_byte(chars) {
                        input.push(byte);
                    }
                } else {
                    input.extend_from_slice(chars.as_bytes());
                }
            }
            Key::Enter => input.push(b'\r'),
            Key::Tab => input.push(b'\t'),
            Key::Backspace => input.push(0x7f),
            Key::ArrowUp => input.extend_from_slice(b"\x1b[A"),
            Key::ArrowDown => input.extend_from_slice(b"\x1b[B"),
            Key::ArrowRight => input.extend_from_slice(b"\x1b[C"),
            Key::ArrowLeft => input.extend_from_slice(b"\x1b[D"),
            Key::Delete => input.extend_from_slice(b"\x1b[3~"),
            Key::Home => input.extend_from_slice(b"\x1b[H"),
            Key::End => input.extend_from_slice(b"\x1b[F"),
            Key::PageUp => input.extend_from_slice(b"\x1b[5~"),
            Key::PageDown => input.extend_from_slice(b"\x1b[6~"),
            Key::Escape => input.push(0x1b),
            _ => {}
        }
    }

    runtime.write_input(&input);
}

fn redraw_soft_terminal(
    runtime: NonSend<TerminalRuntime>,
    mut soft_terminal: NonSendMut<SoftTerminal>,
    mut images: ResMut<Assets<Image>>,
) {
    let text = runtime.state.to_multiline_string();
    let cursor_x = runtime
        .state
        .cursor_x
        .min(soft_terminal.cols.saturating_sub(1) as usize) as u16;
    let cursor_y = runtime
        .state
        .cursor_y
        .min(soft_terminal.rows.saturating_sub(1) as usize) as u16;

    let _ = soft_terminal.terminal.draw(|frame| {
        let area = frame.area();
        frame.render_widget(Paragraph::new(text.as_str()), area);
        frame.set_cursor_position((cursor_x, cursor_y));
    });

    if let Some(handle) = soft_terminal.image_handle.as_ref()
        && let Some(image) = images.get_mut(handle)
    {
        image.data = soft_terminal.terminal.backend().get_pixmap_data_as_rgba();
    }
}

fn sync_cursor_model_transform(
    runtime: NonSend<TerminalRuntime>,
    soft_terminal: NonSend<SoftTerminal>,
    time: Res<Time>,
    mut cursor_model_query: Query<&mut Transform, With<CursorModel>>,
) {
    let cols = soft_terminal.cols.max(1) as f32;
    let rows = soft_terminal.rows.max(1) as f32;
    let cell_width = soft_terminal.plane_size.x / cols;
    let cell_height = soft_terminal.plane_size.y / rows;

    let cursor_col = runtime
        .state
        .cursor_x
        .min(soft_terminal.cols.saturating_sub(1) as usize) as f32;
    let cursor_row = runtime
        .state
        .cursor_y
        .min(soft_terminal.rows.saturating_sub(1) as usize) as f32;

    let world_x = -soft_terminal.plane_size.x * 0.5 + (cursor_col + 0.5) * cell_width;
    let world_y = soft_terminal.plane_size.y * 0.5 - (cursor_row + 0.5) * cell_height;
    let spin = time.elapsed_seconds() * 1.25;

    for mut transform in &mut cursor_model_query {
        transform.translation = Vec3::new(world_x, world_y, CURSOR_DEPTH);
        transform.rotation = Quat::from_rotation_y(spin) * Quat::from_rotation_x(-0.35);
        transform.scale = Vec3::splat(cell_width.min(cell_height) * 0.78);
    }
}

fn ctrl_character_byte(chars: &str) -> Option<u8> {
    let ch = chars.chars().next()?.to_ascii_lowercase();
    if !ch.is_ascii_lowercase() {
        return None;
    }
    Some((ch as u8) - b'a' + 1)
}
