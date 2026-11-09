//! The Jobs page (PLAN.md A3): every job on every Bastion server, the ones
//! waiting on the owner first, after wipp's Jobs page and GitHub Actions'
//! runs list.

use bastion_protocol::{JobSummary, command};
use iced::widget::{Column, button, column, container, pick_list, row, text, text_input};
use iced::{Element, Fill, Task};

use super::{Ctx, ago, labeled, mark, section};
use crate::app::Message;
use crate::client::Ask;
use crate::route::Route;
use crate::theme;

const IN_PROGRESS: [&str; 3] = ["queued", "provisioning", "running"];
const FINISHED: [&str; 3] = ["landed", "failed", "stopped"];
const EVERY: &str = "Every workspace";

/// A job, and the Bastion server it runs on (D30).
#[derive(Clone, Debug)]
pub struct ServerJob {
    server: String,
    job: JobSummary,
}

#[derive(Default)]
pub struct State {
    jobs: Vec<ServerJob>,
    errors: Vec<String>,
    place: Option<String>,
    search: String,
    show_finished: bool,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read((Vec<ServerJob>, Vec<String>)),
    Place(String),
    Search(String),
    ToggleFinished,
}

/// Read every server's jobs again. A server that cannot be read keeps the
/// jobs last read from it, so they stay listed.
pub fn load(state: &State, ctx: &Ctx) -> Task<Msg> {
    Task::perform(read_all(ctx.clone(), state.jobs.clone()), Msg::Read)
}

async fn read_all(ctx: Ctx, before: Vec<ServerJob>) -> (Vec<ServerJob>, Vec<String>) {
    let (mut all, mut errors) = (Vec::new(), Vec::new());
    for s in &ctx.servers {
        let read = ctx
            .clone()
            .get::<Vec<JobSummary>>(s.name.clone(), Ask::bare(command::JOBS))
            .await;
        match read {
            Ok(jobs) => all.extend(jobs.into_iter().map(|job| ServerJob {
                server: s.name.clone(),
                job,
            })),
            Err(e) => {
                errors.push(format!("{}: {e}", s.name));
                all.extend(before.iter().filter(|j| j.server == s.name).cloned());
            }
        }
    }
    all.sort_by_key(|j| std::cmp::Reverse(j.job.created_at));
    (all, errors)
}

pub fn update(state: &mut State, msg: Msg) {
    match msg {
        Msg::Read((jobs, errors)) => (state.jobs, state.errors) = (jobs, errors),
        Msg::Place(p) => state.place = (p != EVERY).then_some(p),
        Msg::Search(s) => state.search = s,
        Msg::ToggleFinished => state.show_finished = !state.show_finished,
    }
}

/// Each job followed by its parts, so a split shows under its job.
fn families(mut jobs: Vec<ServerJob>) -> Vec<ServerJob> {
    let head = |j: &ServerJob| j.job.parent.clone().unwrap_or_else(|| j.job.id.clone());
    let mut order: Vec<String> = Vec::new();
    for j in &jobs {
        if !order.contains(&head(j)) {
            order.push(head(j));
        }
    }
    jobs.sort_by_key(|j| {
        let family = order.iter().position(|h| *h == head(j));
        (family, j.job.parent.is_some(), j.job.id.clone())
    });
    jobs
}

/// What the page says about servers it cannot read: unreachable ones first,
/// then why a reachable one's jobs did not read.
fn page_note(down: &[String], errors: &[String]) -> String {
    let failed = errors
        .iter()
        .filter(|e| !down.iter().any(|d| e.starts_with(&format!("{d}: "))));
    let mut lines: Vec<String> = vec![unreachable_note(down)];
    lines.extend(failed.cloned());
    lines.retain(|l| !l.is_empty());
    lines.join(" ")
}

fn unreachable_note(down: &[String]) -> String {
    match down {
        [] => String::new(),
        [one] => {
            format!("The Bastion server {one} cannot be reached. Its jobs show what was last read.")
        }
        many => format!(
            "The Bastion servers {} cannot be reached. Their jobs show what was last read.",
            many.join(", ")
        ),
    }
}

/// A job's Bastion server and workspace, as the filter names them.
fn place(j: &ServerJob) -> String {
    format!("{} / {}", j.server, j.job.workspace)
}

impl State {
    /// The jobs the filter and the search let through.
    fn shown(&self) -> Vec<ServerJob> {
        let words = self.search.to_lowercase();
        let in_place = |j: &&ServerJob| self.place.as_ref().is_none_or(|p| place(j) == *p);
        let matches = |j: &&ServerJob| {
            words.is_empty()
                || j.job.request.to_lowercase().contains(&words)
                || j.job.id.contains(&words)
        };
        self.jobs
            .iter()
            .filter(in_place)
            .filter(matches)
            .cloned()
            .collect()
    }

    fn places(&self) -> Vec<String> {
        let mut all: Vec<String> = self.jobs.iter().map(place).collect();
        all.sort();
        all.dedup();
        all.insert(0, EVERY.into());
        all
    }
}

fn line(job: &JobSummary, server: &str, down: bool) -> String {
    let unreachable = if down { " · unreachable" } else { "" };
    let by = job
        .requester
        .as_ref()
        .map_or_else(String::new, |who| format!(" · by {who}"));
    let state = job.state.replace('_', " ");
    format!(
        "{server}{unreachable} · {} · {} · {state} · {}{by}",
        job.workspace,
        job.id,
        ago(job.created_at)
    )
}

fn job_row<'a>(sj: ServerJob, down: bool) -> Element<'a, Message> {
    let (symbol, color) = mark(&sj.job.state);
    let mut about = column![text(sj.job.request.clone()).font(theme::BOLD)].spacing(2);
    if let Some(p) = &sj.job.part {
        about = about.push(text(format!("Part: {p}")).size(13));
    }
    about = about.push(
        text(line(&sj.job, &sj.server, down))
            .size(12)
            .color(theme::MUTED),
    );
    let stage = container(text(sj.job.stage.clone()).size(12))
        .padding([2, 8])
        .style(theme::chip);
    let body = row![
        text(symbol).size(18).color(color).width(24),
        about.width(Fill),
        stage
    ]
    .spacing(12);
    let indent = if sj.job.parent.is_some() { 24 } else { 0 };
    let open = Message::OpenJob {
        server: sj.server,
        id: sj.job.id,
    };
    let row_button = button(body)
        .width(Fill)
        .padding([10, 12])
        .style(theme::row_button)
        .on_press(open);
    let faded = if down { 0.6 } else { 1.0 };
    container(row_button)
        .padding(iced::Padding::ZERO.left(indent))
        .style(move |_| container::Style {
            text_color: Some(iced::Color {
                a: faded,
                ..theme::INK
            }),
            ..container::Style::default()
        })
        .into()
}

fn group<'a>(title: String, jobs: Vec<ServerJob>, ctx: &Ctx, accent: bool) -> Element<'a, Message> {
    let body: Element<'a, Message> = if jobs.is_empty() {
        container(text("Nothing here.").size(13).color(theme::MUTED))
            .padding([10, 12])
            .into()
    } else {
        let rows = families(jobs).into_iter().map(|j| {
            let down = ctx.down.contains(&j.server);
            job_row(j, down)
        });
        Column::with_children(rows).into()
    };
    section(title, body, accent)
}

fn filters(state: &State) -> Element<'_, Message> {
    let shown = Some(state.place.clone().unwrap_or_else(|| EVERY.into()));
    let place =
        pick_list(state.places(), shown, |p| Message::Jobs(Msg::Place(p))).style(theme::select);
    let search = text_input("Request or id", &state.search)
        .on_input(|s| Message::Jobs(Msg::Search(s)))
        .style(theme::field)
        .width(260);
    row![labeled("Workspace", place), labeled("Search", search)]
        .spacing(12)
        .into()
}

fn groups<'a>(state: &State, ctx: &Ctx) -> Element<'a, Message> {
    let all = state.shown();
    let pick = |states: &[&str]| -> Vec<ServerJob> {
        all.iter()
            .filter(|j| states.contains(&j.job.state.as_str()))
            .cloned()
            .collect()
    };
    let (waiting, active, done) = (pick(&["needs_you"]), pick(&IN_PROGRESS), pick(&FINISHED));
    let mut list = Column::new().spacing(16);
    if !waiting.is_empty() {
        list = list.push(group(
            format!("Waiting for you ({})", waiting.len()),
            waiting,
            ctx,
            true,
        ));
    }
    list = list.push(group(
        format!("In progress ({})", active.len()),
        active,
        ctx,
        false,
    ));
    let arrow = if state.show_finished { "▾" } else { "▸" };
    let toggle = button(text(format!("{arrow} Finished ({})", done.len())).size(13))
        .style(theme::link)
        .on_press(Message::Jobs(Msg::ToggleFinished));
    list = list.push(toggle);
    if state.show_finished {
        list = list.push(group(
            format!("Finished ({})", done.len()),
            done,
            ctx,
            false,
        ));
    }
    list.into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let new = button("New request")
        .padding([8, 12])
        .style(theme::primary)
        .on_press(Message::Go(Route::NewRequest));
    let head = row![text("Jobs").size(24).font(theme::BOLD).width(Fill), new].align_y(iced::Center);
    let down: Vec<String> = ctx.down.iter().cloned().collect();
    let note = text(page_note(&down, &state.errors)).color(theme::BAD);
    let body = column![head, note, filters(state), groups(state, ctx)].spacing(16);
    super::scrolled(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_names_each_server_it_cannot_reach() {
        assert_eq!(unreachable_note(&[]), "");
        assert!(unreachable_note(&["Two".into()]).starts_with("The Bastion server Two cannot"));
        let both = unreachable_note(&["Two".into(), "box".into()]);
        assert!(
            both.starts_with("The Bastion servers Two, box cannot"),
            "{both}"
        );
    }

    #[test]
    fn a_reachable_server_that_fails_to_read_still_says_why() {
        let errors = vec![
            "local: no database".to_string(),
            "Two: cannot be reached".to_string(),
        ];
        let note = page_note(&["Two".into()], &errors);
        assert!(note.contains("Two cannot be reached"), "{note}");
        assert!(note.contains("local: no database"), "{note}");
        assert!(!note.contains("Two: cannot"), "{note}");
    }

    fn job(id: &str, parent: Option<&str>, workspace: &str) -> ServerJob {
        ServerJob {
            server: "local".into(),
            job: JobSummary {
                id: id.into(),
                workspace: workspace.into(),
                request: format!("request {id}"),
                stage: "plan".into(),
                state: "queued".into(),
                question: None,
                error: None,
                created_at: 0,
                updated_at: 0,
                actions: Vec::new(),
                pr: None,
                issue: None,
                requester: None,
                parent: parent.map(String::from),
                part: None,
            },
        }
    }

    #[test]
    fn a_split_job_lists_its_parts_under_it() {
        let listed = vec![
            job("j1-3", Some("j1"), "w"),
            job("j2", None, "w"),
            job("j1", None, "w"),
            job("j1-2", Some("j1"), "w"),
        ];
        let ids: Vec<String> = families(listed).into_iter().map(|j| j.job.id).collect();
        assert_eq!(ids, ["j1", "j1-2", "j1-3", "j2"]);
    }

    #[test]
    fn the_filter_and_the_search_narrow_the_list() {
        let mut state = State {
            jobs: vec![job("j1", None, "a"), job("j2", None, "b")],
            ..State::default()
        };
        assert_eq!(state.places(), [EVERY, "local / a", "local / b"]);
        update(&mut state, Msg::Place("local / b".into()));
        assert_eq!(state.shown().len(), 1);
        update(&mut state, Msg::Place(EVERY.into()));
        update(&mut state, Msg::Search("REQUEST J1".into()));
        assert_eq!(state.shown()[0].job.id, "j1");
    }
}
