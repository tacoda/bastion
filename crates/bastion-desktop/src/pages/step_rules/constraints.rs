//! Constraints at a level: each a rule, a hook or a reviewer, with its rung
//! as a switch, its setting at the level, and the forms that add one.

use bastion_protocol::ConstraintView;
use iced::widget::{column, container, pick_list, row, text, text_input};
use iced::{Element, Fill};

use super::{At, Item, Msg, RuleField, Rules, choice, help, scope, scope_switch, small};
use crate::theme;

/// What kind of constraint `c` is: a rule on calls, a hook or a reviewer.
pub(super) fn kind(c: &ConstraintView) -> &'static str {
    match (c.hook.is_empty(), c.reviewer.is_empty()) {
        (false, _) => "Hook",
        (_, false) => "Reviewer",
        _ => "Rule",
    }
}

/// The layer a constraint belongs to (D25): the charter's, the harness's
/// on the agent's calls, or the factory's on the work, as a review check or
/// a reviewer is.
pub(super) fn layer(c: &ConstraintView) -> &'static str {
    match (c.layer.as_str(), c.reviewer.is_empty()) {
        ("charter", _) => "charter",
        ("factory", _) | (_, false) => "factory",
        _ => "harness",
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

/// A constraint's rung: a switch to move it, or its badge when it may not
/// be changed here. The rung is the item's, the same at every level.
fn rung_switch<'a>(c: &ConstraintView, editable: bool) -> Element<'a, Msg> {
    if !editable || c.source == "charter" {
        return badge(&c.rung, rung(c).1);
    }
    let set = |label: &str, to: &str| {
        let msg = (c.rung != to).then(|| Msg::SetRung(c.id.clone(), to.into()));
        choice(label.to_string(), c.rung == to, msg)
    };
    row![
        set("Told", "told"),
        set("Checked", "checked"),
        set("Enforced", "enforced")
    ]
    .spacing(2)
    .into()
}

/// One made in the app comes off whole, at every workflow; a seeded one is
/// only turned off or moved down its rungs.
fn removable(c: &ConstraintView, at: &At) -> bool {
    let made_in_app = ["rule-", "hook-", "reviewer-"]
        .iter()
        .any(|p| c.id.starts_with(p));
    at.is_every() && made_in_app
}

fn line<'a>(c: &ConstraintView, at: &At, editable: bool) -> Element<'a, Msg> {
    let on = scope::on(&c.steps, at.scope);
    let color = if on { theme::INK } else { theme::MUTED };
    let rule = text(c.rule.clone()).size(13).color(color).width(Fill);
    let item = Item::Constraint(c.id.clone());
    let mut line = row![
        badge(kind(c), theme::MUTED),
        rule,
        rung_switch(c, editable),
        scope_switch(item, &c.steps, at, editable && c.source != "charter"),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if editable && removable(c, at) {
        line = line.push(small("Remove", Some(Msg::Remove(c.id.clone()))));
    }
    line.into()
}

/// The form for a rule: what the agent is told, its rung, and the calls it
/// binds once it is more than told.
pub(super) fn rule_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let d = &rules.rule;
    let named = !d.text.trim().is_empty();
    let field = |hint: &'static str, value: &str, f: RuleField| {
        text_input(hint, value)
            .on_input(move |v| Msg::Rule(f, v))
            .style(theme::field)
    };
    let rungs = vec!["told".to_string(), "checked".into(), "enforced".into()];
    let rung = Some(d.rung.clone())
        .filter(|r| !r.is_empty())
        .or(Some("told".into()));
    let rung = pick_list(rungs, rung, |r| Msg::Rule(RuleField::Rung, r))
        .text_size(13)
        .style(theme::select);
    let text = field(
        "What the agent is told, such as: Keep each commit under 200 lines.",
        &d.text,
        RuleField::Text,
    )
    .on_submit_maybe(named.then_some(Msg::AddRule));
    let binds = row![
        field(
            "Calls, such as sandbox::exec",
            &d.function,
            RuleField::Function
        )
        .width(260),
        field(
            "Words a call holds, such as git push",
            &d.contains,
            RuleField::Contains
        ),
        rung,
        small("Add rule", named.then_some(Msg::AddRule)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let why = "Told is only said to the agent. Checked notes a call that breaks it, and enforced refuses one: both need the calls it binds.";
    column![help(why), text, binds].spacing(6).into()
}

/// The form for a hook: the calls it runs before, and its command, which
/// runs in the job's VM.
pub(super) fn hook_form<'a>(rules: &Rules) -> Element<'a, Msg> {
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
    let why = "A command that runs in the job's VM before each matching call. Exit 2 refuses the call, and its output says why.";
    column![help(why), fields].spacing(6).into()
}

/// The form for a reviewer: an agent in `.agents/agents/`.
pub(super) fn reviewer_form<'a>(rules: &Rules) -> Element<'a, Msg> {
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
    let why = "An agent in .agents/agents/ reads a step's change when it finishes. If it fails the change, the step fails.";
    column![help(why), fields].spacing(6).into()
}

/// The rows of `list`, enforced first, narrowed to the filter.
pub(super) fn rows<'a>(
    rules: &Rules,
    list: Vec<&ConstraintView>,
    at: &At,
    editable: bool,
) -> Vec<Element<'a, Msg>> {
    let mut shown: Vec<&ConstraintView> = list
        .into_iter()
        .filter(|c| fits(c, &rules.filter))
        .collect();
    shown.sort_by_key(|c| rung(c).0);
    shown.into_iter().map(|c| line(c, at, editable)).collect()
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
        assert!(!fits(&hook, "reviewer"));
        assert_eq!(layer(&hook), "harness", "a hook binds calls");
        let reviewer = ConstraintView {
            reviewer: "security".into(),
            ..ConstraintView::default()
        };
        assert_eq!(layer(&reviewer), "factory", "a reviewer reads the work");
    }
}
