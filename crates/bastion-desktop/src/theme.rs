//! D4's theme: one dark theme, purple as the main color, as iced styles.

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
/// A hovered row: lighter than a panel, darker than a line, so a chip's
/// edge and a section's edge stay visible over it.
pub const RAISED: Color = hex(0x221e34);
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
/// A title in a list: heavier than the body, lighter than bold.
pub const MEDIUM: Font = Font {
    weight: font::Weight::Medium,
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

/// The top bar: the panel color, with a line.
pub fn bar(_: &Theme) -> container::Style {
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

/// A top bar entry or a server in the switcher: purple when chosen, a
/// line on hover.
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
    let fill = match status {
        button::Status::Hovered => PRIMARY_HOVER,
        button::Status::Disabled => Color { a: 0.5, ..PRIMARY },
        _ => PRIMARY,
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
    // A button that cannot be pressed now reads faded, as `disabled:opacity-40` did.
    let text_color = if status == button::Status::Disabled {
        LINE
    } else {
        INK
    };
    button::Style {
        background,
        text_color,
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

/// A section that wants attention: a warning-colored edge.
pub fn accented(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: WARN,
            ..lined(12.0)
        },
        ..panel(theme)
    }
}

/// A 1 px line across a section.
pub fn rule<'a, M: 'a>() -> iced::Element<'a, M> {
    container(iced::widget::space::horizontal())
        .height(1)
        .width(iced::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(LINE)),
            ..container::Style::default()
        })
        .into()
}

/// A whole row that opens something: no fill until hovered.
pub fn row_button(_: &Theme, status: button::Status) -> button::Style {
    let background = matches!(status, button::Status::Hovered | button::Status::Pressed)
        .then_some(Background::Color(RAISED));
    button::Style {
        background,
        text_color: INK,
        ..button::Style::default()
    }
}

/// A diff line picked for a comment: a purple tint that stays.
pub fn picked_row(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color { a: 0.25, ..PRIMARY })),
        ..row_button(theme, status)
    }
}

/// A row in a list card: [`row_button`] with rounded corners, inset from the
/// card's edge so the edge shows while it is hovered.
pub fn card_row(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        border: rounded(8.0),
        ..row_button(theme, status)
    }
}

/// A small text button, muted until hovered, as "← All jobs".
pub fn link(_: &Theme, status: button::Status) -> button::Style {
    let text_color = if status == button::Status::Hovered {
        INK
    } else {
        MUTED
    };
    button::Style {
        text_color,
        ..button::Style::default()
    }
}

/// A pill around a short label, as a job's stage.
pub fn chip(_: &Theme) -> container::Style {
    container::Style {
        text_color: Some(MUTED),
        border: Border {
            radius: 999.0.into(),
            width: 1.0,
            color: LINE,
        },
        ..container::Style::default()
    }
}

/// A section that says something failed: a red edge.
pub fn alarmed(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: BAD,
            ..lined(12.0)
        },
        ..panel(theme)
    }
}

/// Code, a report or a diff: the page color, slightly rounded.
pub fn code(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(BG)),
        border: rounded(4.0),
        ..container::Style::default()
    }
}

pub const MONO: Font = Font::MONOSPACE;
