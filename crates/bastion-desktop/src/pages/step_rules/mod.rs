//! The rules on the workflows page, at a level (D53, D54): every workflow,
//! one workflow, or one step. Each constraint and grant is one item that a
//! level turns on, off, or leaves to inherit; the most specific setting
//! wins. A rung belongs to the item, so it reads the same at every level.

mod capabilities;
mod constraints;
mod layers;
mod scope;

use bastion_protocol::api::{self, Request, ScopeSet};
use bastion_protocol::{
    CapabilityView, Console, ConstraintView, FactoryCapabilityView, McpServerView, Registered,
    WorkspaceCharterView,
};
use iced::widget::{button, text};
use iced::{Element, Task};

use super::Ctx;
use crate::theme;

pub use layers::view;
pub use scope::EVERY;

/// The form shown to add an item: one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Adding {
    /// Feedforward: a rule told to the agent, or a note on what it may do.
    #[default]
    Rule,
    Note,
    /// Feedback: a word match, a hook, a reviewer, or a grant.
    Check,
    Hook,
    Reviewer,
    Grant,
}

/// A field of a new rule's form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleField {
    /// What the agent is told.
    Text,
    /// The calls it binds: a function pattern, and words a call holds.
    Function,
    Contains,
}

/// A new rule, as typed.
#[derive(Default)]
struct RuleDraft {
    text: String,
    function: String,
    contains: String,
}

/// A field of a rule's deterministic form (D55).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormField {
    Function,
    Contains,
    Hook,
}

/// A rule's deterministic form being edited: the calls and words code
/// matches, or a hook it runs; and the rung to promote it to on save.
#[derive(Clone, Debug, Default)]
pub(super) struct FormDraft {
    pub(super) id: String,
    pub(super) function: String,
    pub(super) contains: String,
    pub(super) hook: String,
    pub(super) rung: Option<String>,
}

/// The form shown to add a grant: one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Granting {
    #[default]
    Function,
    Command,
    Mcp,
}

/// An item a level turns on or off: a constraint by id, or a grant by its
/// kind and function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Constraint(String),
    Grant(String, String),
}

#[derive(Default)]
pub struct Rules {
    constraints: Vec<ConstraintView>,
    grants: Vec<CapabilityView>,
    /// Each workspace's charter, read from its repository, and what the
    /// factory itself may do.
    charters: Vec<WorkspaceCharterView>,
    factory: Vec<FactoryCapabilityView>,
    /// The kind a grant added above a step is for: prompt, agent or skill.
    for_kind: String,
    /// The engine's functions, for the search, and the server's MCP servers.
    catalog: Vec<Registered>,
    mcp: Vec<McpServerView>,
    adding: Adding,
    granting: Granting,
    /// What the list is narrowed to, and the function search.
    filter: String,
    search: String,
    /// A new command grant's words, a hook's calls and command, and a
    /// reviewer agent, as typed.
    command: String,
    hook_function: String,
    hook: String,
    reviewer: String,
    rule: RuleDraft,
    /// The rule whose deterministic form is open, if any.
    pub(super) forming: Option<FormDraft>,
    said: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Constraints(Result<Vec<ConstraintView>, String>),
    Grants(Result<Vec<CapabilityView>, String>),
    Catalog(Result<Console, String>),
    Mcp(Result<Vec<McpServerView>, String>),
    Charters(Result<Vec<WorkspaceCharterView>, String>),
    Factory(Result<Vec<FactoryCapabilityView>, String>),
    /// The kind a new grant above a step is for.
    ForKind(String),
    /// Set an item `on`, `off` or `inherit` at the level shown.
    SetScope(Item, String),
    /// Set a constraint's rung: told, checked or enforced.
    SetRung(String, String),
    /// Set a grant's rung: ask or allowed.
    GrantRung(CapabilityView, String),
    /// Remove a grant everywhere.
    Ungrant(CapabilityView),
    /// Grant `function` on at the level shown, as `ask` or `allowed`.
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
    AddCheck,
    AddNote,
    /// Open a rule's deterministic form, with the rung to promote it to.
    Form(String, Option<String>),
    FormField(FormField, String),
    SaveForm,
    CancelForm,
    /// Remove a constraint made in the app, everywhere.
    Remove(String),
    Done(Result<(), String>),
}

/// The level a page shows rules for: its scope, `*`, a workflow, or
/// `workflow/step`, and at a step, the step's kind.
pub struct At<'a> {
    pub scope: &'a str,
    pub stage: &'a str,
}

impl At<'_> {
    fn is_step(&self) -> bool {
        self.scope.contains('/')
    }

    fn is_every(&self) -> bool {
        self.scope == EVERY
    }
}

pub fn load(ctx: &Ctx) -> Task<Msg> {
    Task::batch([
        ctx.task(&ctx.current, api::Constraints, Msg::Constraints),
        ctx.task(&ctx.current, api::Capabilities, Msg::Grants),
        ctx.task(&ctx.current, api::Console, Msg::Catalog),
        ctx.task(&ctx.current, api::McpServers, Msg::Mcp),
        ctx.task(&ctx.current, api::Charters, Msg::Charters),
        ctx.task(&ctx.current, api::FactoryCapabilities, Msg::Factory),
    ])
}

fn send<R>(ctx: &Ctx, request: R) -> Task<Msg>
where
    R: Request<Answer = ()> + Send + Sync + 'static,
{
    ctx.task(&ctx.current, request, Msg::Done)
}

impl Rules {
    /// A kind's grants: one item per function.
    fn kind(&self, stage: &str) -> Vec<&CapabilityView> {
        let kind = self.grants.iter();
        kind.filter(|g| g.step.is_empty() && g.stage == stage)
            .collect()
    }

    /// The kind a grant made at `at` is for: the step's, or above a step,
    /// the one the form picked, a prompt's by default.
    fn kind_of<'a>(&'a self, at: &At<'a>) -> &'a str {
        match (at.is_step(), self.for_kind.as_str()) {
            (true, _) => at.stage,
            (false, "") => "prompt",
            (false, k) => k,
        }
    }
}

/// How many constraints and grants are on at a step, for its Rules tab.
pub fn count(rules: &Rules, at: &At) -> usize {
    let on = |scopes: &[String]| scope::on(scopes, at.scope);
    let constraints = rules.constraints.iter().filter(|c| on(&c.steps)).count();
    constraints
        + rules
            .kind(at.stage)
            .into_iter()
            .filter(|g| on(&g.scopes))
            .count()
}

/// An enforced constraint made at `scope`, on there only, or everywhere at
/// every workflow, with an id no constraint has yet, such as
/// `hook-short-build-1` or `rule-every-1`.
fn made_at(rules: &Rules, kind: &str, scope: &str, rule: String) -> ConstraintView {
    let at = match scope {
        EVERY => "every".to_string(),
        s => s.replace('/', "-"),
    };
    let base = format!("{kind}-{at}");
    let taken = |id: &String| rules.constraints.iter().any(|c| &c.id == id);
    let id = (1..).map(|n| format!("{base}-{n}")).find(|id| !taken(id));
    ConstraintView {
        id: id.unwrap_or(base),
        rule,
        rung: "enforced".into(),
        source: "owner".into(),
        steps: (scope != EVERY)
            .then(|| scope.to_string())
            .into_iter()
            .collect(),
        ..ConstraintView::default()
    }
}

/// A hook (D50): enforced, so its exit 2 refuses the call.
fn new_hook(rules: &Rules, scope: &str) -> ConstraintView {
    let function = Some(rules.hook_function.trim()).filter(|f| !f.is_empty());
    let function = function.unwrap_or("sandbox::exec");
    let command = rules.hook.trim();
    let rule = format!("Before each {function} call, `{command}` runs; exit 2 refuses it.");
    ConstraintView {
        function: function.into(),
        hook: command.into(),
        ..made_at(rules, "hook", scope, rule)
    }
}

/// A reviewer (D51): the agent must pass the step's change.
fn new_reviewer(rules: &Rules, scope: &str) -> ConstraintView {
    let agent = rules.reviewer.trim();
    let rule = format!("The agent {agent} reviews the step's change, and it must pass.");
    ConstraintView {
        reviewer: agent.into(),
        ..made_at(rules, "reviewer", scope, rule)
    }
}

/// A feedforward rule (D56): told to the agent, and judged by it.
fn new_rule(rules: &Rules, scope: &str) -> ConstraintView {
    ConstraintView {
        rung: "told".into(),
        ..made_at(rules, "rule", scope, rules.rule.text.trim().into())
    }
}

/// A feedback check (D56): the gate refuses a call to its function that
/// holds its words.
fn new_check(rules: &Rules, scope: &str) -> ConstraintView {
    let d = &rules.rule;
    ConstraintView {
        function: d.function.trim().into(),
        contains: d.contains.trim().into(),
        ..made_at(rules, "rule", scope, d.text.trim().into())
    }
}

/// A feedforward capability (D56): a note told to the agent on what it may
/// do, such as a skill to use.
fn new_note(rules: &Rules, scope: &str) -> ConstraintView {
    ConstraintView {
        rung: "told".into(),
        capability: true,
        ..made_at(rules, "note", scope, rules.rule.text.trim().into())
    }
}

/// The rule, hook or reviewer a form made at `scope`, and the forms cleared.
fn made(rules: &mut Rules, msg: &Msg, scope: &str) -> ConstraintView {
    let made = match msg {
        Msg::AddRule => new_rule(rules, scope),
        Msg::AddCheck => new_check(rules, scope),
        Msg::AddNote => new_note(rules, scope),
        Msg::AddHook => new_hook(rules, scope),
        _ => new_reviewer(rules, scope),
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
    }
}

/// An item set `on`, `off` or `inherit` at `scope`.
fn scope_set(item: Item, setting: String, scope: &str) -> ScopeSet {
    let (constraint, stage, function) = match item {
        Item::Constraint(id) => (id, String::new(), String::new()),
        Item::Grant(stage, function) => (String::new(), stage, function),
    };
    ScopeSet {
        constraint,
        stage,
        function,
        scope: scope.into(),
        setting,
    }
}

/// A grant's rung set to `rung`: one step up the ladder to allowed, or one
/// down to ask.
fn set_rung(g: CapabilityView, rung: String, ctx: &Ctx) -> Task<Msg> {
    if g.rung == rung {
        return Task::none();
    }
    let moved = api::CapabilityMove {
        stage: g.stage,
        step: g.step,
        function: g.function,
        up: rung == "allowed",
    };
    send(ctx, moved)
}

/// A grant of `function` for `stage`, on at `scope`, as `rung`. The server
/// reads the view's step as the scope to turn it on at; none is every
/// workflow.
fn grant(function: &str, rung: String, stage: &str, scope: &str) -> CapabilityView {
    let scope = if scope == EVERY { "" } else { scope };
    CapabilityView {
        stage: stage.into(),
        step: scope.into(),
        function: function.trim().into(),
        rung,
        ..CapabilityView::default()
    }
}

/// The open form for rule `id`, filled from what it has.
fn open_form(rules: &mut Rules, id: String, rung: Option<String>) {
    let c = rules.constraints.iter().find(|c| c.id == id);
    let mut draft = FormDraft {
        id,
        rung,
        ..FormDraft::default()
    };
    if let Some(c) = c {
        (draft.function, draft.contains, draft.hook) =
            (c.function.clone(), c.contains.clone(), c.hook.clone());
    }
    rules.forming = Some(draft);
}

/// The open form, saved: its calls, words and hook, and its rung if it
/// promotes the rule.
fn save_form(rules: &mut Rules, ctx: &Ctx) -> Task<Msg> {
    let Some(d) = rules.forming.take() else {
        return Task::none();
    };
    let form = api::ConstraintForm {
        id: d.id,
        function: d.function,
        contains: d.contains,
        hook: d.hook,
        rung: d.rung,
    };
    send(ctx, form)
}

fn form_field(d: &mut FormDraft, field: FormField, value: String) {
    match field {
        FormField::Function => d.function = value,
        FormField::Contains => d.contains = value,
        FormField::Hook => d.hook = value,
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
        Msg::ForKind(k) => rules.for_kind = k,
        Msg::Form(id, rung) => open_form(rules, id, rung),
        Msg::CancelForm => rules.forming = None,
        Msg::FormField(field, value) => {
            if let Some(d) = rules.forming.as_mut() {
                form_field(d, field, value);
            }
        }
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
        Msg::Factory(Ok(all)) => rules.factory = all,
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
        Msg::SaveForm => save_form(rules, ctx),
        Msg::AddRule | Msg::AddCheck | Msg::AddNote | Msg::AddReviewer | Msg::AddHook => {
            send(ctx, api::ConstraintAdd(made(rules, &msg, at.scope)))
        }
        Msg::SetScope(item, setting) => send(ctx, api::Scope(scope_set(item, setting, at.scope))),
        Msg::GrantRung(g, rung) => set_rung(g, rung, ctx),
        Msg::Ungrant(g) => send(ctx, api::CapabilityRemove(g)),
        Msg::SetRung(id, rung) => send(ctx, api::ConstraintRung { id, rung }),
        Msg::Grant(function, rung) => {
            (rules.search, rules.command) = Default::default();
            let kind = rules.kind_of(at).to_string();
            send(
                ctx,
                api::CapabilityAdd(grant(&function, rung, &kind, at.scope)),
            )
        }
        m @ (Msg::Constraints(_)
        | Msg::Grants(_)
        | Msg::Catalog(_)
        | Msg::Mcp(_)
        | Msg::Charters(_)
        | Msg::Factory(_)
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

/// An item's setting at the level shown: Inherit (with what it inherits),
/// On or Off. Every workflow inherits nothing, so it shows On and Off.
fn scope_switch<'a>(item: Item, scopes: &[String], at: &At, editable: bool) -> Element<'a, Msg> {
    let (own, inherited) = scope::at(scopes, at.scope);
    let now = own.unwrap_or(inherited);
    let set = |label: String, chosen: bool, to: &str| {
        let msg = (editable && !chosen).then(|| Msg::SetScope(item.clone(), to.into()));
        choice(label, chosen, msg)
    };
    let word = if inherited { "on" } else { "off" };
    // "here" sets it apart from the rung switch beside it.
    let here = text("here").size(11).color(theme::MUTED);
    let mut switch = iced::widget::row![here].spacing(2).align_y(iced::Center);
    if !at.is_every() {
        switch = switch.push(set(format!("Inherit · {word}"), own.is_none(), "inherit"));
    }
    let every = at.is_every() || own.is_some();
    switch
        .push(set("On".into(), every && now, "on"))
        .push(set("Off".into(), every && !now, "off"))
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
    fn a_new_item_is_on_where_it_was_made() {
        let mut rules = Rules {
            hook: " ./lint.sh ".into(),
            ..Rules::default()
        };
        let first = new_hook(&rules, "short/build");
        assert_eq!(
            (first.id.as_str(), first.steps.as_slice()),
            ("hook-short-build-1", &["short/build".to_string()][..])
        );
        rules.constraints.push(first);
        assert_eq!(new_hook(&rules, "short/build").id, "hook-short-build-2");
        let every = new_hook(&rules, EVERY);
        assert_eq!(every.id, "hook-every-1");
        assert!(every.steps.is_empty(), "no steps is every step");
        let at_workflow = grant("sandbox::fs::read", "ask".into(), "prompt", "short");
        assert_eq!(at_workflow.step, "short", "on for the workflow");
        assert_eq!(grant("x", "ask".into(), "prompt", EVERY).step, "");
    }

    #[test]
    fn a_rule_is_feedforward_a_check_is_feedback_and_a_note_is_a_capability() {
        let mut rules = Rules::default();
        rule_field(
            &mut rules.rule,
            RuleField::Text,
            " Keep diffs small. ".into(),
        );
        rule_field(&mut rules.rule, RuleField::Function, "sandbox::exec".into());
        let rule = new_rule(&rules, "short/build");
        assert_eq!(
            (
                rule.rule.as_str(),
                rule.rung.as_str(),
                rule.function.as_str()
            ),
            ("Keep diffs small.", "told", "")
        );
        let check = new_check(&rules, "short/build");
        assert_eq!(
            (check.rung.as_str(), check.function.as_str()),
            ("enforced", "sandbox::exec")
        );
        let note = new_note(&rules, "short");
        assert!(note.capability && note.rung == "told");
        assert_eq!(note.id, "note-short-1");
    }

    #[test]
    fn a_rule_form_opens_with_what_it_has_and_saves_its_promotion() {
        let mut rules = Rules {
            constraints: vec![ConstraintView {
                id: "rule-every-1".into(),
                hook: "./lint.sh".into(),
                ..ConstraintView::default()
            }],
            ..Rules::default()
        };
        open_form(&mut rules, "rule-every-1".into(), Some("enforced".into()));
        let d = rules.forming.as_ref().unwrap();
        assert_eq!(
            (d.hook.as_str(), d.rung.as_deref()),
            ("./lint.sh", Some("enforced"))
        );
    }

    #[test]
    fn a_step_counts_what_is_on_there() {
        let grant = |scopes: &[&str]| CapabilityView {
            stage: "prompt".into(),
            function: "sandbox::*".into(),
            rung: "allowed".into(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            ..CapabilityView::default()
        };
        let rules = Rules {
            grants: vec![grant(&[]), grant(&["*", "!short"])],
            constraints: vec![ConstraintView::default()],
            ..Rules::default()
        };
        let at = At {
            scope: "short/review",
            stage: "prompt",
        };
        assert_eq!(
            count(&rules, &at),
            2,
            "the rule and the grant on everywhere"
        );
    }
}
