//! The built-in color themes, as data.
//!
//! A theme gives the terminal default colors, the 16 ANSI colors, the cursor
//! and the selection, plus the colors of the status line. The modal boxes use
//! the terminal default colors. Colors the data picks itself (256-color and
//! truecolor attributes) are drawn as sent and never come from here.
//!
//! The names are kebab-case and case does not matter. `dark` is the original
//! look and `classic-dark` is kept as another spelling of it. Most of the
//! other names are the ones herdr uses; a few common spellings are aliases.

use crate::screen::Rgb;

/// The theme a fresh install uses.
pub const DEFAULT_THEME: &str = "dark";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiColors {
    pub status_bg: Rgb,
    pub status_fg: Rgb,
    /// The idle hint and the `disconnected` word.
    pub muted: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub error: Rgb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalPalette {
    pub name: &'static str,
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub ansi: [Rgb; 16],
    pub ui: UiColors,
}

/// Status line colors as `[bg, fg, muted, ok, warn, error]`.
type UiSpec = [u32; 6];

struct Spec {
    name: &'static str,
    aliases: &'static [&'static str],
    fg: u32,
    bg: u32,
    cursor: u32,
    selection: u32,
    ansi: [u32; 16],
    /// `None` derives the status line colors from the rest.
    ui: Option<UiSpec>,
}

/// The order here is the order `t` cycles in.
const THEMES: &[Spec] = &[
    Spec {
        name: "dark",
        aliases: &["classic-dark"],
        fg: 0xD4D4D4,
        bg: 0x000000,
        cursor: 0x00C800,
        selection: 0x264F78,
        ansi: [
            0x000000, 0xCD0000, 0x00CD00, 0xCDCD00, 0x0000EE, 0xCD00CD, 0x00CDCD, 0xE5E5E5,
            0x7F7F7F, 0xFF0000, 0x00FF00, 0xFFFF00, 0x5C5CFF, 0xFF00FF, 0x00FFFF, 0xFFFFFF,
        ],
        ui: Some([0x202020, 0xD4D4D4, 0x808080, 0x40C040, 0xFFD700, 0xFF6060]),
    },
    Spec {
        name: "light",
        aliases: &[],
        fg: 0x1E1E1E,
        bg: 0xFFFFFF,
        cursor: 0x006400,
        selection: 0xADD6FF,
        ansi: [
            0x000000, 0xC23030, 0x0E7C0E, 0x8A6D00, 0x0451A5, 0x9B2393, 0x0E7490, 0x555555,
            0x767676, 0xD13438, 0x138A13, 0x946B00, 0x1F5FBF, 0xA63AA6, 0x0F7F95, 0xA5A5A5,
        ],
        ui: Some([0xE8E8E8, 0x1E1E1E, 0x5F5F5F, 0x0E7C0E, 0x8A6000, 0xB3261E]),
    },
    // Source: https://github.com/morhetz/gruvbox (palette) and https://github.com/alacritty/alacritty-theme/blob/master/themes/gruvbox_dark.toml
    Spec {
        name: "gruvbox-dark",
        aliases: &["gruvbox"],
        fg: 0xEBDBB2,
        bg: 0x282828,
        cursor: 0xEBDBB2,
        selection: 0x504945,
        ansi: [
            0x282828, 0xCC241D, 0x98971A, 0xD79921, 0x458588, 0xB16286, 0x689D6A, 0xA89984,
            0x928374, 0xFB4934, 0xB8BB26, 0xFABD2F, 0x83A598, 0xD3869B, 0x8EC07C, 0xEBDBB2,
        ],
        ui: None,
    },
    // Source: https://github.com/morhetz/gruvbox (palette) and https://github.com/alacritty/alacritty-theme/blob/master/themes/gruvbox_light.toml
    Spec {
        name: "gruvbox-light",
        aliases: &[],
        fg: 0x3C3836,
        bg: 0xFBF1C7,
        cursor: 0x3C3836,
        selection: 0xD5C4A1,
        ansi: [
            0xFBF1C7, 0xCC241D, 0x98971A, 0xD79921, 0x458588, 0xB16286, 0x689D6A, 0x7C6F64,
            0x928374, 0x9D0006, 0x79740E, 0xB57614, 0x076678, 0x8F3F71, 0x427B58, 0x3C3836,
        ],
        ui: None,
    },
    // Source: https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_night.toml
    Spec {
        name: "tokyo-night",
        aliases: &["tokyonight"],
        fg: 0xC0CAF5,
        bg: 0x1A1B26,
        cursor: 0xC0CAF5,
        selection: 0x283457,
        ansi: [
            0x15161E, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xA9B1D6,
            0x414868, 0xFF899D, 0x9FE044, 0xFABA4A, 0x8DB0FF, 0xC7A9FF, 0xA4DAFF, 0xC0CAF5,
        ],
        ui: None,
    },
    // Source: https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_storm.toml
    Spec {
        name: "tokyo-night-storm",
        aliases: &["tokyonight-storm"],
        fg: 0xC0CAF5,
        bg: 0x24283B,
        cursor: 0xC0CAF5,
        selection: 0x2E3C64,
        ansi: [
            0x1D202F, 0xF7768E, 0x9ECE6A, 0xE0AF68, 0x7AA2F7, 0xBB9AF7, 0x7DCFFF, 0xA9B1D6,
            0x414868, 0xFF899D, 0x9FE044, 0xFABA4A, 0x8DB0FF, 0xC7A9FF, 0xA4DAFF, 0xC0CAF5,
        ],
        ui: None,
    },
    // Source: https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_day.toml
    Spec {
        name: "tokyo-night-light",
        aliases: &["tokyo-night-day", "tokyo-day", "tokyonight-day"],
        fg: 0x3760BF,
        bg: 0xE1E2E7,
        cursor: 0x3760BF,
        selection: 0xB7C1E3,
        ansi: [
            0xB4B5B9, 0xF52A65, 0x587539, 0x8C6C3E, 0x2E7DE9, 0x9854F1, 0x007197, 0x6172B0,
            0xA1A6C5, 0xFF4774, 0x5C8524, 0xA27629, 0x358AFF, 0xA463FF, 0x007EA8, 0x3760BF,
        ],
        ui: None,
    },
    // Source: https://github.com/catppuccin/alacritty/blob/main/catppuccin-mocha.toml
    Spec {
        name: "catppuccin-mocha",
        aliases: &["catppuccin"],
        fg: 0xCDD6F4,
        bg: 0x1E1E2E,
        cursor: 0xF5E0DC,
        selection: 0x585B70,
        ansi: [
            0x45475A, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xBAC2DE,
            0x585B70, 0xF38BA8, 0xA6E3A1, 0xF9E2AF, 0x89B4FA, 0xF5C2E7, 0x94E2D5, 0xA6ADC8,
        ],
        ui: None,
    },
    // Source: https://github.com/catppuccin/alacritty/blob/main/catppuccin-latte.toml
    Spec {
        name: "catppuccin-latte",
        aliases: &["latte"],
        fg: 0x4C4F69,
        bg: 0xEFF1F5,
        cursor: 0xDC8A78,
        selection: 0xDC8A78,
        ansi: [
            0xBCC0CC, 0xD20F39, 0x40A02B, 0xDF8E1D, 0x1E66F5, 0xEA76CB, 0x179299, 0x5C5F77,
            0xACB0BE, 0xD20F39, 0x40A02B, 0xDF8E1D, 0x1E66F5, 0xEA76CB, 0x179299, 0x6C6F85,
        ],
        ui: None,
    },
    // Source: https://github.com/dracula/alacritty/blob/master/dracula.toml
    Spec {
        name: "dracula",
        aliases: &[],
        fg: 0xF8F8F2,
        bg: 0x282A36,
        cursor: 0xF8F8F2,
        selection: 0x44475A,
        ansi: [
            0x21222C, 0xFF5555, 0x50FA7B, 0xF1FA8C, 0xBD93F9, 0xFF79C6, 0x8BE9FD, 0xF8F8F2,
            0x6272A4, 0xFF6E6E, 0x69FF94, 0xFFFFA5, 0xD6ACFF, 0xFF92DF, 0xA4FFFF, 0xFFFFFF,
        ],
        ui: None,
    },
    // Source: https://github.com/alacritty/alacritty-theme/blob/master/themes/nord.toml (palette: https://www.nordtheme.com)
    Spec {
        name: "nord",
        aliases: &[],
        fg: 0xD8DEE9,
        bg: 0x2E3440,
        cursor: 0xD8DEE9,
        selection: 0x434C5E,
        ansi: [
            0x3B4252, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x88C0D0, 0xE5E9F0,
            0x4C566A, 0xBF616A, 0xA3BE8C, 0xEBCB8B, 0x81A1C1, 0xB48EAD, 0x8FBCBB, 0xECEFF4,
        ],
        ui: None,
    },
    // Source: https://github.com/alacritty/alacritty-theme/blob/master/themes/solarized_dark.toml (palette: https://ethanschoonover.com/solarized)
    Spec {
        name: "solarized-dark",
        aliases: &["solarized"],
        fg: 0x839496,
        bg: 0x002B36,
        cursor: 0x93A1A1,
        selection: 0x073642,
        ansi: [
            0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5,
            0x002B36, 0xCB4B16, 0x586E75, 0x657B83, 0x839496, 0x6C71C4, 0x93A1A1, 0xFDF6E3,
        ],
        ui: None,
    },
    // Source: https://github.com/alacritty/alacritty-theme/blob/master/themes/solarized_light.toml (palette: https://ethanschoonover.com/solarized)
    Spec {
        name: "solarized-light",
        aliases: &[],
        fg: 0x586E75,
        bg: 0xFDF6E3,
        cursor: 0x586E75,
        selection: 0xEEE8D5,
        ansi: [
            0x073642, 0xDC322F, 0x859900, 0xB58900, 0x268BD2, 0xD33682, 0x2AA198, 0xEEE8D5,
            0x002B36, 0xCB4B16, 0x586E75, 0x657B83, 0x839496, 0x6C71C4, 0x93A1A1, 0xFDF6E3,
        ],
        ui: None,
    },
    // Source: https://github.com/alacritty/alacritty-theme/blob/master/themes/one_dark.toml
    Spec {
        name: "one-dark",
        aliases: &["onedark"],
        fg: 0xABB2BF,
        bg: 0x282C34,
        cursor: 0x528BFF,
        selection: 0x3E4451,
        ansi: [
            0x1E2127, 0xE06C75, 0x98C379, 0xD19A66, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF,
            0x5C6370, 0xE06C75, 0x98C379, 0xD19A66, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xFFFFFF,
        ],
        ui: None,
    },
    // Source: https://github.com/alacritty/alacritty-theme/blob/master/themes/one_light.toml
    Spec {
        name: "one-light",
        aliases: &["onelight"],
        fg: 0x2A2B33,
        bg: 0xF8F8F8,
        cursor: 0x526FFF,
        selection: 0xE5E5E6,
        ansi: [
            0x000000, 0xDE3D35, 0x3E953A, 0xD2B67B, 0x2F5AF3, 0xA00095, 0x3E953A, 0xBBBBBB,
            0x000000, 0xDE3D35, 0x3E953A, 0xD2B67B, 0x2F5AF3, 0xA00095, 0x3E953A, 0xFFFFFF,
        ],
        ui: None,
    },
    // Source: https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_wave.toml
    Spec {
        name: "kanagawa",
        aliases: &[],
        fg: 0xDCD7BA,
        bg: 0x1F1F28,
        cursor: 0xC8C093,
        selection: 0x2D4F67,
        ansi: [
            0x090618, 0xC34043, 0x76946A, 0xC0A36E, 0x7E9CD8, 0x957FB8, 0x6A9589, 0xC8C093,
            0x727169, 0xE82424, 0x98BB6C, 0xE6C384, 0x7FB4CA, 0x938AA9, 0x7AA89F, 0xDCD7BA,
        ],
        ui: None,
    },
    // Source: https://github.com/rebelot/kanagawa.nvim/blob/master/extras/alacritty/kanagawa_lotus.toml
    Spec {
        name: "kanagawa-lotus",
        aliases: &["lotus"],
        fg: 0x545464,
        bg: 0xF2ECBC,
        cursor: 0x43436C,
        selection: 0xC9CBD1,
        ansi: [
            0x1F1F28, 0xC84053, 0x6F894E, 0x77713F, 0x4D699B, 0xB35B79, 0x597B75, 0x545464,
            0x8A8980, 0xD7474B, 0x6E915F, 0x836F4A, 0x6693BF, 0x624C83, 0x5E857A, 0x43436C,
        ],
        ui: None,
    },
    // Source: https://github.com/rose-pine/alacritty/blob/main/dist/rose-pine.toml
    Spec {
        name: "rose-pine",
        aliases: &["rosepine"],
        fg: 0xE0DEF4,
        bg: 0x191724,
        cursor: 0x524F67,
        selection: 0x403D52,
        ansi: [
            0x26233A, 0xEB6F92, 0x31748F, 0xF6C177, 0x9CCFD8, 0xC4A7E7, 0xEBBCBA, 0xE0DEF4,
            0x6E6A86, 0xEB6F92, 0x31748F, 0xF6C177, 0x9CCFD8, 0xC4A7E7, 0xEBBCBA, 0xE0DEF4,
        ],
        ui: None,
    },
    // Source: https://github.com/rose-pine/alacritty/blob/main/dist/rose-pine-dawn.toml
    Spec {
        name: "rose-pine-dawn",
        aliases: &["rosepine-dawn", "dawn"],
        fg: 0x575279,
        bg: 0xFAF4ED,
        cursor: 0xCECACD,
        selection: 0xDFDAD9,
        ansi: [
            0xF2E9E1, 0xB4637A, 0x286983, 0xEA9D34, 0x56949F, 0x907AA9, 0xD7827E, 0x575279,
            0x9893A5, 0xB4637A, 0x286983, 0xEA9D34, 0x56949F, 0x907AA9, 0xD7827E, 0x575279,
        ],
        ui: None,
    },
    // Source: https://github.com/raunofreiberg/vesper (via https://github.com/alacritty/alacritty-theme/blob/master/themes/vesper.toml)
    Spec {
        name: "vesper",
        aliases: &[],
        fg: 0xFFFFFF,
        bg: 0x101010,
        cursor: 0xFFC799,
        selection: 0x2A2A2A,
        ansi: [
            0x101010, 0xF5A191, 0x90B99F, 0xE6B99D, 0xACA1CF, 0xE29ECA, 0xEA83A5, 0xA0A0A0,
            0x7E7E7E, 0xFF8080, 0x99FFE4, 0xFFC799, 0xB9AEDA, 0xECAAD6, 0xF591B2, 0xFFFFFF,
        ],
        ui: None,
    },
];

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

/// `from` moved `part` of the way (0.0 to 1.0) towards `to`.
fn mix(from: Rgb, to: Rgb, part: f32) -> Rgb {
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * part).round() as u8;
    Rgb {
        r: channel(from.r, to.r),
        g: channel(from.g, to.g),
        b: channel(from.b, to.b),
    }
}

/// WCAG relative luminance.
fn luminance(color: Rgb) -> f32 {
    let linear = |value: u8| {
        let value = value as f32 / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

/// WCAG contrast ratio, 1.0 (same color) to 21.0 (black on white).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Whichever of the normal and bright color reads better on `on`. One that
/// is still under a ratio of 3 is pulled towards `towards` until it is not.
fn readable(normal: Rgb, bright: Rgb, on: Rgb, towards: Rgb) -> Rgb {
    let mut color = if contrast_ratio(bright, on) > contrast_ratio(normal, on) {
        bright
    } else {
        normal
    };
    for _ in 0..10 {
        if contrast_ratio(color, on) >= 3.0 {
            break;
        }
        color = mix(color, towards, 0.15);
    }
    color
}

fn build(spec: &Spec) -> TerminalPalette {
    let ansi = spec.ansi.map(rgb);
    let (foreground, background) = (rgb(spec.fg), rgb(spec.bg));
    let ui = match spec.ui {
        Some([bg, fg, muted, ok, warn, error]) => UiColors {
            status_bg: rgb(bg),
            status_fg: rgb(fg),
            muted: rgb(muted),
            ok: rgb(ok),
            warn: rgb(warn),
            error: rgb(error),
        },
        None => {
            let status_bg = mix(background, foreground, 0.10);
            UiColors {
                status_bg,
                status_fg: foreground,
                muted: readable(
                    mix(status_bg, foreground, 0.65),
                    foreground,
                    status_bg,
                    foreground,
                ),
                ok: readable(ansi[2], ansi[10], status_bg, foreground),
                warn: readable(ansi[3], ansi[11], status_bg, foreground),
                error: readable(ansi[1], ansi[9], status_bg, foreground),
            }
        }
    };
    TerminalPalette {
        name: spec.name,
        foreground,
        background,
        cursor: rgb(spec.cursor),
        selection: rgb(spec.selection),
        ansi,
        ui,
    }
}

fn find(name: &str) -> Option<&'static Spec> {
    let name = name.to_ascii_lowercase();
    THEMES
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name.as_str()))
}

/// The theme names in cycling order.
pub fn theme_names() -> impl Iterator<Item = &'static str> {
    THEMES.iter().map(|spec| spec.name)
}

/// The names for a usage message, `a, b, c`.
pub fn theme_list() -> String {
    theme_names().collect::<Vec<_>>().join(", ")
}

/// The theme's own name for a name or alias in any case, or `None`.
pub fn canonical_theme_name(name: &str) -> Option<&'static str> {
    find(name).map(|spec| spec.name)
}

pub fn palette_by_name(name: &str) -> Option<TerminalPalette> {
    find(name).map(build)
}

/// The default look.
pub fn default_palette() -> TerminalPalette {
    build(&THEMES[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_are_unique() {
        let names: Vec<_> = theme_names().collect();
        for name in &names {
            assert_eq!(palette_by_name(name).unwrap().name, *name);
            assert_eq!(
                canonical_theme_name(&name.to_ascii_uppercase()),
                Some(*name)
            );
            assert_eq!(names.iter().filter(|other| *other == name).count(), 1);
        }
        for spec in THEMES {
            for alias in spec.aliases {
                assert_eq!(canonical_theme_name(alias), Some(spec.name));
                assert!(!names.contains(alias), "{alias}");
            }
        }
    }

    #[test]
    fn the_requested_names_and_aliases_exist() {
        for name in [
            "dark",
            "light",
            "classic-dark",
            "gruvbox-dark",
            "gruvbox-light",
            "tokyo-night",
            "tokyo-night-storm",
            "tokyo-night-light",
            "tokyo-night-day",
            "catppuccin-mocha",
            "catppuccin-latte",
            "dracula",
            "nord",
            "solarized-dark",
            "solarized-light",
            "one-dark",
            "Gruvbox",
        ] {
            assert!(palette_by_name(name).is_some(), "{name}");
        }
        assert_eq!(canonical_theme_name("classic-dark"), Some("dark"));
        assert!(palette_by_name("nope").is_none());
        assert!(palette_by_name("").is_none());
    }

    #[test]
    fn dark_is_the_original_look() {
        let dark = default_palette();
        assert_eq!(dark.name, DEFAULT_THEME);
        assert_eq!(dark.background, rgb(0x000000));
        assert_eq!(dark.foreground, rgb(0xD4D4D4));
        assert_eq!(dark.ui.status_bg, rgb(0x202020));
        assert_eq!(dark.ui.error, rgb(0xFF6060));
    }

    #[test]
    fn every_theme_has_readable_status_colors() {
        for name in theme_names() {
            let ui = palette_by_name(name).unwrap().ui;
            assert_ne!(ui.status_fg, ui.status_bg, "{name}");
            let checks = [
                ("fg", ui.status_fg),
                ("muted", ui.muted),
                ("ok", ui.ok),
                ("warn", ui.warn),
                ("error", ui.error),
            ];
            for (what, color) in checks {
                let ratio = contrast_ratio(color, ui.status_bg);
                assert!(ratio >= 3.0, "{name} {what}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn every_theme_has_readable_default_text_and_a_visible_selection() {
        for name in theme_names() {
            let theme = palette_by_name(name).unwrap();
            let ratio = contrast_ratio(theme.foreground, theme.background);
            assert!(ratio >= 4.0, "{name}: {ratio:.2}");
            assert_ne!(theme.selection, theme.background, "{name}");
            let selected = contrast_ratio(theme.foreground, theme.selection);
            assert!(selected >= 3.0, "{name} selection: {selected:.2}");
        }
    }

    #[test]
    fn light_ansi_colors_are_readable_on_the_light_background() {
        let light = palette_by_name("light").unwrap();
        // Index 7 and 15 are the light greys; every other color is text.
        for (index, color) in light.ansi.iter().enumerate() {
            if index == 7 || index == 15 {
                continue;
            }
            let ratio = contrast_ratio(*color, light.background);
            assert!(ratio >= 3.0, "ansi {index}: {ratio:.2}");
        }
    }
}
