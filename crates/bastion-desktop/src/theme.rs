//! The themes (D4): named sets of colors, purple as the main color, as iced
//! styles. The app keeps the one picked in Settings.

use iced::theme::Palette;
use iced::widget::{button, container, pick_list, text_input};
use std::sync::atomic::{AtomicUsize, Ordering};

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

/// One theme's colors (D4): named, so more can be added.
pub struct Colors {
    pub bg: Color,
    pub panel: Color,
    pub ink: Color,
    pub muted: Color,
    pub line: Color,
    /// A hovered row: lighter than a panel, darker than a line, so a
    /// chip's edge and a section's edge stay visible over it.
    pub raised: Color,
    pub primary: Color,
    pub primary_hover: Color,
    pub link: Color,
    pub ok: Color,
    pub bad: Color,
    pub warn: Color,
}

const DARK: Colors = Colors {
    bg: hex(0x0f0d17),
    panel: hex(0x19162a),
    ink: hex(0xece9f5),
    muted: hex(0xa39cb8),
    line: hex(0x2c2742),
    raised: hex(0x221e34),
    primary: hex(0x7c3aed),
    primary_hover: hex(0x6d28d9),
    link: hex(0xc4b5fd),
    ok: hex(0x4ade80),
    bad: hex(0xf87171),
    warn: hex(0xfbbf24),
};

const LIGHT: Colors = Colors {
    bg: hex(0xf6f5fa),
    panel: hex(0xffffff),
    ink: hex(0x1a1626),
    muted: hex(0x5d5670),
    line: hex(0xdfdaec),
    raised: hex(0xefecf7),
    primary: hex(0x6d28d9),
    primary_hover: hex(0x5b21b6),
    link: hex(0x5b21b6),
    ok: hex(0x15803d),
    bad: hex(0xb91c1c),
    warn: hex(0xa16207),
};

/// The old-world chart of the software expedition: parchment and ink, red
/// for what went wrong, forest for what landed.
const CHART: Colors = Colors {
    bg: hex(0xefe4c8),
    panel: hex(0xf6eed8),
    ink: hex(0x3b2a1a),
    muted: hex(0x6b5233),
    line: hex(0xcdbb8a),
    raised: hex(0xe5d6b0),
    primary: hex(0x3b2a1a),
    primary_hover: hex(0x5a4128),
    link: hex(0x7a2418),
    ok: hex(0x3f6b2a),
    bad: hex(0x9b2c1f),
    warn: hex(0x8a5a00),
};

/// The themes, by name; the first is the default.
pub const THEMES: [(&str, Colors); 3] = [
    ("bastion-dark", DARK),
    ("bastion-light", LIGHT),
    ("bastion-chart", CHART),
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// A color read from the theme in use, for a table of colors.
pub type Tint = fn() -> Color;

/// The file this app keeps its theme's name in.
fn file() -> std::path::PathBuf {
    crate::start::config_dir().join("theme")
}

/// Use the theme the app last kept, if any.
pub fn load() {
    if let Ok(kept) = std::fs::read_to_string(file()) {
        choose(kept.trim());
    }
}

/// Use the theme named `name`, and keep it for the next start.
pub fn save(name: &str) -> Result<(), String> {
    choose(name);
    let path = file();
    std::fs::write(&path, name).map_err(|e| format!("{}: {e}", path.display()))
}

/// Use the theme named `name`; an unknown name changes nothing.
fn choose(name: &str) {
    if let Some(i) = THEMES.iter().position(|(n, _)| *n == name) {
        CURRENT.store(i, Ordering::Relaxed);
    }
}

/// The name of the theme in use.
pub fn name() -> &'static str {
    THEMES[CURRENT.load(Ordering::Relaxed)].0
}

fn colors() -> &'static Colors {
    &THEMES[CURRENT.load(Ordering::Relaxed)].1
}

pub fn bg() -> Color {
    colors().bg
}
pub fn panel_color() -> Color {
    colors().panel
}
pub fn ink() -> Color {
    colors().ink
}
pub fn muted() -> Color {
    colors().muted
}
pub fn line() -> Color {
    colors().line
}
pub fn raised() -> Color {
    colors().raised
}
pub fn primary_hover() -> Color {
    colors().primary_hover
}
pub fn primary_color() -> Color {
    colors().primary
}
pub fn link_color() -> Color {
    colors().link
}
pub fn ok() -> Color {
    colors().ok
}
pub fn bad() -> Color {
    colors().bad
}
pub fn warn() -> Color {
    colors().warn
}

/// The app's theme, so a widget with no style of its own still reads
/// light on dark.
pub fn bastion() -> Theme {
    let palette = Palette {
        background: bg(),
        text: ink(),
        primary: primary_color(),
        success: ok(),
        warning: warn(),
        danger: bad(),
    };
    Theme::custom(name().to_string(), palette)
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
        color: line(),
    }
}

/// The window behind every page.
pub fn page(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg())),
        text_color: Some(ink()),
        ..container::Style::default()
    }
}

/// The top bar: the panel color, with a line.
pub fn bar(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(panel_color())),
        border: Border {
            width: 1.0,
            color: line(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// A section or a row: the panel color in a rounded, lined box.
pub fn panel(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(panel_color())),
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
            (true, _) => Some(Background::Color(primary_color())),
            (false, button::Status::Hovered | button::Status::Pressed) => {
                Some(Background::Color(line()))
            }
            _ => None,
        };
        let text_color = if here { Color::WHITE } else { ink() };
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
        button::Status::Hovered => primary_hover(),
        button::Status::Disabled => Color {
            a: 0.5,
            ..primary_color()
        },
        _ => primary_color(),
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
    let background = (status == button::Status::Hovered).then_some(Background::Color(line()));
    // A button that cannot be pressed now reads faded, as `disabled:opacity-40` did.
    let text_color = if status == button::Status::Disabled {
        line()
    } else {
        ink()
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
    let background = (status == button::Status::Hovered).then_some(Background::Color(line()));
    button::Style {
        background,
        text_color: muted(),
        border: rounded(8.0),
        ..button::Style::default()
    }
}

pub fn select(_: &Theme, _: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: ink(),
        placeholder_color: muted(),
        handle_color: muted(),
        background: Background::Color(bg()),
        border: lined(4.0),
    }
}

pub fn field(_: &Theme, status: text_input::Status) -> text_input::Style {
    let edge = if matches!(status, text_input::Status::Focused { .. }) {
        primary_color()
    } else {
        line()
    };
    text_input::Style {
        background: Background::Color(bg()),
        border: Border {
            color: edge,
            ..lined(4.0)
        },
        icon: muted(),
        placeholder: muted(),
        value: ink(),
        selection: primary_color(),
    }
}

/// A section that wants attention: a warning-colored edge.
pub fn accented(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: warn(),
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
            background: Some(Background::Color(line())),
            ..container::Style::default()
        })
        .into()
}

/// A whole row that opens something: no fill until hovered.
pub fn row_button(_: &Theme, status: button::Status) -> button::Style {
    let background = matches!(status, button::Status::Hovered | button::Status::Pressed)
        .then_some(Background::Color(raised()));
    button::Style {
        background,
        text_color: ink(),
        ..button::Style::default()
    }
}

/// A diff line picked for a comment: a purple tint that stays.
pub fn picked_row(theme: &Theme, status: button::Status) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color {
            a: 0.25,
            ..primary_color()
        })),
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
        ink()
    } else {
        muted()
    };
    button::Style {
        text_color,
        ..button::Style::default()
    }
}

/// A pill around a short label, as a job's stage.
pub fn chip(_: &Theme) -> container::Style {
    container::Style {
        text_color: Some(muted()),
        border: Border {
            radius: 999.0.into(),
            width: 1.0,
            color: line(),
        },
        ..container::Style::default()
    }
}

/// A section that says something failed: a red edge.
pub fn alarmed(theme: &Theme) -> container::Style {
    container::Style {
        border: Border {
            color: bad(),
            ..lined(12.0)
        },
        ..panel(theme)
    }
}

/// Code, a report or a diff: the page color, slightly rounded.
pub fn code(_: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg())),
        border: rounded(4.0),
        ..container::Style::default()
    }
}

pub const MONO: Font = Font::MONOSPACE;
