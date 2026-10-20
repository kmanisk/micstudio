//! Color palettes and theme system for MicStudio TUI.
//! Inspired by NvChad, Catppuccin, Tokyo Night, and Nord.

use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    NvChad,
    Catppuccin,
    TokyoNight,
    Nord,
    Terminal,
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::NvChad
    }
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 5] = [
        ThemeChoice::NvChad,
        ThemeChoice::Catppuccin,
        ThemeChoice::TokyoNight,
        ThemeChoice::Nord,
        ThemeChoice::Terminal,
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Self::NvChad => "NvChad (OneDark)",
            Self::Catppuccin => "Catppuccin Mocha",
            Self::TokyoNight => "Tokyo Night",
            Self::Nord => "Nord Dark",
            Self::Terminal => "Adaptive ANSI",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            Self::NvChad => "NVCHAD",
            Self::Catppuccin => "CATPPUCCIN",
            Self::TokyoNight => "TOKYO NIGHT",
            Self::Nord => "NORD",
            Self::Terminal => "TERMINAL",
        }
    }

    pub fn from_str_lenient(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "catppuccin" | "mocha" => Self::Catppuccin,
            "tokyonight" | "tokyo" | "tokyo-night" => Self::TokyoNight,
            "nord" => Self::Nord,
            "terminal" | "ansi" => Self::Terminal,
            _ => Self::NvChad,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NvChad => "nvchad",
            Self::Catppuccin => "catppuccin",
            Self::TokyoNight => "tokyonight",
            Self::Nord => "nord",
            Self::Terminal => "terminal",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::NvChad => Self::Catppuccin,
            Self::Catppuccin => Self::TokyoNight,
            Self::TokyoNight => Self::Nord,
            Self::Nord => Self::Terminal,
            Self::Terminal => Self::NvChad,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::NvChad => Self::Terminal,
            Self::Catppuccin => Self::NvChad,
            Self::TokyoNight => Self::Catppuccin,
            Self::Nord => Self::TokyoNight,
            Self::Terminal => Self::Nord,
        }
    }

    pub fn palette(&self) -> ColorPalette {
        match self {
            Self::NvChad => ColorPalette {
                border: Color::Rgb(62, 68, 81),           // #3e4451
                border_active: Color::Rgb(86, 182, 194),   // #56b6c2
                text: Color::Rgb(171, 178, 191),          // #abb2bf
                text_muted: Color::Rgb(92, 99, 112),      // #5c6370
                accent_primary: Color::Rgb(97, 175, 239),  // #61afef (Blue)
                accent_secondary: Color::Rgb(152, 195, 121), // #98c379 (Green)
                accent_warning: Color::Rgb(229, 192, 123), // #e5c07b (Yellow)
                accent_danger: Color::Rgb(224, 108, 117),  // #e06c75 (Coral Red)
                accent_purple: Color::Rgb(198, 120, 221),  // #c678dd (Violet)
                select_bg: Color::Rgb(40, 44, 52),         // #282c34
                select_fg: Color::Rgb(97, 175, 239),       // #61afef
                gauge_fill: Color::Rgb(86, 182, 194),      // #56b6c2
                gauge_empty: Color::Rgb(62, 68, 81),       // #3e4451
                badge_bg: Color::Rgb(86, 182, 194),        // #56b6c2
                badge_fg: Color::Rgb(24, 27, 32),          // #181b20
            },
            Self::Catppuccin => ColorPalette {
                border: Color::Rgb(69, 71, 90),           // #45475a
                border_active: Color::Rgb(137, 180, 250),  // #89b4fa
                text: Color::Rgb(205, 214, 244),          // #cdd6f4
                text_muted: Color::Rgb(108, 112, 134),    // #6c7086
                accent_primary: Color::Rgb(137, 180, 250), // #89b4fa
                accent_secondary: Color::Rgb(166, 227, 161), // #a6e3a1
                accent_warning: Color::Rgb(249, 226, 175), // #f9e2af
                accent_danger: Color::Rgb(243, 139, 168),  // #f38ba8
                accent_purple: Color::Rgb(203, 166, 247),  // #cba6f7
                select_bg: Color::Rgb(49, 50, 68),         // #313244
                select_fg: Color::Rgb(137, 180, 250),
                gauge_fill: Color::Rgb(148, 226, 213),     // #94e2d5
                gauge_empty: Color::Rgb(69, 71, 90),
                badge_bg: Color::Rgb(137, 180, 250),
                badge_fg: Color::Rgb(17, 17, 27),
            },
            Self::TokyoNight => ColorPalette {
                border: Color::Rgb(59, 66, 97),           // #3b4261
                border_active: Color::Rgb(122, 162, 247),  // #7aa2f7
                text: Color::Rgb(192, 202, 245),          // #c0caf5
                text_muted: Color::Rgb(86, 95, 137),      // #565f89
                accent_primary: Color::Rgb(122, 162, 247), // #7aa2f7
                accent_secondary: Color::Rgb(158, 206, 106), // #9ece6a
                accent_warning: Color::Rgb(224, 175, 104), // #e0af68
                accent_danger: Color::Rgb(247, 118, 142),  // #f7768e
                accent_purple: Color::Rgb(187, 154, 247),  // #bb9af7
                select_bg: Color::Rgb(41, 46, 66),         // #292e42
                select_fg: Color::Rgb(125, 207, 255),      // #7dcfff
                gauge_fill: Color::Rgb(125, 207, 255),
                gauge_empty: Color::Rgb(59, 66, 97),
                badge_bg: Color::Rgb(122, 162, 247),
                badge_fg: Color::Rgb(26, 27, 38),
            },
            Self::Nord => ColorPalette {
                border: Color::Rgb(76, 86, 106),           // #4c566a
                border_active: Color::Rgb(136, 192, 208),  // #88c0d0
                text: Color::Rgb(236, 239, 244),          // #eceff4
                text_muted: Color::Rgb(94, 129, 172),     // #5e81ac
                accent_primary: Color::Rgb(136, 192, 208), // #88c0d0
                accent_secondary: Color::Rgb(163, 190, 140), // #a3be8c
                accent_warning: Color::Rgb(235, 203, 139), // #ebcb8b
                accent_danger: Color::Rgb(191, 97, 106),   // #bf616a
                accent_purple: Color::Rgb(180, 142, 173),  // #b48ead
                select_bg: Color::Rgb(59, 66, 82),         // #3b4252
                select_fg: Color::Rgb(143, 188, 187),      // #8fbcbb
                gauge_fill: Color::Rgb(136, 192, 208),
                gauge_empty: Color::Rgb(76, 86, 106),
                badge_bg: Color::Rgb(136, 192, 208),
                badge_fg: Color::Rgb(46, 52, 64),
            },
            Self::Terminal => ColorPalette {
                border: Color::DarkGray,
                border_active: Color::Cyan,
                text: Color::White,
                text_muted: Color::DarkGray,
                accent_primary: Color::Cyan,
                accent_secondary: Color::Green,
                accent_warning: Color::Yellow,
                accent_danger: Color::Red,
                accent_purple: Color::Magenta,
                select_bg: Color::DarkGray,
                select_fg: Color::Cyan,
                gauge_fill: Color::Cyan,
                gauge_empty: Color::DarkGray,
                badge_bg: Color::Cyan,
                badge_fg: Color::Black,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    pub border: Color,
    pub border_active: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent_primary: Color,
    pub accent_secondary: Color,
    pub accent_warning: Color,
    pub accent_danger: Color,
    pub accent_purple: Color,
    pub select_bg: Color,
    pub select_fg: Color,
    pub gauge_fill: Color,
    pub gauge_empty: Color,
    pub badge_bg: Color,
    pub badge_fg: Color,
}
