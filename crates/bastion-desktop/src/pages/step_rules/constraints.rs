//! The step's Constraints tab: what applies at the step, each marked as a
//! rule, a hook or a reviewer, and one form at a time to add one.

use bastion_protocol::ConstraintView;
use iced::widget::{Column, column, container, pick_list, row, text, text_input};
use iced::{Element, Fill};

use super::{Adding, At, Msg, Rules, chip, choice, help, small};
use crate::pages::Choice;
use crate::theme;

/// The list narrows to a filter once it is longer than this.
const FILTER_FROM: usize = 5;

/// What kind of constraint `c` is: a rule on calls, a hook or a reviewer.
pub(super) fn kind(c: &ConstraintView) -> &'static str {
    match (c.hook.is_empty(), c.reviewer.is_empty()) {
        (false, _) => "Hook",
        (_, false) => "Reviewer",
        _ => "Rule",
    }
}

/// `c` holds every word of `filter`, in its rule, id, kind or rung.
pub(super) fn fits(c: &ConstraintView, filter: &str) -> bool {
    let all = format!(
        "{} {} {} {} {} {}",
        c.rule,
        c.id,
        kind(c),
        c.rung,
        c.hook,
        c.reviewer
    );
    let all = all.to_lowercase();
    filter
        .to_lowercase()
        .split_whitespace()
        .all(|w| all.contains(w))
}

/// Where a constraint applies, and how it comes off the step: a hook or a
/// reviewer made for this step alone is removed, and any other scoped one
/// is "Not here".
fn scope<'a>(c: &ConstraintView, at: &At, editable: bool) -> Element<'a, Msg> {
    let only_here = c.steps.len() == 1 && c.steps[0] == at.step;
    let msg = |m: Msg| editable.then_some(m);
    let made_here = kind(c) != "Rule" && only_here;
    match (c.steps.is_empty(), made_here) {
        (true, _) => chip("every step"),
        (false, true) => small("Remove", msg(Msg::Remove(c.id.clone()))),
        (false, false) => small("Not here", msg(Msg::Scope(c.id.clone(), false))),
    }
}

/// A rung's place in the list, enforced first, and its color.
fn rung(c: &ConstraintView) -> (u8, iced::Color) {
    match c.rung.as_str() {
        "enforced" => (0, theme::BAD),
        "checked" => (1, theme::WARN),
        _ => (2, theme::MUTED),
    }
}

/// A fixed-width pill, so the rules line up.
fn badge<'a>(label: &str, color: iced::Color) -> Element<'a, Msg> {
    container(text(label.to_string()).size(11).color(color))
        .padding([2, 0])
        .width(76)
        .align_x(iced::Center)
        .style(theme::chip)
        .into()
}

fn line<'a>(c: &ConstraintView, at: &At, editable: bool) -> Element<'a, Msg> {
    let rule = text(c.rule.clone()).size(13).width(Fill);
    let rung_badge = badge(&c.rung, rung(c).1);
    row![
        badge(kind(c), theme::MUTED),
        rung_badge,
        rule,
        scope(c, at, editable)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// The form for a hook at the step: the calls it runs before, and its
/// command, which runs in the job's VM.
fn hook_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let named = !rules.hook.trim().is_empty();
    let fields = row![
        text_input("sandbox::exec", &rules.hook_function)
            .on_input(Msg::HookFunction)
            .style(theme::field)
            .width(180),
        text_input("./.agents/hooks/check.sh", &rules.hook)
            .on_input(Msg::Hook)
            .on_submit_maybe(named.then_some(Msg::AddHook))
            .style(theme::field),
        small("Add hook", named.then_some(Msg::AddHook)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let why = "A command that runs in the job's VM before each matching call here. Exit 2 refuses the call, and its output says why.";
    column![help(why), fields].spacing(6).into()
}

/// The form for a reviewer at the step: an agent in `.agents/agents/`.
fn reviewer_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let named = !rules.reviewer.trim().is_empty();
    let fields = row![
        text_input("security", &rules.reviewer)
            .on_input(Msg::Reviewer)
            .on_submit_maybe(named.then_some(Msg::AddReviewer))
            .style(theme::field),
        small("Add reviewer", named.then_some(Msg::AddReviewer)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let why = "An agent in .agents/agents/ reads this step's change when it finishes. If it fails the change, the step fails.";
    column![help(why), fields].spacing(6).into()
}

/// A picker for a constraint scoped to other steps, to apply here too.
fn elsewhere_form<'a>(rules: &Rules, at: &At) -> Element<'a, Msg> {
    let elsewhere: Vec<Choice> = rules
        .constraints
        .iter()
        .filter(|c| !c.steps.is_empty() && !c.steps.iter().any(|s| s == at.step))
        .map(|c| Choice::new(&c.id, &c.rule))
        .collect();
    if elsewhere.is_empty() {
        return help("No constraint applies only at other steps.");
    }
    let pick = pick_list(elsewhere, None::<Choice>, |c| Msg::Scope(c.id, true))
        .placeholder("Pick a constraint that applies at other steps")
        .text_size(13)
        .style(theme::select)
        .width(Fill);
    column![
        help("A constraint already scoped to other steps, applied here too."),
        pick
    ]
    .spacing(6)
    .into()
}

/// One form at a time to add a constraint, behind a switch.
fn adding<'a>(rules: &Rules, at: &At) -> Element<'a, Msg> {
    let pick =
        |label: &str, a: Adding| choice(label.to_string(), rules.adding == a, Some(Msg::Adding(a)));
    let switch = row![
        text("Add").size(13).color(theme::MUTED),
        pick("A hook", Adding::Hook),
        pick("A reviewer", Adding::Reviewer),
        pick("One from another step", Adding::Elsewhere),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let form = match rules.adding {
        Adding::Hook => hook_form(rules),
        Adding::Reviewer => reviewer_form(rules),
        Adding::Elsewhere => elsewhere_form(rules, at),
    };
    container(column![switch, form].spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

pub fn view<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let here = rules.here(at.step);
    let mut body = Column::new().spacing(8).push(help(
        "What this step's agent works under. Enforced refuses a call, checked notes it, and told is only said to the agent.",
    ));
    if here.len() > FILTER_FROM {
        let filter = text_input("Filter by words, kind or rung", &rules.filter)
            .on_input(Msg::Filter)
            .style(theme::field);
        body = body.push(filter);
    }
    let mut shown: Vec<&ConstraintView> = here
        .into_iter()
        .filter(|c| fits(c, &rules.filter))
        .collect();
    shown.sort_by_key(|c| rung(c).0);
    body = body.extend(shown.into_iter().map(|c| line(c, at, editable)));
    if editable {
        body = body.push(adding(rules, at));
    }
    body.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constraint_shows_its_kind_and_a_filter_reads_every_field() {
        let hook = ConstraintView {
            rule: "Lint first.".into(),
            rung: "enforced".into(),
            hook: "./lint.sh".into(),
            ..ConstraintView::default()
        };
        assert_eq!(kind(&hook), "Hook");
        assert!(fits(&hook, "hook ENFORCED"));
        assert!(fits(&hook, ""));
        assert!(!fits(&hook, "reviewer"));
        assert_eq!(kind(&ConstraintView::default()), "Rule");
    }
}
