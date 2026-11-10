//! One place resolves a terminal cell to RGB.
//!
//! `Flags::DIM` does not darken a color. `Flags::INVERSE` swaps the resolved
//! foreground and background. Dynamic OSC colors win only where `Colors` has
//! `Some`.

use rs_term_core::{AnsiName, CellColor, Rgb, TerminalPalette};

const fn level(index: u8) -> u8 {
    if index == 0 {
        0
    } else {
        55 + 40 * index
    }
}

pub fn xterm_cube(index: u8) -> Rgb {
    if index >= 232 {
        let gray = 8 + (index - 232) * 10;
        return Rgb {
            r: gray,
            g: gray,
            b: gray,
        };
    }
    let offset = index.saturating_sub(16);
    Rgb {
        r: level(offset / 36),
        g: level((offset % 36) / 6),
        b: level(offset % 6),
    }
}

pub fn resolve(
    fg: &CellColor,
    bg: &CellColor,
    inverse: bool,
    colors: &[Option<Rgb>],
    palette: &TerminalPalette,
) -> (Rgb, Rgb) {
    let mut fg = resolve_one(fg, colors, palette, Role::Foreground);
    let mut bg = resolve_one(bg, colors, palette, Role::Background);
    if inverse {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg)
}

#[derive(Clone, Copy)]
enum Role {
    Foreground,
    Background,
}

fn resolve_one(
    color: &CellColor,
    colors: &[Option<Rgb>],
    palette: &TerminalPalette,
    role: Role,
) -> Rgb {
    match color {
        CellColor::Rgb(rgb) => *rgb,
        CellColor::Indexed(index) => {
            if let Some(Some(rgb)) = colors.get(*index as usize) {
                *rgb
            } else if *index < 16 {
                palette.ansi[*index as usize]
            } else {
                xterm_cube(*index)
            }
        }
        CellColor::Named(name) => named(*name, colors, palette, role),
    }
}

fn named(name: AnsiName, colors: &[Option<Rgb>], palette: &TerminalPalette, role: Role) -> Rgb {
    let ansi = |index: usize| palette.ansi[index];
    match name {
        AnsiName::Black => ansi(0),
        AnsiName::Red => ansi(1),
        AnsiName::Green => ansi(2),
        AnsiName::Yellow => ansi(3),
        AnsiName::Blue => ansi(4),
        AnsiName::Magenta => ansi(5),
        AnsiName::Cyan => ansi(6),
        AnsiName::White => ansi(7),
        AnsiName::BrightBlack => ansi(8),
        AnsiName::BrightRed => ansi(9),
        AnsiName::BrightGreen => ansi(10),
        AnsiName::BrightYellow => ansi(11),
        AnsiName::BrightBlue => ansi(12),
        AnsiName::BrightMagenta => ansi(13),
        AnsiName::BrightCyan => ansi(14),
        AnsiName::BrightWhite => ansi(15),
        AnsiName::Foreground | AnsiName::BrightForeground => {
            override_or(colors, 256, palette.foreground)
        }
        AnsiName::Background => override_or(colors, 257, palette.background),
        AnsiName::Cursor => override_or(colors, 258, palette.cursor),
        AnsiName::DimForeground => override_or(
            colors,
            268,
            match role {
                Role::Foreground => palette.foreground,
                Role::Background => palette.background,
            },
        ),
        AnsiName::DimBlack => override_or(colors, 259, ansi(0)),
        AnsiName::DimRed => override_or(colors, 260, ansi(1)),
        AnsiName::DimGreen => override_or(colors, 261, ansi(2)),
        AnsiName::DimYellow => override_or(colors, 262, ansi(3)),
        AnsiName::DimBlue => override_or(colors, 263, ansi(4)),
        AnsiName::DimMagenta => override_or(colors, 264, ansi(5)),
        AnsiName::DimCyan => override_or(colors, 265, ansi(6)),
        AnsiName::DimWhite => override_or(colors, 266, ansi(7)),
    }
}

fn override_or(colors: &[Option<Rgb>], index: usize, fallback: Rgb) -> Rgb {
    colors.get(index).copied().flatten().unwrap_or(fallback)
}

pub fn resolve_font<'a>(available: &'a [String], requested: &str) -> Option<&'a str> {
    for name in [requested, "Consolas", "Cascadia Mono"] {
        if name.is_empty() {
            continue;
        }
        if let Some(found) = available.iter().find(|item| item.as_str() == name) {
            return Some(found.as_str());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rs_term_core::classic_dark;

    #[test]
    fn indexed_override_beats_the_palette_and_inverse_swaps() {
        let palette = classic_dark();
        let mut colors = vec![None; 269];
        colors[1] = Some(Rgb { r: 1, g: 2, b: 3 });
        let (fg, bg) = resolve(
            &CellColor::Indexed(1),
            &CellColor::Named(AnsiName::Background),
            true,
            &colors,
            &palette,
        );
        assert_eq!(fg, palette.background);
        assert_eq!(bg, Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn dim_flag_is_not_a_darker_rgb_and_dim_override_is() {
        let palette = classic_dark();
        let colors = vec![None; 269];
        let (fg, _) = resolve(
            &CellColor::Named(AnsiName::Red),
            &CellColor::Named(AnsiName::Background),
            false,
            &colors,
            &palette,
        );
        assert_eq!(fg, palette.ansi[1]);
        let mut colors = vec![None; 269];
        colors[260] = Some(Rgb { r: 9, g: 9, b: 9 });
        let (fg, _) = resolve(
            &CellColor::Named(AnsiName::DimRed),
            &CellColor::Named(AnsiName::Background),
            false,
            &colors,
            &palette,
        );
        assert_eq!(fg, Rgb { r: 9, g: 9, b: 9 });
    }

    #[test]
    fn cube_and_gray_are_the_xterm_table() {
        assert_eq!(xterm_cube(16), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            xterm_cube(255),
            Rgb {
                r: 238,
                g: 238,
                b: 238
            }
        );
    }

    #[test]
    fn font_resolve_skips_missing_names_and_the_system_font() {
        let available = vec!["Cascadia Mono".into(), ".SystemUIFont".into()];
        assert_eq!(resolve_font(&available, "Missing"), Some("Cascadia Mono"));
        assert_eq!(resolve_font(&[".SystemUIFont".into()], ""), None);
    }
}
