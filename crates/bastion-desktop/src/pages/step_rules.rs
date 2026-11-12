//! The rules at one workflow step, on the workflows page (D44, D48): the
//! constraints that apply there, and the grants that decide what its
//! agent's calls may do. A step uses its kind's grants until it has its own,
//! which then take their place.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{CapabilityView, ConstraintView};
use iced::widget::{Column, button, column, container, pick_list, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{Choice, Ctx, labeled};
use crate::theme;

#[derive(Default)]
pub struct Rules {
    constraints: Vec<ConstraintView>,
    grants: Vec<CapabilityView>,
    /// A new grant for the step, as the form holds it.
    function: String,
    rung: String,
    /// A new hook for the step: the calls it runs before, and its command.
    hook_function: String,
    hook: String,
    said: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Constraints(Result<Vec<ConstraintView>, String>),
    Grants(Result<Vec<CapabilityView>, String>),
    /// Apply a constraint at the step too, or take it off the step.
    Scope(String, bool),
    /// Give the step its own grants, starting from its kind's.
    Own,
    /// Take the step's own grants back, so its kind's apply.
    Kind,
    Move(CapabilityView, bool),
    Function(String),
    Rung(String),
    Add,
    HookFunction(String),
    Hook(String),
    AddHook,
    /// Remove a hook made for this step.
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
}

/// A hook at `step` (D50): enforced, so its exit 2 refuses the call, with an
/// id no constraint has yet.
fn new_hook(rules: &Rules, step: &str) -> ConstraintView {
    let function = rules.hook_function.trim();
    let function = if function.is_empty() {
        "sandbox::exec"
    } else {
        function
    };
    let command = rules.hook.trim();
    let base = format!("hook-{}", step.replace('/', "-"));
    let taken = |id: &String| rules.constraints.iter().any(|c| &c.id == id);
    let id = (1..).map(|n| format!("{base}-{n}")).find(|id| !taken(id));
    ConstraintView {
        id: id.unwrap_or(base),
        rule: format!("Before each {function} call, `{command}` runs; exit 2 refuses it."),
        function: function.into(),
        rung: "enforced".into(),
        source: "owner".into(),
        steps: vec![step.into()],
        hook: command.into(),
        ..ConstraintView::default()
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

pub fn update(rules: &mut Rules, msg: Msg, at: &At, ctx: &Ctx) -> Task<Msg> {
    match msg {
        Msg::Constraints(Ok(all)) => rules.constraints = all,
        Msg::Grants(Ok(all)) => rules.grants = all,
        Msg::Constraints(Err(e)) | Msg::Grants(Err(e)) | Msg::Done(Err(e)) => rules.said = e,
        Msg::Done(Ok(_)) => {
            rules.said.clear();
            return load(ctx);
        }
        Msg::Function(f) => rules.function = f,
        Msg::HookFunction(f) => rules.hook_function = f,
        Msg::Hook(h) => rules.hook = h,
        Msg::Remove(id) => return send(ctx, api::ConstraintRemove { id }),
        Msg::AddHook => {
            let hook = new_hook(rules, at.step);
            rules.hook.clear();
            return send(ctx, api::ConstraintAdd(hook));
        }
        Msg::Rung(r) => rules.rung = r,
        Msg::Scope(id, add) => {
            let Some(c) = rules.constraints.iter().find(|c| c.id == id) else {
                return Task::none();
            };
            let Some(steps) = rescoped(c, at.step, add) else {
                rules.said = format!(
                    "This is the only step {id} applies at; taking it off would apply it at every step. Change it on the Constraints page."
                );
                return Task::none();
            };
            return send(ctx, api::ConstraintSteps { id, steps });
        }
        Msg::Own => {
            let copies = rules.kind(at.stage).into_iter().map(|g| CapabilityView {
                step: at.step.into(),
                ..g.clone()
            });
            return Task::batch(copies.map(|g| send(ctx, api::CapabilityAdd(g))));
        }
        Msg::Kind => {
            let own = rules.own(at.step).into_iter().cloned();
            return Task::batch(own.map(|g| send(ctx, api::CapabilityRemove(g))));
        }
        Msg::Move(g, up) => {
            let moved = api::CapabilityMove {
                stage: g.stage,
                step: g.step,
                function: g.function,
                up,
            };
            return send(ctx, moved);
        }
        Msg::Add => {
            let grant = CapabilityView {
                stage: at.stage.into(),
                step: at.step.into(),
                function: rules.function.trim().into(),
                rung: if rules.rung.is_empty() {
                    "ask".into()
                } else {
                    rules.rung.clone()
                },
            };
            rules.function.clear();
            return send(ctx, api::CapabilityAdd(grant));
        }
    }
    Task::none()
}

fn small<'a>(label: &'a str, msg: Option<Msg>) -> Element<'a, Msg> {
    button(text(label).size(12))
        .padding([3, 10])
        .style(theme::secondary)
        .on_press_maybe(msg)
        .into()
}

fn chip<'a>(label: String) -> Element<'a, Msg> {
    container(text(label).size(12).color(theme::MUTED))
        .padding([2, 8])
        .style(theme::chip)
        .into()
}

/// Where a constraint applies, and how it comes off the step: a hook made
/// for this step alone is removed, and any other scoped one is "Not here".
fn scope<'a>(c: &ConstraintView, at: &At, editable: bool) -> Element<'a, Msg> {
    let only_here = c.steps.len() == 1 && c.steps[0] == at.step;
    let msg = |m: Msg| editable.then_some(m);
    match (c.steps.is_empty(), !c.hook.is_empty() && only_here) {
        (true, _) => chip("every step".into()),
        (false, true) => small("Remove", msg(Msg::Remove(c.id.clone()))),
        (false, false) => small("Not here", msg(Msg::Scope(c.id.clone(), false))),
    }
}

/// The constraints that apply at the step, and a picker for one that
/// applies at other steps to apply here too.
fn constraints<'a>(rules: &Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let here = rules
        .constraints
        .iter()
        .filter(|c| c.steps.is_empty() || c.steps.iter().any(|s| s == at.step));
    let rows = here.map(|c| {
        let scope = scope(c, at, editable);
        row![
            chip(c.rung.clone()),
            text(c.rule.clone()).size(13).width(Fill),
            scope
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    });
    let elsewhere: Vec<Choice> = rules
        .constraints
        .iter()
        .filter(|c| !c.steps.is_empty() && !c.steps.iter().any(|s| s == at.step))
        .map(|c| Choice::new(&c.id, &c.rule))
        .collect();
    let mut body = Column::with_children(rows).spacing(6);
    if editable {
        body = body.push(hook_form(rules));
    }
    if editable && !elsewhere.is_empty() {
        let add = pick_list(elsewhere, None::<Choice>, |c| Msg::Scope(c.id, true))
            .placeholder("Also apply here a constraint for other steps…")
            .text_size(12)
            .style(theme::select);
        body = body.push(add);
    }
    body.into()
}

/// The form for a hook at the step: the calls it runs before, and its
/// command, which runs in the job's VM.
fn hook_form<'a>(rules: &Rules) -> Element<'a, Msg> {
    let named = !rules.hook.trim().is_empty();
    row![
        text_input("sandbox::exec", &rules.hook_function)
            .on_input(Msg::HookFunction)
            .style(theme::field)
            .width(180),
        text_input("./.agents/hooks/check.sh", &rules.hook)
            .on_input(Msg::Hook)
            .style(theme::field),
        small("Add hook", named.then_some(Msg::AddHook)),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// One grant: its rung and its function, with its ladder when the step's
/// own grant may be changed.
fn grant_line<'a>(g: &CapabilityView, changeable: bool) -> Element<'a, Msg> {
    let line = row![
        chip(g.rung.clone()),
        text(g.function.clone()).size(13).width(Fill)
    ]
    .spacing(8)
    .align_y(iced::Center);
    if !changeable {
        return line.into();
    }
    let go = |up| Msg::Move(g.clone(), up);
    let down = if g.rung == "ask" { "Remove" } else { "Demote" };
    let up = (g.rung != "allowed").then(|| go(true));
    line.push(small(down, Some(go(false))))
        .push(small("Promote", up))
        .into()
}

/// The form for one more of the step's own grants, and the way back to
/// its kind's.
fn grant_form(rules: &Rules) -> Element<'_, Msg> {
    let rungs = vec!["ask".to_string(), "allowed".into()];
    let rung = Some(rules.rung.clone()).filter(|r| !r.is_empty());
    let named = !rules.function.trim().is_empty();
    row![
        text_input(
            "sandbox::exec cargo test, or mcp::github::*",
            &rules.function
        )
        .on_input(Msg::Function)
        .style(theme::field),
        pick_list(rungs, rung.or(Some("ask".into())), Msg::Rung).style(theme::select),
        small("Grant", named.then_some(Msg::Add)),
        small("Use its kind's instead", Some(Msg::Kind)),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// The grants that decide the step's calls: its own, with their ladder and
/// a form to add one, or its kind's, which it may take as its own.
fn grants<'a>(rules: &'a Rules, at: &At, editable: bool) -> Element<'a, Msg> {
    let own = rules.own(at.step);
    let mine = !own.is_empty();
    let shown = if mine { own } else { rules.kind(at.stage) };
    let rows = shown.into_iter().map(|g| grant_line(g, mine && editable));
    let title = match mine {
        true => {
            "This step's own grants. A call none covers is refused. A grant on sandbox::exec may name a command: then it covers only that command, alone."
        }
        false => "Its kind's grants. A call none covers is refused.",
    };
    let body = Column::with_children(rows).spacing(6);
    let body = match (editable, mine) {
        (true, true) => body.push(grant_form(rules)),
        (true, false) => body.push(small("Give this step its own grants", Some(Msg::Own))),
        _ => body,
    };
    column![text(title).size(12).color(theme::MUTED), body]
        .spacing(6)
        .into()
}

fn panel<'a>(title: &'a str, body: Element<'a, Msg>) -> Element<'a, Msg> {
    labeled(
        title,
        container(body).padding(12).width(Fill).style(theme::panel),
    )
}

/// The step's constraints and its capabilities, each in its own section,
/// then why the last change failed, when one did.
pub fn view<'a>(rules: &'a Rules, at: &At, editable: bool) -> Vec<Element<'a, Msg>> {
    let mut all = vec![
        panel("Constraints", constraints(rules, at, editable)),
        panel("Capabilities", grants(rules, at, editable)),
    ];
    if !rules.said.is_empty() {
        all.push(text(&rules.said).size(13).color(theme::BAD).into());
    }
    all
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
        assert_eq!(
            rescoped(&c, "short/plan", true),
            Some(vec!["short/build".to_string(), "short/plan".to_string()])
        );
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
        assert_eq!(first.id, "hook-short-build-1");
        assert_eq!(first.function, "sandbox::exec");
        assert_eq!(first.hook, "./lint.sh");
        rules.constraints.push(first);
        assert_eq!(new_hook(&rules, "short/build").id, "hook-short-build-2");
    }

    #[test]
    fn a_step_shows_its_own_grants_once_it_has_any() {
        let grant = |stage: &str, step: &str| CapabilityView {
            stage: stage.into(),
            step: step.into(),
            function: "sandbox::*".into(),
            rung: "allowed".into(),
        };
        let rules = Rules {
            grants: vec![
                grant("prompt", ""),
                grant("agent", ""),
                grant("prompt", "short/review"),
            ],
            ..Rules::default()
        };
        assert_eq!(rules.kind("prompt").len(), 1);
        assert_eq!(rules.own("short/review").len(), 1);
        assert!(rules.own("short/build").is_empty());
    }
}
