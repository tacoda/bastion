//! D4's theme: one dark theme, purple as the main color. The same tokens as
//! the Leptos pages' `styles.css`, as iced styles.

use iced::theme::Palette;
use iced::widget::{button, container, pick_list, text_input};
use iced::{Background, Border, Color, Font, Theme, font};

const fn byte(rgb: u32, shift: u32) -> f32 {
    ((rgb >> shift) & 0xff) as f32 / 255.0
}

const fn hex(rgb: u32) -> Color {
    Color {
        r: byte(rgb, 16),
        g: byte(rgb, 8),
        b: byte(rgb, 0),
        a: 1.0,
    }
}

pub const BG: Color = hex(0x0f0d17);
pub const PANEL: Color = hex(0x19162a);
pub const INK: Color = hex(0xece9f5);
pub const MUTED: Color = hex(0xa39cb8);
pub const LINE: Color = hex(0x2c2742);
pub const PRIMARY: Color = hex(0x7c3aed);
pub const PRIMARY_HOVER: Color = hex(0x6d28d9);
pub const LINK: Color = hex(0xc4b5fd);
pub const OK: Color = hex(0x4ade80);
pub const BAD: Color = hex(0xf87171);
pub const WARN: Color = hex(0xfbbf24);

/// The app's theme, so a widget with no style of its own still reads
/// light on dark.
pub fn bastion() -> Theme {
    let palette = Palette {
        background: BG,
        text: INK,
        primary: PRIMARY,
        success: OK,
        warning: WARN,
        danger: BAD,
    };
    Theme::custom("Bastion".to_string(), palette)
}

/// The text face. macOS's system face is variable, and iced draws no bold
/// from it, so the client uses a face with a real bold.
// ponytail: Helvetica Neue on macOS; other systems fall back to their own sans.
pub const FONT: Font = Font::with_name("Helvetica Neue");
pub const BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..FONT
};

fn rounded(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

fn lined(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        width: 1.0,
        color: LINE,
    }
}

/// The window behind every page.
pub fn page(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG)),
        text_color: Some(INK),
        ..container::Style::default()
    }
}

/// The sidebar: the panel color, a line on its right.
pub fn sidebar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: Border {
            width: 1.0,
            color: LINE,
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A section or a row: the panel color in a rounded, lined box.
pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(PANEL)),
        border: lined(12.0),
        ..container::Style::default()
    }
}

/// The dark veil behind a dialog.
pub fn veil(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color {
            a: 0.6,
            ..Color::BLACK
        })),
        ..container::Style::default()
    }
}

/// A sidebar entry: purple when its page is shown, a line on hover.
pub fn nav(here: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let background = match (here, status) {
            (true, _) => Some(Background::Color(PRIMARY)),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(Background::Color(LINE))
            }
            _ => None,
        };
        let text_color = if here { Color::WHITE } else { INK };
        button::Style {
            background,
            text_color,
            border: rounded(8.0),
            ..button::Style::default()
        }
    }
}

/// The main action: white on purple.
pub fn primary(_: &Theme, status: button::Status) -> button::Style {
    let fill = if status == button::Status::Hovered {
        PRIMARY_HOVER
    } else {
        PRIMARY
    };
    button::Style {
        background: Some(Background::Color(fill)),
        text_color: Color::WHITE,
        border: rounded(8.0),
        ..button::Style::default()
    }
}

/// Any other button: a lined box that fills on hover.
pub fn secondary(_: &Theme, status: button::Status) -> button::Style {
    let background = (status == button::Status::Hovered).then_some(Background::Color(LINE));
    button::Style {
        background,
        text_color: INK,
        border: lined(8.0),
        ..button::Style::default()
    }
}

/// A plain button with muted text, as Cancel.
pub fn quiet(_: &Theme, status: button::Status) -> button::Style {
    let background = (status == button::Status::Hovered).then_some(Background::Color(LINE));
    button::Style {
        background,
        text_color: MUTED,
        border: rounded(8.0),
        ..button::Style::default()
    }
}

pub fn select(_: &Theme, _: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: INK,
        placeholder_color: MUTED,
        handle_color: MUTED,
        background: Background::Color(BG),
        border: lined(4.0),
    }
}

pub fn field(_: &Theme, status: text_input::Status) -> text_input::Style {
    let edge = if matches!(status, text_input::Status::Focused { .. }) {
        PRIMARY
    } else {
        LINE
    };
    text_input::Style {
        background: Background::Color(BG),
        border: Border {
            color: edge,
            ..lined(4.0)
        },
        icon: MUTED,
        placeholder: MUTED,
        value: INK,
        selection: PRIMARY,
    }
}
