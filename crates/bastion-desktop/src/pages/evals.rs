//! The Evals pages: every eval with its progress, a form to run
//! one, and one eval as A against B per request and in total, each refused
//! call listed with a link to its job.

use bastion_protocol::api::{self, Request};
use bastion_protocol::{
    ConstraintView, EvalRow, EvalSummary, EvalView, NewEval, RunView, Workspace,
};
use iced::widget::{
    Column, Row, button, column, container, pick_list, row, text, text_editor, text_input,
};
use iced::{Element, Fill, Task};

use super::{Choice, Ctx, ago, labeled, scrolled, section};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

const RUNGS: [&str; 3] = ["told", "checked", "enforced"];

fn short(base: &str) -> &str {
    &base[..7.min(base.len())]
}

/// The form to run an eval: requests, and the one rung A and B set.
pub struct Form {
    open: bool,
    name: String,
    workspaces: Vec<Workspace>,
    workspace: Option<String>,
    requests: text_editor::Content,
    targets: Vec<Choice>,
    target: Option<Choice>,
    rung_a: String,
    rung_b: String,
    error: String,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            open: false,
            name: String::new(),
            workspaces: vec![],
            workspace: None,
            requests: text_editor::Content::new(),
            targets: vec![],
            target: None,
            rung_a: "told".into(),
            rung_b: "enforced".into(),
            error: String::new(),
        }
    }
}

impl Form {
    /// The eval as the server takes it; a choice left alone is its first option.
    fn eval(&self) -> NewEval {
        let workspace = self
            .workspace
            .clone()
            .or_else(|| self.workspaces.first().map(|w| w.name.clone()));
        let target = self
            .target
            .clone()
            .or_else(|| self.targets.first().cloned())
            .map(|c| c.id);
        let requests: Vec<String> = self
            .requests
            .text()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect();
        NewEval {
            name: self.name.trim().into(),
            workspace: workspace.unwrap_or_default(),
            requests,
            target: target.unwrap_or_default(),
            rung_a: self.rung_a.clone(),
            rung_b: self.rung_b.clone(),
        }
    }
}

#[derive(Default)]
pub struct List {
    evals: Vec<EvalSummary>,
    error: String,
    form: Form,
}

pub struct One {
    id: String,
    eval: Option<EvalView>,
    error: String,
    note: String,
}

impl One {
    pub fn new(id: String) -> Self {
        One {
            id,
            eval: None,
            error: String::new(),
            note: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Rung {
    A,
    B,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Evals(Result<Vec<EvalSummary>, String>),
    Workspaces(Result<Vec<Workspace>, String>),
    Constraints(Result<Vec<ConstraintView>, String>),
    ToggleForm,
    Name(String),
    Workspace(String),
    Requests(text_editor::Action),
    Target(Choice),
    Rung(Rung, String),
    Run,
    Created(Result<String, String>),
    Eval(Result<Box<EvalView>, String>),
    Apply(&'static str),
    Applied(&'static str, Result<(), String>),
}

fn task<R>(ctx: &Ctx, request: R, then: fn(Result<R::Answer, String>) -> Msg) -> Task<Message>
where
    R: Request + Send + Sync + 'static,
    R::Answer: Send + 'static,
{
    ctx.task(&ctx.current, request, then).map(Message::Evals)
}

pub fn load_list(ctx: &Ctx) -> Task<Message> {
    Task::batch([
        task(ctx, api::Evals, Msg::Evals),
        task(ctx, api::Workspaces, Msg::Workspaces),
        task(ctx, api::Constraints, Msg::Constraints),
    ])
}

pub fn load_one(one: &One, ctx: &Ctx) -> Task<Message> {
    let read = api::Eval { id: one.id.clone() };
    task(ctx, read, |r| Msg::Eval(r.map(Box::new)))
}

fn targets(all: Vec<ConstraintView>) -> Vec<Choice> {
    all.iter()
        .map(|c| {
            Choice::new(
                &format!("constraint:{}", c.id),
                &format!("{} ({})", c.id, c.rung),
            )
        })
        .collect()
}

fn on_form(form: &mut Form, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Workspaces(Ok(all)) => form.workspaces = all,
        Msg::Constraints(Ok(all)) => form.targets = targets(all),
        Msg::Workspaces(Err(e)) | Msg::Constraints(Err(e)) | Msg::Created(Err(e)) => form.error = e,
        Msg::ToggleForm => form.open = !form.open,
        Msg::Name(n) => form.name = n,
        Msg::Workspace(w) => form.workspace = Some(w),
        Msg::Requests(a) => form.requests.perform(a),
        Msg::Target(t) => form.target = Some(t),
        Msg::Rung(Rung::A, r) => form.rung_a = r,
        Msg::Rung(Rung::B, r) => form.rung_b = r,
        Msg::Run => {
            return task(ctx, api::EvalCreate(form.eval()), Msg::Created);
        }
        Msg::Created(Ok(id)) => return Task::done(Message::Go(Route::Eval(id))),
        _ => {}
    }
    Task::none()
}

pub fn update_list(list: &mut List, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Evals(Ok(all)) => list.evals = all,
        Msg::Evals(Err(e)) => list.error = e,
        other => return on_form(&mut list.form, other, ctx),
    }
    Task::none()
}

fn applied(arm: &str, answer: Result<(), String>) -> String {
    match answer {
        Ok(_) => format!(
            "Arm {} is now the live policy. The audit records this eval.",
            arm.to_uppercase()
        ),
        Err(e) => e,
    }
}

pub fn update_one(one: &mut One, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Eval(Ok(v)) => one.eval = Some(*v),
        Msg::Eval(Err(e)) => one.error = e,
        Msg::Apply(arm) => {
            let apply = api::EvalApply {
                id: one.id.clone(),
                arm: arm.into(),
            };
            let read = ctx.clone().get(ctx.current.clone(), apply);
            return Task::perform(read, move |a| Message::Evals(Msg::Applied(arm, a)));
        }
        Msg::Applied(arm, answer) => one.note = applied(arm, answer),
        _ => {}
    }
    Task::none()
}

fn rung_pick<'a>(arm: Rung, value: &str) -> Element<'a, Message> {
    let all: Vec<String> = RUNGS.iter().map(|r| r.to_string()).collect();
    pick_list(all, Some(value.to_string()), move |r| {
        Message::Evals(Msg::Rung(arm, r))
    })
    .style(theme::select)
    .into()
}

fn form_view(form: &Form) -> Element<'_, Message> {
    let msg = Message::Evals;
    let arrow = if form.open { "▾" } else { "▸" };
    let toggle = button(
        text(format!("{arrow} Run an eval"))
            .size(13)
            .font(theme::BOLD),
    )
    .style(theme::link)
    .on_press(msg(Msg::ToggleForm));
    if !form.open {
        return container(toggle)
            .padding(8)
            .width(Fill)
            .style(theme::panel)
            .into();
    }
    let spaces: Vec<String> = form.workspaces.iter().map(|w| w.name.clone()).collect();
    let chosen_space = form.workspace.clone().or_else(|| spaces.first().cloned());
    let space =
        pick_list(spaces, chosen_space, move |w| msg(Msg::Workspace(w))).style(theme::select);
    let chosen_target = form
        .target
        .clone()
        .or_else(|| form.targets.first().cloned());
    let target = pick_list(form.targets.clone(), chosen_target, move |t| {
        msg(Msg::Target(t))
    })
    .style(theme::select);
    let name = text_input("", &form.name)
        .on_input(move |n| msg(Msg::Name(n)))
        .style(theme::field);
    let requests = text_editor(&form.requests)
        .on_action(move |a| msg(Msg::Requests(a)))
        .height(96);
    let run = button(text("Run it").size(13))
        .padding([6, 12])
        .style(theme::secondary)
        .on_press(msg(Msg::Run));
    let cost = "Each request runs once under A and once under B: two plan and two build turns per request, plus a grade.";
    let body = column![
        toggle,
        row![labeled("Name", name), labeled("Workspace", space)].spacing(12),
        labeled("Requests, one per line", requests),
        row![
            labeled("The constraint A and B set", target),
            labeled("A at", rung_pick(Rung::A, &form.rung_a)),
            labeled("B at", rung_pick(Rung::B, &form.rung_b))
        ]
        .spacing(12),
        row![run, text(cost).size(12).color(theme::MUTED)]
            .spacing(12)
            .align_y(iced::Center),
        text(&form.error).size(13).color(theme::BAD),
    ];
    container(body.spacing(12))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

fn summary_row<'a>(e: &EvalSummary) -> Element<'a, Message> {
    let about = column![
        text(e.name.clone()).font(theme::BOLD),
        text(format!(
            "{} · {} · {} · {}",
            e.id,
            e.workspace,
            short(&e.base),
            ago(e.created_at)
        ))
        .size(12)
        .color(theme::MUTED)
    ];
    let progress = text(format!("{} of {} runs done", e.done, e.runs))
        .size(13)
        .color(theme::MUTED);
    let line = row![about.width(Fill), progress].align_y(iced::Center);
    button(line)
        .width(Fill)
        .padding([8, 12])
        .style(theme::row_button)
        .on_press(Message::Go(Route::Eval(e.id.clone())))
        .into()
}

pub fn view_list(list: &List) -> Element<'_, Message> {
    let lede = "An eval runs the same requests from the same commit under two harness configurations, A and B, that differ in one rung. Runs approve their own plans and stop at review, so nothing lands.";
    let rows: Element<'_, Message> = if list.evals.is_empty() {
        container(text("No evals yet.").size(13).color(theme::MUTED))
            .padding([8, 12])
            .into()
    } else {
        Column::with_children(list.evals.iter().map(summary_row)).into()
    };
    let body = column![
        text("Evals").size(24).font(theme::BOLD),
        text(lede).size(13).color(theme::MUTED),
        text(&list.error).color(theme::BAD),
        form_view(&list.form),
        section("Evals".into(), rows, false),
    ];
    scrolled(body.spacing(12))
}

/// A run's measures, by name.
fn measures(r: &RunView) -> [(&'static str, String); 8] {
    let outcome = match (r.job.is_empty(), r.reviewed) {
        (true, _) => String::new(),
        (false, true) => "reached review".into(),
        (false, false) => r.state.clone(),
    };
    [
        ("Outcome", outcome),
        ("Refused", r.refused.to_string()),
        ("Held", r.held.to_string()),
        ("Flagged", r.flagged.to_string()),
        ("Calls", r.passed.to_string()),
        ("Turns", r.turns.to_string()),
        ("Cost", format!("${:.4}", r.cost_micros as f64 / 1e6)),
        ("Grade", r.grade.map_or("-".into(), |g| g.to_string())),
    ]
}

/// One arm's row: each measure, in the warning color where the other arm differs.
fn arm_row<'a>(
    arm: &'a str,
    mine: Option<&RunView>,
    other: Option<&RunView>,
) -> Element<'a, Message> {
    let label = text(arm).font(theme::BOLD).width(40);
    let Some(mine) = mine.map(measures) else {
        return row![label, text("not started").color(theme::MUTED)].into();
    };
    let theirs = other.map(measures);
    let cells = mine.into_iter().enumerate().map(|(i, (_, v))| {
        let differs = theirs.as_ref().is_some_and(|t| t[i].1 != v);
        let cell = text(v).size(13).width(Fill);
        if differs {
            cell.color(theme::WARN).font(theme::BOLD).into()
        } else {
            cell.into()
        }
    });
    Row::with_children(std::iter::once(label.into()).chain(cells)).into()
}

/// A against B, for one request or in total.
fn table<'a>(label: String, a: Option<&RunView>, b: Option<&RunView>) -> Element<'a, Message> {
    let names = measures(&RunView {
        job: "x".into(),
        ..RunView::default()
    })
    .map(|(n, _)| text(n).size(12).color(theme::MUTED).width(Fill).into());
    let head = Row::with_children(std::iter::once(text("").width(40).into()).chain(names));
    column![
        text(label).size(13).font(theme::BOLD),
        head,
        arm_row("A", a, b),
        arm_row("B", b, a)
    ]
    .spacing(4)
    .into()
}

/// A run's refused calls, and a link to its job.
fn refusals<'a>(arm: &str, run: Option<&RunView>, server: &str) -> Option<Element<'a, Message>> {
    let r = run.filter(|r| !r.refused_calls.is_empty() || !r.job.is_empty())?;
    let open = Message::OpenJob {
        server: server.into(),
        id: r.job.clone(),
    };
    let link = button(text(format!("job {}", r.job)).size(12).color(theme::LINK))
        .padding(0)
        .style(theme::link)
        .on_press(open);
    let head = row![
        text(format!("{arm}: {} refused · ", r.refused_calls.len()))
            .size(12)
            .font(theme::BOLD),
        link
    ];
    let calls = r
        .refused_calls
        .iter()
        .map(|c| text(format!("• {c}")).size(12).into());
    Some(
        column![head, Column::with_children(calls)]
            .spacing(2)
            .width(Fill)
            .into(),
    )
}

fn request_row<'a>(r: &EvalRow, server: &str) -> Element<'a, Message> {
    let differs = if r.differences.is_empty() {
        "No difference.".to_string()
    } else {
        format!("Differs: {}", r.differences.join("; "))
    };
    let refused = row![]
        .spacing(12)
        .extend(refusals("A", r.a.as_ref(), server))
        .extend(refusals("B", r.b.as_ref(), server));
    let body = column![
        table(
            format!("{}. {}", r.idx + 1, r.request),
            r.a.as_ref(),
            r.b.as_ref()
        ),
        text(differs).size(12).color(theme::MUTED),
        refused
    ];
    container(body.spacing(8)).padding(12).into()
}

fn totals<'a>(one: &'a One, v: &'a EvalView) -> Element<'a, Message> {
    let done = v.summary.done == v.summary.runs;
    let differs = if v.differences.is_empty() {
        "No difference in total.".to_string()
    } else {
        format!("B differs from A: {}", v.differences.join("; "))
    };
    let arm = |label: &'static str, which: &'static str| {
        let b = button(text(label).size(13))
            .padding([6, 12])
            .style(theme::secondary);
        b.on_press_maybe(done.then_some(Message::Evals(Msg::Apply(which))))
    };
    let hint = if done {
        "Sets the live policy to that arm's rungs."
    } else {
        "Wait for every run to finish."
    };
    let body = column![
        table("In total".into(), Some(&v.total_a), Some(&v.total_b)),
        text(differs).size(13),
        row![
            arm("Keep A", "a"),
            arm("Apply B", "b"),
            text(hint).size(12).color(theme::MUTED)
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(&one.note).size(13),
    ];
    container(body.spacing(10))
        .padding(12)
        .width(Fill)
        .style(theme::panel)
        .into()
}

pub fn view_one<'a>(one: &'a One, ctx: &Ctx) -> Element<'a, Message> {
    let back = button(text("← All evals").size(13))
        .style(theme::link)
        .on_press(Message::Go(Route::Evals));
    let mut body = column![back, text(&one.error).color(theme::BAD)].spacing(12);
    if let Some(v) = &one.eval {
        let s = &v.summary;
        let facts = format!(
            "{} · {} from {} · {} of {} runs done",
            s.id,
            s.workspace,
            short(&s.base),
            s.done,
            s.runs
        );
        let arms = column![
            row![text("A: ").font(theme::BOLD), text(v.arm_a.join(", "))],
            row![text("B: ").font(theme::BOLD), text(v.arm_b.join(", "))]
        ];
        let rows = v.rows.iter().map(|r| request_row(r, &ctx.current));
        body = body
            .push(text(s.name.clone()).size(24).font(theme::BOLD))
            .push(text(facts).size(13).color(theme::MUTED))
            .push(
                container(arms.spacing(4))
                    .padding(12)
                    .width(Fill)
                    .style(theme::panel),
            )
            .push(totals(one, v))
            .push(
                container(Column::with_children(rows))
                    .width(Fill)
                    .style(theme::panel),
            );
    }
    scrolled(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_eval_takes_the_first_choices_and_skips_blank_requests() {
        let form = Form {
            workspaces: vec![Workspace {
                name: "calc".into(),
                path: "/c".into(),
                branch: "main".into(),
                checks: String::new(),
            }],
            targets: vec![Choice::new("constraint:no-curl", "no-curl (told)")],
            requests: text_editor::Content::with_text("Add add.\n\n  Add sub.  "),
            ..Form::default()
        };
        let e = form.eval();
        assert_eq!(e.workspace, "calc");
        assert_eq!(e.target, "constraint:no-curl");
        assert_eq!(e.requests, ["Add add.", "Add sub."]);
        assert_eq!((e.rung_a.as_str(), e.rung_b.as_str()), ("told", "enforced"));
    }

    #[test]
    fn a_run_reads_as_its_measures() {
        let run = RunView {
            job: "j1".into(),
            reviewed: true,
            cost_micros: 12_345,
            grade: Some(4),
            ..RunView::default()
        };
        let m = measures(&run);
        assert_eq!(
            (m[0].1.as_str(), m[6].1.as_str(), m[7].1.as_str()),
            ("reached review", "$0.0123", "4")
        );
        assert_eq!(measures(&RunView::default())[0].1, "");
    }
}
