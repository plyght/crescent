use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
    Char(char),
}

#[derive(Debug, Clone, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Enter => write!(f, "Enter"),
            Key::Tab => write!(f, "Tab"),
            Key::Backspace => write!(f, "Backspace"),
            Key::Escape => write!(f, "Escape"),
            Key::Up => write!(f, "Up"),
            Key::Down => write!(f, "Down"),
            Key::Left => write!(f, "Left"),
            Key::Right => write!(f, "Right"),
            Key::Home => write!(f, "Home"),
            Key::End => write!(f, "End"),
            Key::PageUp => write!(f, "PageUp"),
            Key::PageDown => write!(f, "PageDown"),
            Key::Insert => write!(f, "Insert"),
            Key::Delete => write!(f, "Delete"),
            Key::F(n) => write!(f, "F{n}"),
            Key::Char(c) => write!(f, "{c}"),
        }
    }
}

pub fn key_to_escape(key: &Key, mods: &Modifiers) -> Vec<u8> {
    if mods.ctrl {
        if let Key::Char(c) = key {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() {
                return vec![c as u8 - b'a' + 1];
            }
        }
    }

    if mods.alt {
        let base = key_to_escape(key, &Modifiers::default());
        let mut out = vec![0x1b];
        out.extend_from_slice(&base);
        return out;
    }

    match key {
        Key::Enter => vec![b'\r'],
        Key::Tab => {
            if mods.shift {
                b"\x1b[Z".to_vec()
            } else {
                vec![b'\t']
            }
        }
        Key::Backspace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::Up => b"\x1b[A".to_vec(),
        Key::Down => b"\x1b[B".to_vec(),
        Key::Right => b"\x1b[C".to_vec(),
        Key::Left => b"\x1b[D".to_vec(),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::F(n) => f_key_escape(*n),
        Key::Char(c) => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            s.as_bytes().to_vec()
        }
    }
}

fn f_key_escape(n: u8) -> Vec<u8> {
    match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => Vec::new(),
    }
}

pub fn parse_key(name: &str) -> Option<Key> {
    match name.to_lowercase().as_str() {
        "enter" | "return" => Some(Key::Enter),
        "tab" => Some(Key::Tab),
        "backspace" | "bs" => Some(Key::Backspace),
        "escape" | "esc" => Some(Key::Escape),
        "up" | "arrowup" => Some(Key::Up),
        "down" | "arrowdown" => Some(Key::Down),
        "left" | "arrowleft" => Some(Key::Left),
        "right" | "arrowright" => Some(Key::Right),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "insert" => Some(Key::Insert),
        "delete" | "del" => Some(Key::Delete),
        "space" => Some(Key::Char(' ')),
        s if s.starts_with('f') => s[1..].parse::<u8>().ok().map(Key::F),
        s if s.len() == 1 => Some(Key::Char(s.chars().next().unwrap())),
        _ => None,
    }
}

pub fn parse_modifiers(mods: &[String]) -> Modifiers {
    let mut result = Modifiers::default();
    for m in mods {
        match m.to_lowercase().as_str() {
            "ctrl" | "control" => result.ctrl = true,
            "alt" | "option" | "meta" => result.alt = true,
            "shift" => result.shift = true,
            _ => {}
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// SGR mouse press event: \x1b[<button;col;rowM
pub fn sgr_mouse_press(row: u16, col: u16, button: MouseButton) -> Vec<u8> {
    let btn = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    // SGR uses 1-based coordinates
    format!("\x1b[<{btn};{};{}M", col + 1, row + 1).into_bytes()
}

/// SGR mouse release event: \x1b[<button;col;rowm
pub fn sgr_mouse_release(row: u16, col: u16, button: MouseButton) -> Vec<u8> {
    let btn = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    format!("\x1b[<{btn};{};{}m", col + 1, row + 1).into_bytes()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// SGR mouse scroll events
pub fn sgr_scroll(row: u16, col: u16, direction: ScrollDirection) -> Vec<u8> {
    let btn = match direction {
        ScrollDirection::Up => 64,
        ScrollDirection::Down => 65,
    };
    format!("\x1b[<{btn};{};{}M", col + 1, row + 1).into_bytes()
}
