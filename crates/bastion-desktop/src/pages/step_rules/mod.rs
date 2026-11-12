//! The rules at one workflow step, on the workflows page (D44, D48): the
//! constraints that apply there, and the grants that decide what its
//! agent's calls may do. A step uses its kind's grants until it has its own,
//! which then take their place. Each is a tab of the step's panel.

mod capabilities;
mod constraints;
mod layers;

use bastion_protocol::api::{self, Request};
use bastion_protocol::{
    CapabilityView, Console, ConstraintView, McpServerView, Registered, WorkspaceCharterView,
};
use iced::widget::{button, text};
use iced::{Element, Task};

use super::Ctx;
use crate::theme;

pub use layers::view;

/// The form shown to add a constraint: one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Adding {
    #[default]
    Grant,
    Rule,
    Hook,
    Reviewer,
    Elsewhere,
}

/// A field of a new rule's form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleField {
    /// What the agent is told.
    Text,
    /// The calls it binds: a function pattern, and words a call holds.
    Function,
    Contains,
    Rung,
}

/// A new rule for the step, as typed.
#[derive(Default)]
struct RuleDraft {
    text: String,
    function: String,
    contains: String,
    rung: String,
}

/// The form shown to add a grant: one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Granting {
    #[default]
    Function,
    Command,
    Mcp,
}

#[derive(Default)]
pub struct Rules {
    constraints: Vec<ConstraintView>,
    grants: Vec<CapabilityView>,
    /// Each workspace's charter, read from its repository.
    charters: Vec<WorkspaceCharterView>,
    /// The engine's functions, for the search, and the server's MCP servers.
    catalog: Vec<Registered>,
    mcp: Vec<McpServerView>,
    adding: Adding,
    granting: Granting,
    /// What the constraint list is narrowed to, and the function search.
    filter: String,
    search: String,
    /// A new command grant's words, a hook's calls and command, and a
    /// reviewer agent, as typed.
    command: String,
    hook_function: String,
    hook: String,
    reviewer: String,
    rule: RuleDraft,
    said: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Constraints(Result<Vec<ConstraintView>, String>),
    Grants(Result<Vec<CapabilityView>, String>),
    Catalog(Result<Console, String>),
    Mcp(Result<Vec<McpServerView>, String>),
    Charters(Result<Vec<WorkspaceCharterView>, String>),
    /// Set a constraint's rung: told, checked or enforced.
    SetRung(String, String),
    /// Apply a constraint at the step too, or take it off the step.
    Scope(String, bool),
    /// Give the step its own grants, starting from its kind's.
    Own,
    /// Take the step's own grants back, so its kind's apply.
    Kind,
    /// Set a grant of the step's own to `ask` or `allowed`.
    Rung(CapabilityView, String),
    Ungrant(CapabilityView),
    /// Grant `function` at the step, as `ask` or `allowed`.
    Grant(String, String),
    Adding(Adding),
    Granting(Granting),
    Filter(String),
    Search(String),
    Command(String),
    HookFunction(String),
    Hook(String),
    AddHook,
    Reviewer(String),
    AddReviewer,
    Rule(RuleField, String),
    AddRule,
    /// Remove a hook or a reviewer made for this step.
    Remove(String),
    Done(Result<(), String>),
}

/// The step a page shows rules for: its name, `workflow/step`, and its kind.
pub struct At<'a> {
    pub step: &'a str,
    pub stage: &'a str,
}

pub fn load(ctx: &Ctx) -> Task<Msg> {
    Task::batch([
        ctx.task(&ctx.current, api::Constraints, Msg::Constraints),
        ctx.task(&ctx.current, api::Capabilities, Msg::Grants),
        ctx.task(&ctx.current, api::Console, Msg::Catalog),
        ctx.task(&ctx.current, api::McpServers, Msg::Mcp),
        ctx.task(&ctx.current, api::Charters, Msg::Charters),
    ])
}

fn send<R>(ctx: &Ctx, request: R) -> Task<Msg>
where
    R: Request<Answer = ()> + Send + Sync + 'static,
{
    ctx.task(&ctx.current, request, Msg::Done)
}

impl Rules {
    fn own(&self, step: &str) -> Vec<&CapabilityView> {
        self.grants.iter().filter(|g| g.step == step).collect()
    }

    fn kind(&self, stage: &str) -> Vec<&CapabilityView> {
        let kind = self.grants.iter();
        kind.filter(|g| g.step.is_empty() && g.stage == stage)
            .collect()
    }

    /// The constraints that apply at `step`.
    fn here(&self, step: &str) -> Vec<&ConstraintView> {
        let at = |c: &&ConstraintView| c.steps.is_empty() || c.steps.iter().any(|s| s == step);
        self.constraints.iter().filter(at).collect()
    }

    /// The grants that decide the step's calls: its own, else its kind's.
    fn deciding(&self, at: &At) -> Vec<&CapabilityView> {
        let own = self.own(at.step);
        match own.is_empty() {
            true => self.kind(at.stage),
            false => own,
        }
    }
}

/// How many constraints apply at the step and grants decide its calls, for
/// its Rules tab.
pub fn count(rules: &Rules, at: &At) -> usize {
    rules.here(at.step).len() + rules.deciding(at).len()
}

/// An enforced constraint made for `step` alone, with an id no constraint
/// has yet, such as `hook-short-build-1`.
fn for_step(rules: &Rules, kind: &str, step: &str, rule: String) -> ConstraintView {
    let base = format!("{kind}-{}", step.replace('/', "-"));
    let taken = |id: &String| rules.constraints.iter().any(|c| &c.id == id);
    let id = (1..).map(|n| format!("{base}-{n}")).find(|id| !taken(id));
    ConstraintView {
        id: id.unwrap_or(base),
        rule,
        rung: "enforced".into(),
        source: "owner".into(),
        steps: vec![step.into()],
        ..ConstraintView::default()
    }
}

/// A hook at `step` (D50): enforced, so its exit 2 refuses the call.
fn new_hook(rules: &Rules, step: &str) -> ConstraintView {
    let function = Some(rules.hook_function.trim()).filter(|f| !f.is_empty());
    let function = function.unwrap_or("sandbox::exec");
    let command = rules.hook.trim();
    let rule = format!("Before each {function} call, `{command}` runs; exit 2 refuses it.");
    ConstraintView {
        function: function.into(),
        hook: command.into(),
        ..for_step(rules, "hook", step, rule)
    }
}

/// A reviewer at `step` (D51): the agent must pass the step's change.
fn new_reviewer(rules: &Rules, step: &str) -> ConstraintView {
    let agent = rules.reviewer.trim();
    let rule = format!("The agent {agent} reviews this step's change, and it must pass.");
    ConstraintView {
        reviewer: agent.into(),
        ..for_step(rules, "reviewer", step, rule)
    }
}

/// A rule at `step`: told to the agent, and at checked or enforced it binds
/// the calls to its function that hold its words. Told by default.
fn new_rule(rules: &Rules, step: &str) -> ConstraintView {
    let d = &rules.rule;
    let rung = Some(d.rung.as_str())
        .filter(|r| !r.is_empty())
        .unwrap_or("told");
    ConstraintView {
        function: d.function.trim().into(),
        contains: d.contains.trim().into(),
        rung: rung.into(),
        ..for_step(rules, "rule", step, d.text.trim().into())
    }
}

/// The rule, hook or reviewer a form made for `step`, and the forms cleared.
fn made(rules: &mut Rules, msg: &Msg, step: &str) -> ConstraintView {
    let made = match msg {
        Msg::AddRule => new_rule(rules, step),
        Msg::AddHook => new_hook(rules, step),
        _ => new_reviewer(rules, step),
    };
    rules.hook.clear();
    rules.reviewer.clear();
    rules.rule = RuleDraft::default();
    made
}

fn rule_field(d: &mut RuleDraft, field: RuleField, value: String) {
    match field {
        RuleField::Text => d.text = value,
        RuleField::Function => d.function = value,
        RuleField::Contains => d.contains = value,
        RuleField::Rung => d.rung = value,
    }
}

/// The steps a constraint applies at once `step` is added to or taken off
/// them; none when taking it off would leave none, which means every step.
fn rescoped(c: &ConstraintView, step: &str, add: bool) -> Option<Vec<String>> {
    let mut steps: Vec<String> = c.steps.iter().filter(|s| *s != step).cloned().collect();
    if add {
        steps.push(step.into());
    }
    (!steps.is_empty()).then_some(steps)
}

/// Constraint `id` applied at `step` too, or taken off it; or why not.
fn scope(rules: &Rules, id: String, add: bool, step: &str) -> Result<api::ConstraintSteps, String> {
    let c = rules.constraints.iter().find(|c| c.id == id);
    let c = c.ok_or(format!("No constraint {id}."))?;
    match rescoped(c, step, add) {
        Some(steps) => Ok(api::ConstraintSteps { id, steps }),
        None => Err(format!(
            "This is the only step {id} applies at; taking it off would apply it at every step. Change it on the Constraints page."
        )),
    }
}

/// A grant's rung set to `rung`: one step up the ladder to allowed, or one
/// down to ask.
fn set_rung(g: CapabilityView, rung: String, ctx: &Ctx) -> Task<Msg> {
    if g.rung == rung {
        return Task::none();
    }
    let up = rung == "allowed";
    let moved = api::CapabilityMove {
        stage: g.stage,
        step: g.step,
        function: g.function,
        up,
    };
    send(ctx, moved)
}

/// A grant of `function` at the step, as `rung`.
fn grant(function: &str, rung: String, at: &At) -> CapabilityView {
    CapabilityView {
        stage: at.stage.into(),
        step: at.step.into(),
        function: function.trim().into(),
        rung,
    }
}

/// The step's own grants: a copy of its kind's, or none again.
fn own_or_kind(rules: &Rules, own: bool, at: &At, ctx: &Ctx) -> Task<Msg> {
    match own {
        true => {
            let copies = rules.kind(at.stage).into_iter().map(|g| CapabilityView {
                step: at.step.into(),
                ..g.clone()
            });
            Task::batch(copies.map(|g| send(ctx, api::CapabilityAdd(g))))
        }
        false => {
            let own = rules.own(at.step).into_iter().cloned();
            Task::batch(own.map(|g| send(ctx, api::CapabilityRemove(g))))
        }
    }
}

/// What the forms hold as typed, and which form shows.
fn typed(rules: &mut Rules, msg: Msg) {
    match msg {
        Msg::Adding(a) => rules.adding = a,
        Msg::Granting(g) => rules.granting = g,
        Msg::Filter(f) => rules.filter = f,
        Msg::Search(s) => rules.search = s,
        Msg::Command(c) => rules.command = c,
        Msg::HookFunction(f) => rules.hook_function = f,
        Msg::Hook(h) => rules.hook = h,
        Msg::Reviewer(r) => rules.reviewer = r,
        Msg::Rule(field, value) => rule_field(&mut rules.rule, field, value),
        _ => {}
    }
}

/// What the server answered.
fn read(rules: &mut Rules, msg: Msg) {
    match msg {
        Msg::Constraints(Ok(all)) => rules.constraints = all,
        Msg::Grants(Ok(all)) => rules.grants = all,
        Msg::Catalog(Ok(console)) => rules.catalog = console.functions,
        Msg::Mcp(Ok(all)) => rules.mcp = all,
        Msg::Charters(Ok(all)) => rules.charters = all,
        Msg::Constraints(Err(e)) | Msg::Grants(Err(e)) | Msg::Done(Err(e)) => rules.said = e,
        // The search and the MCP shortcut are a help, not a need.
        _ => {}
    }
}

pub fn update(rules: &mut Rules, msg: Msg, at: &At, ctx: &Ctx) -> Task<Msg> {
    match msg {
        Msg::Done(Ok(_)) => {
            rules.said.clear();
            load(ctx)
        }
        Msg::Remove(id) => send(ctx, api::ConstraintRemove { id }),
        Msg::AddRule | Msg::AddReviewer | Msg::AddHook => {
            send(ctx, api::ConstraintAdd(made(rules, &msg, at.step)))
        }
        Msg::Scope(id, add) => match scope(rules, id, add, at.step) {
            Ok(scoped) => send(ctx, scoped),
            Err(said) => {
                rules.said = said;
                Task::none()
            }
        },
        Msg::Own => own_or_kind(rules, true, at, ctx),
        Msg::Kind => own_or_kind(rules, false, at, ctx),
        Msg::Rung(g, rung) => set_rung(g, rung, ctx),
        Msg::Ungrant(g) => send(ctx, api::CapabilityRemove(g)),
        Msg::SetRung(id, rung) => send(ctx, api::ConstraintRung { id, rung }),
        Msg::Grant(function, rung) => {
            (rules.search, rules.command) = Default::default();
            send(ctx, api::CapabilityAdd(grant(&function, rung, at)))
        }
        m @ (Msg::Constraints(_)
        | Msg::Grants(_)
        | Msg::Catalog(_)
        | Msg::Mcp(_)
        | Msg::Charters(_)
        | Msg::Done(_)) => {
            read(rules, m);
            Task::none()
        }
        m => {
            typed(rules, m);
            Task::none()
        }
    }
}

/// Why the last change failed, when one did.
pub fn said(rules: &Rules) -> Option<Element<'_, Msg>> {
    let said = (!rules.said.is_empty()).then_some(&rules.said)?;
    Some(text(said).size(13).color(theme::BAD).into())
}

fn small<'a>(label: impl Into<String>, msg: Option<Msg>) -> Element<'a, Msg> {
    button(text(label.into()).size(12))
        .padding([3, 10])
        .style(theme::secondary)
        .on_press_maybe(msg)
        .into()
}

/// One choice of a switch, filled when it is the one chosen.
fn choice<'a>(label: impl Into<String>, on: bool, msg: Option<Msg>) -> Element<'a, Msg> {
    button(text(label.into()).size(12))
        .padding([4, 10])
        .style(theme::nav(on))
        .on_press_maybe(msg)
        .into()
}

fn chip<'a>(label: impl Into<String>) -> Element<'a, Msg> {
    iced::widget::container(text(label.into()).size(11).color(theme::MUTED))
        .padding([2, 8])
        .style(theme::chip)
        .into()
}

/// A line of muted help.
fn help<'a>(words: impl Into<String>) -> Element<'a, Msg> {
    text(words.into()).size(12).color(theme::MUTED).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_a_constraint_off_its_only_step_is_refused() {
        let c = ConstraintView {
            steps: vec!["short/build".into()],
            ..ConstraintView::default()
        };
        assert_eq!(rescoped(&c, "short/build", false), None, "else every step");
        let two = ConstraintView {
            steps: vec!["short/build".into(), "short/plan".into()],
            ..ConstraintView::default()
        };
        assert_eq!(
            rescoped(&two, "short/build", false),
            Some(vec!["short/plan".to_string()])
        );
    }

    #[test]
    fn a_new_hook_takes_an_id_no_constraint_has() {
        let mut rules = Rules {
            hook: " ./lint.sh ".into(),
            ..Rules::default()
        };
        let first = new_hook(&rules, "short/build");
        assert_eq!(
            (first.id.as_str(), first.function.as_str()),
            ("hook-short-build-1", "sandbox::exec")
        );
        rules.constraints.push(first);
        assert_eq!(new_hook(&rules, "short/build").id, "hook-short-build-2");
    }

    #[test]
    fn a_new_rule_is_told_unless_its_rung_says_more() {
        let mut rules = Rules::default();
        rule_field(
            &mut rules.rule,
            RuleField::Text,
            " Keep diffs small. ".into(),
        );
        let told = new_rule(&rules, "short/build");
        assert_eq!(
            (told.rule.as_str(), told.rung.as_str()),
            ("Keep diffs small.", "told")
        );
        assert_eq!(
            (told.id.as_str(), told.steps.as_slice()),
            ("rule-short-build-1", &["short/build".to_string()][..])
        );
        rule_field(&mut rules.rule, RuleField::Rung, "enforced".into());
        rule_field(&mut rules.rule, RuleField::Function, "sandbox::exec".into());
        let enforced = new_rule(&rules, "short/build");
        assert_eq!(
            (enforced.rung.as_str(), enforced.function.as_str()),
            ("enforced", "sandbox::exec")
        );
    }

    #[test]
    fn a_step_counts_its_own_grants_once_it_has_any() {
        let grant = |stage: &str, step: &str| CapabilityView {
            stage: stage.into(),
            step: step.into(),
            function: "sandbox::*".into(),
            rung: "allowed".into(),
        };
        let mut rules = Rules {
            grants: vec![grant("prompt", ""), grant("agent", "")],
            constraints: vec![ConstraintView::default()],
            ..Rules::default()
        };
        let at = At {
            step: "short/review",
            stage: "prompt",
        };
        assert_eq!(count(&rules, &at), 2, "its kind's grant and the rule");
        rules.grants.push(grant("prompt", "short/review"));
        rules.grants.push(grant("prompt", "short/review"));
        assert_eq!(count(&rules, &at), 3, "its own two and the rule");
    }
}
