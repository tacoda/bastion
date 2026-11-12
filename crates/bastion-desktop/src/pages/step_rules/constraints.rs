//! Constraints at a level: each a rule, a hook or a reviewer, with its rung
//! as a switch, its setting at the level, and the forms that add one.

use bastion_protocol::ConstraintView;
use iced::widget::{column, container, row, text, text_input};
use iced::{Element, Fill};

use super::{
    At, FormDraft, FormField, Item, Msg, RuleField, Rules, choice, help, scope, scope_switch, small,
};
use crate::theme;

/// What kind of constraint `c` is: a rule on calls, a hook or a reviewer.
pub(super) fn kind(c: &ConstraintView) -> &'static str {
    match (c.hook.is_empty(), c.reviewer.is_empty(), c.capability) {
        (_, _, true) => "Note",
        (false, _, _) => "Hook",
        (_, false, _) => "Reviewer",
        // A told rule keeps its calls when demoted, so its mode names it.
        _ if c.function.is_empty() || c.rung == "told" => "Rule",
        _ => "Check",
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

/// A constraint's mode (D56): feedback checks what the agent did, as a
/// word match, a hook, a review check or a reviewer does; feedforward
/// guides it before it acts, as a told rule or a note does.
pub(super) fn mode(c: &ConstraintView) -> &'static str {
    let feedback = !c.reviewer.is_empty() || (c.rung != "told" && has_form(c));
    match (feedback, c.capability) {
        (true, false) => "Feedback",
        _ => "Feedforward",
    }
}

/// Where a constraint comes from: the workspace's charter, or Bastion, at
/// the harness or the factory.
pub(super) fn source(c: &ConstraintView) -> String {
    match layer(c) {
        "charter" => "Workspace".into(),
        l => format!("Bastion · {l}"),
    }
}

/// Whether code has something to judge the rule by: calls to match, or a
/// hook to run.
fn has_form(c: &ConstraintView) -> bool {
    !c.function.is_empty() || !c.hook.is_empty()
}

/// A rung's place in the list, enforced first, and its color.
fn rung(c: &ConstraintView) -> (u8, iced::Color) {
    match c.rung.as_str() {
        "enforced" => (0, theme::bad()),
        "checked" => (1, theme::warn()),
        _ => (2, theme::muted()),
    }
}

/// A fixed-width pill, so the rules line up.
pub(super) fn badge<'a>(label: &str, color: iced::Color) -> Element<'a, Msg> {
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
    let fixed = !editable || c.source == "charter" || !c.reviewer.is_empty() || c.capability;
    if fixed {
        return badge(mode(c), theme::muted());
    }
    // Feedforward with nothing for code to judge asks for its form first.
    let needs_form = !has_form(c);
    let set = |label: &str, to: &str| {
        let msg = match (c.rung == to, needs_form && to == "enforced") {
            (true, _) => None,
            (false, true) => Some(Msg::Form(c.id.clone(), Some(to.into()))),
            (false, false) => Some(Msg::SetRung(c.id.clone(), to.into())),
        };
        choice(label.to_string(), c.rung == to, msg)
    };
    row![set("Feedforward", "told"), set("Feedback", "enforced")]
        .spacing(2)
        .into()
}

/// One made in the app for this workflow comes off whole at the workflow's
/// level; a seeded one, or one other workflows use, is only turned off.
fn removable(c: &ConstraintView, at: &At) -> bool {
    let prefixes = ["rule-", "hook-", "reviewer-", "note-"];
    let made_in_app = prefixes.iter().any(|p| c.id.starts_with(p));
    !at.is_step() && made_in_app && scope::only_in(&c.steps, at.scope)
}

/// The editor for a rule's deterministic form: the calls and words code
/// matches, or a hook it runs.
fn form_editor<'a>(d: &FormDraft) -> Element<'a, Msg> {
    let field = |hint: &'static str, value: &str, f: FormField| {
        text_input(hint, value)
            .on_input(move |v| Msg::FormField(f, v))
            .style(theme::field)
    };
    let matches = row![
        field(
            "Calls, such as sandbox::exec",
            &d.function,
            FormField::Function
        )
        .width(260),
        field(
            "Words a call holds, such as git push",
            &d.contains,
            FormField::Contains
        ),
    ]
    .spacing(8);
    let hook = field(
        "Or a hook command, such as ./.agents/hooks/lint.sh",
        &d.hook,
        FormField::Hook,
    );
    let ready = !d.function.trim().is_empty() || !d.hook.trim().is_empty();
    let save = match &d.rung {
        Some(r) => format!("Save and make it {r}"),
        None => "Save".into(),
    };
    let buttons = row![
        small(save, ready.then_some(Msg::SaveForm)),
        small("Cancel", Some(Msg::CancelForm))
    ]
    .spacing(6);
    let why = "Code judges this rule by the calls it matches, or by a hook that exits 2 to refuse. Told keeps this and stops running it.";
    container(column![help(why), matches, hook, buttons].spacing(6))
        .padding(10)
        .width(Fill)
        .style(theme::panel)
        .into()
}

/// A row's buttons: Form for a rule the owner keeps, and Remove where it
/// may come off whole.
fn actions<'a>(c: &ConstraintView, at: &At, editable: bool) -> Vec<Element<'a, Msg>> {
    let formed = editable && c.source != "charter" && c.reviewer.is_empty() && !c.capability;
    let form = formed.then(|| small("Form", Some(Msg::Form(c.id.clone(), None))));
    let remove =
        (editable && removable(c, at)).then(|| small("Remove", Some(Msg::Remove(c.id.clone()))));
    form.into_iter().chain(remove).collect()
}

fn line<'a>(rules: &Rules, c: &ConstraintView, at: &At, editable: bool) -> Element<'a, Msg> {
    let on = scope::on(&c.steps, at.scope);
    let color = if on { theme::ink() } else { theme::muted() };
    let rule = column![text(c.rule.clone()).size(13).color(color), help(source(c)),]
        .spacing(2)
        .width(Fill);
    let item = Item::Constraint(c.id.clone());
    let mut line = row![
        badge(kind(c), theme::muted()),
        rule,
        rung_switch(c, editable),
        scope_switch(item, &c.steps, at, editable && c.source != "charter"),
    ]
    .spacing(8)
    .align_y(iced::Center);
    line = line.extend(actions(c, at, editable));
    match rules.forming.as_ref().filter(|d| d.id == c.id) {
        Some(d) => column![line, form_editor(d)].spacing(6).into(),
        None => line.into(),
    }
}

/// The form for a rule: what the agent is told, its rung, and the calls it
/// binds once it is more than told.
pub(super) fn rule_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let form = TextForm {
        hint: "What the agent is told, such as: Keep each commit under 200 lines.",
        label: "Add rule",
        why: "Feedforward: said to the agent before it acts, and the agent judges it.",
    };
    text_form(rules, form, Msg::AddRule)
}

/// The form for a note on what the agent may do, a feedforward capability.
pub(super) fn note_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let form = TextForm {
        hint: "What the agent may do, such as: Use the code-review skill before you commit.",
        label: "Add note",
        why: "Feedforward: told to the agent as what it may do, such as a skill or an agent to use.",
    };
    text_form(rules, form, Msg::AddNote)
}

/// The words of a one-line form: its hint, its button, and what it adds.
struct TextForm {
    hint: &'static str,
    label: &'static str,
    why: &'static str,
}

/// A form of one line of text and its button.
fn text_form<'a>(rules: &Rules, form: TextForm, add: Msg) -> Element<'a, Msg> {
    let named = !rules.rule.text.trim().is_empty();
    let text = text_input(form.hint, &rules.rule.text)
        .on_input(|v| Msg::Rule(RuleField::Text, v))
        .on_submit_maybe(named.then(|| add.clone()))
        .style(theme::field);
    let row = row![text, small(form.label, named.then_some(add))]
        .spacing(8)
        .align_y(iced::Center);
    column![help(form.why), row].spacing(6).into()
}

/// The form for a check: what the agent is told, and the calls and words
/// the gate refuses.
pub(super) fn check_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let d = &rules.rule;
    let ready = !d.text.trim().is_empty() && !d.function.trim().is_empty();
    let field = |hint: &'static str, value: &str, f: RuleField| {
        text_input(hint, value)
            .on_input(move |v| Msg::Rule(f, v))
            .style(theme::field)
    };
    let text = field(
        "What the agent is told, such as: Do not push to main.",
        &d.text,
        RuleField::Text,
    );
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
        small("Add check", ready.then_some(Msg::AddCheck)),
    ]
    .spacing(8)
    .align_y(iced::Center);
    let why = "Feedback: the gate refuses a call to these calls that holds these words.";
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
    shown
        .into_iter()
        .map(|c| line(rules, c, at, editable))
        .collect()
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
        assert_eq!(mode(&hook), "Feedback", "an enforced hook");
        assert_eq!(
            mode(&reviewer),
            "Feedback",
            "a reviewer reads what was done"
        );
        let told = ConstraintView {
            rung: "told".into(),
            ..hook.clone()
        };
        assert_eq!(mode(&told), "Feedforward", "a told rule, form or not");
        assert!(has_form(&told), "demoted, it keeps its hook");
    }
}
