//! Shared palette. Capability conversion is applied once to the completed frame.
use ratatui::{buffer::Buffer, style::Color};

pub const TEXT: Color = Color::Rgb(220, 227, 237);
pub const MUTED: Color = Color::Rgb(145, 160, 181);
pub const ACCENT: Color = Color::Rgb(125, 211, 252);
pub const ACTIVE: Color = Color::Rgb(165, 180, 252);
pub const SUCCESS: Color = Color::Rgb(74, 222, 128);
pub const WARNING: Color = Color::Rgb(251, 191, 36);
pub const ERROR: Color = Color::Rgb(251, 113, 133);
pub const GIT: Color = Color::Rgb(245, 158, 139);
pub const NUMBER: Color = Color::Rgb(196, 181, 253);
pub const BORDER: Color = Color::Rgb(71, 85, 105);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    TrueColor,
    Ansi,
    None,
}
impl ColorMode {
    pub fn detect() -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self::None;
        }
        let term = std::env::var("COLORTERM").unwrap_or_default();
        if matches!(term.as_str(), "truecolor" | "24bit")
            || std::env::var("TERM")
                .unwrap_or_default()
                .ends_with("-direct")
        {
            Self::TrueColor
        } else {
            Self::Ansi
        }
    }
    pub fn color(self, color: Color) -> Color {
        match self {
            Self::TrueColor => color,
            Self::None => Color::Reset,
            Self::Ansi => match color {
                TEXT => Color::White,
                MUTED => Color::Gray,
                ACCENT => Color::Cyan,
                ACTIVE | NUMBER => Color::LightMagenta,
                SUCCESS => Color::Green,
                WARNING => Color::Yellow,
                ERROR => Color::LightRed,
                GIT => Color::LightYellow,
                BORDER => Color::DarkGray,
                other => other,
            },
        }
    }
}

pub fn apply(buffer: &mut Buffer, mode: ColorMode) {
    if mode == ColorMode::TrueColor {
        return;
    }
    for cell in &mut buffer.content {
        cell.fg = mode.color(cell.fg);
        cell.bg = mode.color(cell.bg);
        cell.underline_color = mode.color(cell.underline_color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_and_monochrome_keep_symbols_and_modifiers() {
        let mut b = Buffer::empty(ratatui::layout::Rect::new(0, 0, 2, 1));
        b[(0, 0)]
            .set_symbol(">")
            .set_fg(ACTIVE)
            .set_style(ratatui::style::Modifier::BOLD);
        apply(&mut b, ColorMode::Ansi);
        assert_eq!(b[(0, 0)].fg, Color::LightMagenta);
        apply(&mut b, ColorMode::None);
        assert_eq!(b[(0, 0)].fg, Color::Reset);
        assert_eq!(b[(0, 0)].symbol(), ">");
        assert!(b[(0, 0)].modifier.contains(ratatui::style::Modifier::BOLD));
    }
}
