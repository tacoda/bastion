//! A step's constraints: each a rule, a hook or a reviewer, with its rung as
//! a switch, and the forms that add one.

use bastion_protocol::ConstraintView;
use iced::widget::{column, container, pick_list, row, text, text_input};
use iced::{Element, Fill};

use super::{At, Msg, RuleField, Rules, chip, choice, help, small};
use crate::pages::Choice;
use crate::theme;

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
    // At every step, no step is named: one for every step made here comes
    // off there. A seeded rule is only demoted, never removed by a click.
    let made_in_app = ["rule-", "hook-", "reviewer-"]
        .iter()
        .any(|p| c.id.starts_with(p));
    let every_here = at.step.is_empty() && c.steps.is_empty() && made_in_app;
    let msg = |m: Msg| editable.then_some(m);
    // One the owner made for this step alone comes off whole; "Not here"
    // would leave it at no step, which means every step.
    let made_here = c.source == "owner" && (only_here || every_here);
    match (made_here, c.steps.is_empty()) {
        (true, _) => small("Remove", msg(Msg::Remove(c.id.clone()))),
        (false, true) => chip("every step"),
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

/// A constraint's rung: a switch to move it, or its badge when it may not
/// be changed here.
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

fn line<'a>(c: &ConstraintView, at: &At, editable: bool) -> Element<'a, Msg> {
    let rule = text(c.rule.clone()).size(13).width(Fill);
    row![
        badge(kind(c), theme::MUTED),
        rule,
        rung_switch(c, editable),
        scope(c, at, editable)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// The form for a rule at the step: what the agent is told, its rung, and
/// the calls it binds once it is more than told.
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

/// The form for a hook at the step: the calls it runs before, and its
/// command, which runs in the job's VM.
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
    let why = "A command that runs in the job's VM before each matching call here. Exit 2 refuses the call, and its output says why.";
    column![help(why), fields].spacing(6).into()
}

/// The form for a reviewer at the step: an agent in `.agents/agents/`.
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
    let why = "An agent in .agents/agents/ reads this step's change when it finishes. If it fails the change, the step fails.";
    column![help(why), fields].spacing(6).into()
}

/// A picker for a constraint scoped to other steps, to apply here too.
pub(super) fn elsewhere_form<'a>(rules: &Rules, at: &At) -> Element<'a, Msg> {
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
        assert!(fits(&hook, ""));
        assert!(!fits(&hook, "reviewer"));
        assert_eq!(kind(&ConstraintView::default()), "Rule");
        assert_eq!(layer(&hook), "harness", "a hook binds calls");
        let reviewer = ConstraintView {
            reviewer: "security".into(),
            ..ConstraintView::default()
        };
        assert_eq!(layer(&reviewer), "factory", "a reviewer reads the work");
    }
}
