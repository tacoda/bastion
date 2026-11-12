//! The step's Capabilities tab: the grants that decide its agent's calls,
//! each with a switch between ask and allowed, and one form at a time to
//! add one: a function found by search, a named command, or an MCP server.

use bastion_protocol::{CapabilityView, Registered};
use iced::widget::{Column, column, container, row, text, text_input};
use iced::{Element, Fill};

use super::{At, Granting, Msg, Rules, chip, choice, help, small};
use crate::theme;

/// The most functions a search shows.
const SHOWN: usize = 8;

/// The engine's functions whose id, worker or description holds every word
/// of `search`: those whose id starts with it first, then the shorter ids.
pub(super) fn found<'a>(catalog: &'a [Registered], search: &str) -> Vec<&'a Registered> {
    let search = search.trim().to_lowercase();
    if search.is_empty() {
        return Vec::new();
    }
    let holds = |f: &&Registered| {
        let all = format!("{} {} {}", f.id, f.worker, f.description).to_lowercase();
        search.split_whitespace().all(|w| all.contains(w))
    };
    let mut hits: Vec<&Registered> = catalog.iter().filter(holds).collect();
    // A name that starts with the search, then one that holds it, then a
    // match in the description; shorter names first within each.
    hits.sort_by_key(|f| {
        let id = f.id.to_lowercase();
        (!id.starts_with(&search), !id.contains(&search), f.id.len())
    });
    hits.dedup_by(|a, b| a.id == b.id);
    hits.truncate(SHOWN);
    hits
}

/// What a grant lets the agent call, in words.
pub(super) fn about(g: &str, catalog: &[Registered]) -> String {
    if let Some(f) = catalog.iter().find(|f| f.id == g) {
        return first_sentence(&f.description);
    }
    if let Some(command) = g.strip_prefix("sandbox::exec ") {
        return format!("runs `{}` and nothing else on the line", command.trim());
    }
    let server = g.strip_prefix("mcp::").and_then(|s| s.strip_suffix("::*"));
    match (server, g.strip_suffix('*')) {
        (Some(server), _) => format!("every tool of the MCP server {server}"),
        (None, Some(prefix)) => format!("every function that starts with {prefix}"),
        _ => String::new(),
    }
}

/// A function's description cut to its first sentence, for a list.
pub(super) fn first_sentence(description: &str) -> String {
    let first = description
        .split(". ")
        .next()
        .unwrap_or_default()
        .trim_end_matches('.');
    match first.chars().count() > 110 {
        true => first.chars().take(109).chain(['…']).collect(),
        false => first.to_string(),
    }
}

/// Grant `function` at either rung.
fn grant_buttons<'a>(function: String) -> Element<'a, Msg> {
    let go = |rung: &str| Some(Msg::Grant(function.clone(), rung.into()));
    row![small("Ask", go("ask")), small("Allow", go("allowed"))]
        .spacing(6)
        .into()
}

/// One grant: its function and what it covers, with its switch and its
/// removal when the step's own grant may be changed.
fn line<'a>(g: &CapabilityView, catalog: &[Registered], changeable: bool) -> Element<'a, Msg> {
    let about = text(about(&g.function, catalog))
        .size(12)
        .color(theme::MUTED);
    let what = column![text(g.function.clone()).size(13).font(theme::MONO), about]
        .spacing(2)
        .width(Fill);
    if !changeable {
        return row![what, chip(g.rung.clone())]
            .spacing(8)
            .align_y(iced::Center)
            .into();
    }
    let set = |label: &str, rung: &str| {
        choice(
            label.to_string(),
            g.rung == rung,
            Some(Msg::Rung(g.clone(), rung.into())),
        )
    };
    let remove = small("Remove", Some(Msg::Ungrant(g.clone())));
    row![what, set("Ask", "ask"), set("Allowed", "allowed"), remove]
        .spacing(6)
        .align_y(iced::Center)
        .into()
}

/// The search over the engine's functions, and what it found.
fn function_form<'a>(rules: &'a Rules) -> Element<'a, Msg> {
    let input = text_input(
        "Search the engine's functions: fs, exec, github…",
        &rules.search,
    )
    .on_input(Msg::Search)
    .style(theme::field);
    let hits = found(&rules.catalog, &rules.search);
    let mut body = Column::new().spacing(6).push(input);
    let typed = rules.search.trim();
    if typed.is_empty() {
        let why = "Find a function by part of its name or what it does. A pattern such as sandbox::fs::* covers every function it starts.";
        return body.push(help(why)).into();
    }
    body = body.extend(hits.iter().map(|f| {
        let about = format!("{} · {}", f.worker, first_sentence(&f.description));
        let what = column![text(f.id.clone()).size(13).font(theme::MONO), help(about)]
            .spacing(2)
            .width(Fill);
        row![what, grant_buttons(f.id.clone())]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    }));
    if !hits.iter().any(|f| f.id == typed) {
        let what = text(format!("Grant {typed} as typed")).size(13).width(Fill);
        body = body.push(
            row![what, grant_buttons(typed.into())]
                .spacing(8)
                .align_y(iced::Center),
        );
    }
    body.into()
}

/// A named command: `sandbox::exec` bound to the words a line starts with (D49).
fn command_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let words = rules.command.trim();
    let input = text_input("cargo test", &rules.command)
        .on_input(Msg::Command)
        .style(theme::field);
    let buttons: Element<'a, Msg> = match words.is_empty() {
        true => help("Type a command."),
        false => grant_buttons(format!("sandbox::exec {words}")),
    };
    let why = "The agent may run a command line that starts with these words. A line with ; && | or $( never counts.";
    column![
        help(why),
        row![input, buttons].spacing(8).align_y(iced::Center)
    ]
    .spacing(6)
    .into()
}

/// The server's MCP servers, each one's tools granted at once (D52).
fn mcp_form<'a>(rules: &'a Rules) -> Element<'a, Msg> {
    if rules.mcp.is_empty() {
        return help("No MCP servers yet. Add one under Settings, then grant it here.");
    }
    let rows = rules.mcp.iter().map(|s| {
        let what = column![text(s.id.clone()).size(13), help(s.status.clone())]
            .spacing(2)
            .width(Fill);
        row![what, grant_buttons(format!("mcp::{}::*", s.id))]
            .spacing(8)
            .align_y(iced::Center)
            .into()
    });
    Column::with_children(rows).spacing(6).into()
}

/// One form at a time to add a grant, behind a switch.
fn granting<'a>(rules: &'a Rules) -> Element<'a, Msg> {
    let pick = |label: &str, g: Granting| {
        choice(
            label.to_string(),
            rules.granting == g,
            Some(Msg::Granting(g)),
        )
    };
    let switch = row![
        text("Add").size(13).color(theme::MUTED),
        pick("A function", Granting::Function),
        pick("A command", Granting::Command),
        pick("An MCP server", Granting::Mcp),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let form = match rules.granting {
        Granting::Function => function_form(rules),
        Granting::Command => command_form(rules),
        Granting::Mcp => mcp_form(rules),
    };
    container(column![switch, form].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// Whose grants decide the step's calls, and the way to the other.
fn whose<'a>(mine: bool, at: &At, editable: bool) -> Element<'a, Msg> {
    let (said, action, msg) = match mine {
        true => (
            "This step has its own grants. A call none covers is refused.",
            "Use its kind's instead",
            Msg::Kind,
        ),
        false => (
            "This step uses the grants of every step of its kind. A call none covers is refused.",
            "Give it its own",
            Msg::Own,
        ),
    };
    let said = format!("{said} Kind: {}.", at.stage);
    row![
        text(said).size(12).color(theme::MUTED).width(Fill),
        small(action, editable.then_some(msg))
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

pub fn view<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let mine = !rules.own(at.step).is_empty();
    let rows = rules
        .deciding(at)
        .into_iter()
        .map(|g| line(g, &rules.catalog, mine && editable));
    let mut body = Column::new()
        .spacing(8)
        .push(whose(mine, at, editable))
        .extend(rows);
    if mine && editable {
        body = body.push(granting(rules));
    }
    body.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(id: &str, description: &str) -> Registered {
        Registered {
            id: id.into(),
            namespace: "bastion".into(),
            worker: "w".into(),
            description: description.into(),
        }
    }

    #[test]
    fn a_search_finds_by_name_or_purpose_and_puts_a_name_match_first() {
        let catalog = [
            function("sandbox::fs::read", "Read a file in the VM"),
            function("mcp::docs::search", "Search the docs"),
            function("sandbox::fs::write", "Write a file in the VM"),
        ];
        let ids = |s: &str| {
            found(&catalog, s)
                .iter()
                .map(|f| f.id.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids("fs"), ["sandbox::fs::read", "sandbox::fs::write"]);
        assert_eq!(
            ids("file"),
            ["sandbox::fs::read", "sandbox::fs::write"],
            "by what it does"
        );
        assert_eq!(first_sentence("Run code. Then stop."), "Run code");
        assert_eq!(ids("write file"), ["sandbox::fs::write"]);
        assert_eq!(ids("mcp"), ["mcp::docs::search"]);
        assert!(ids("  ").is_empty());
    }

    #[test]
    fn a_grant_says_what_it_covers() {
        let catalog = [function("sandbox::fs::read", "Read a file in the VM")];
        assert_eq!(
            about("sandbox::fs::read", &catalog),
            "Read a file in the VM"
        );
        assert_eq!(
            about("sandbox::exec cargo test", &[]),
            "runs `cargo test` and nothing else on the line"
        );
        assert_eq!(
            about("mcp::github::*", &[]),
            "every tool of the MCP server github"
        );
        assert_eq!(
            about("sandbox::*", &[]),
            "every function that starts with sandbox::"
        );
    }
}
