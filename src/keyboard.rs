use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::runtime::TerminalRuntime;

#[derive(Default)]
pub struct TerminalKeyboard {
    ctrl_pressed: bool,
    alt_pressed: bool,
}

impl TerminalKeyboard {
    pub fn handle_event(&mut self, event: &KeyboardInput) -> Option<Vec<u8>> {
        match event.key_code {
            KeyCode::ControlLeft | KeyCode::ControlRight => {
                self.ctrl_pressed = event.state == ButtonState::Pressed;
                return None;
            }
            KeyCode::AltLeft | KeyCode::AltRight => {
                self.alt_pressed = event.state == ButtonState::Pressed;
                return None;
            }
            _ => {}
        }

        if event.state != ButtonState::Pressed {
            return None;
        }

        Some(translate_key(
            event.key_code,
            &event.logical_key,
            event.text.as_deref(),
            self.ctrl_pressed,
            self.alt_pressed,
        ))
    }
}

pub fn handle_keyboard_input(
    mut keyboard_events: MessageReader<KeyboardInput>,
    mut keyboard: Local<TerminalKeyboard>,
    runtime: NonSend<TerminalRuntime>,
) {
    for event in keyboard_events.read() {
        if let Some(input) = keyboard.handle_event(event) {
            runtime.write_input(&input);
        }
    }
}

fn translate_key(
    key_code: KeyCode,
    logical_key: &Key,
    text: Option<&str>,
    ctrl_pressed: bool,
    alt_pressed: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();

    if ctrl_pressed {
        if let Some(ctrl) = ctrl_keycode_byte(key_code) {
            if alt_pressed {
                bytes.push(0x1b);
            }
            bytes.push(ctrl);
            return bytes;
        }
    }

    if alt_pressed {
        bytes.push(0x1b);
    }

    match key_code {
        KeyCode::Enter | KeyCode::NumpadEnter => bytes.push(b'\r'),
        KeyCode::Tab => bytes.push(b'\t'),
        KeyCode::Space => bytes.push(b' '),
        KeyCode::Backspace => bytes.push(0x7f),
        KeyCode::Escape => bytes.push(0x1b),
        KeyCode::ArrowUp => bytes.extend_from_slice(b"\x1b[A"),
        KeyCode::ArrowDown => bytes.extend_from_slice(b"\x1b[B"),
        KeyCode::ArrowRight => bytes.extend_from_slice(b"\x1b[C"),
        KeyCode::ArrowLeft => bytes.extend_from_slice(b"\x1b[D"),
        KeyCode::Delete => bytes.extend_from_slice(b"\x1b[3~"),
        KeyCode::Home => bytes.extend_from_slice(b"\x1b[H"),
        KeyCode::End => bytes.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => bytes.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => bytes.extend_from_slice(b"\x1b[6~"),
        _ => {
            if let Some(text) = text {
                bytes.extend_from_slice(text.as_bytes());
            } else if let Key::Character(chars) = logical_key {
                bytes.extend_from_slice(chars.as_bytes());
            }
        }
    }

    bytes
}

fn ctrl_keycode_byte(key: KeyCode) -> Option<u8> {
    match key {
        KeyCode::KeyA => Some(0x01),
        KeyCode::KeyB => Some(0x02),
        KeyCode::KeyC => Some(0x03),
        KeyCode::KeyD => Some(0x04),
        KeyCode::KeyE => Some(0x05),
        KeyCode::KeyF => Some(0x06),
        KeyCode::KeyG => Some(0x07),
        KeyCode::KeyH => Some(0x08),
        KeyCode::KeyI => Some(0x09),
        KeyCode::KeyJ => Some(0x0a),
        KeyCode::KeyK => Some(0x0b),
        KeyCode::KeyL => Some(0x0c),
        KeyCode::KeyM => Some(0x0d),
        KeyCode::KeyN => Some(0x0e),
        KeyCode::KeyO => Some(0x0f),
        KeyCode::KeyP => Some(0x10),
        KeyCode::KeyQ => Some(0x11),
        KeyCode::KeyR => Some(0x12),
        KeyCode::KeyS => Some(0x13),
        KeyCode::KeyT => Some(0x14),
        KeyCode::KeyU => Some(0x15),
        KeyCode::KeyV => Some(0x16),
        KeyCode::KeyW => Some(0x17),
        KeyCode::KeyX => Some(0x18),
        KeyCode::KeyY => Some(0x19),
        KeyCode::KeyZ => Some(0x1a),
        _ => None,
    }
}
