//! The Jobs page: the jobs on the server the switcher shows, or on every
//! server, the ones waiting on the owner first, after wipp's Jobs page and
//! GitHub Actions' runs list.

use bastion_protocol::{JobSummary, api};
use iced::widget::{Column, column, container, pick_list, row, text, text_input};
use iced::{Element, Fill, Task};

use super::chart::{self, Bars};
use super::{Ctx, ago, labeled, section};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

const FINISHED: [&str; 3] = ["landed", "failed", "stopped"];
const EVERY: &str = "Every region";

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
impl State {
    /// Whether any job here is in progress.
    pub fn moving(&self) -> bool {
        let busy = |sj: &ServerJob| super::IN_PROGRESS.contains(&sj.job.state.as_str());
        self.jobs.iter().any(busy)
    }
}

pub fn load(state: &State, ctx: &Ctx) -> Task<Msg> {
    Task::perform(read_all(ctx.clone(), state.jobs.clone()), Msg::Read)
}

async fn read_all(ctx: Ctx, before: Vec<ServerJob>) -> (Vec<ServerJob>, Vec<String>) {
    let (mut all, mut errors) = (Vec::new(), Vec::new());
    for s in &ctx.servers {
        let read = ctx.clone().get(s.name.clone(), api::Jobs).await;
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

impl State {
    /// The workspace picked, while the servers shown have it.
    fn place(&self, scope: Option<&str>) -> Option<&String> {
        let have = self.places(scope);
        self.place.as_ref().filter(|p| have.contains(p))
    }

    /// The jobs the switcher, the filter and the search let through.
    fn shown(&self, scope: Option<&str>) -> Vec<ServerJob> {
        let words = self.search.to_lowercase();
        let place = self.place(scope);
        let in_place =
            |j: &&ServerJob| on_server(scope, j) && place.is_none_or(|p| j.job.workspace == *p);
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

    /// The workspaces with jobs on the servers shown.
    fn places(&self, scope: Option<&str>) -> Vec<String> {
        let on = self.jobs.iter().filter(|j| on_server(scope, j));
        let mut all: Vec<String> = on.map(|j| j.job.workspace.clone()).collect();
        all.sort();
        all.dedup();
        all.insert(0, EVERY.into());
        all
    }
}

/// The one server the switcher shows, or none for every server.
fn scope(ctx: &Ctx) -> Option<&str> {
    (!ctx.all).then_some(ctx.current.as_str())
}

fn on_server(scope: Option<&str>, j: &ServerJob) -> bool {
    scope.is_none_or(|s| j.server == s)
}

fn line(job: &JobSummary, server: &str, down: bool) -> String {
    let unreachable = if down { " · unreachable" } else { "" };
    let by = job
        .requester
        .as_ref()
        .map_or_else(String::new, |who| format!(" · by {who}"));
    let state = super::journey_word(&job.state);
    format!(
        "{server}{unreachable} · {} · {} · {state} · {}{by}",
        job.workspace,
        job.id,
        ago(job.created_at)
    )
}

/// The most of a request a row shows; the job's page shows all of it.
const TITLE_CHARS: usize = 120;

/// A request as a row's title: its first line, cut at a word to
/// [`TITLE_CHARS`], with an ellipsis when anything was cut.
pub(super) fn title(request: &str) -> String {
    let first = request.trim().lines().next().unwrap_or_default();
    let cut = first.char_indices().nth(TITLE_CHARS).map(|(i, _)| i);
    match cut {
        None if first.len() == request.trim().len() => first.to_string(),
        None => format!("{first} …"),
        Some(i) => {
            let at_word = first[..i].rfind(' ').unwrap_or(i);
            format!("{}…", first[..at_word].trim_end())
        }
    }
}

fn job_row<'a>(sj: ServerJob, down: bool) -> Element<'a, Message> {
    let about = column![text(title(&sj.job.request)).size(15).font(theme::MEDIUM)].spacing(3);
    let about = about.push(
        text(line(&sj.job, &sj.server, down))
            .size(12)
            .color(theme::muted()),
    );
    let at = sj
        .job
        .node
        .as_deref()
        .unwrap_or("the summit")
        .replace('_', " ");
    let stage = container(text(at).size(12))
        .padding([2, 8])
        .style(theme::chip);
    let body = row![
        container(super::mark_view(&sj.job.state, 18.0)).width(24),
        about.width(Fill),
        stage
    ]
    .spacing(12);
    let open = Message::OpenJob {
        server: sj.server,
        id: sj.job.id,
    };
    let row_button = crate::lift::button(body)
        .width(Fill)
        .padding([10, 12])
        .style(theme::card_row)
        .on_press(open);
    let faded = if down { 0.6 } else { 1.0 };
    container(row_button)
        .style(move |_| container::Style {
            text_color: Some(iced::Color {
                a: faded,
                ..theme::ink()
            }),
            ..container::Style::default()
        })
        .into()
}

fn group<'a>(title: String, jobs: Vec<ServerJob>, ctx: &Ctx, accent: bool) -> Element<'a, Message> {
    let body: Element<'a, Message> = if jobs.is_empty() {
        container(text("Nothing here.").size(13).color(theme::muted()))
            .padding([10, 12])
            .into()
    } else {
        let rows = jobs.into_iter().map(|j| {
            let down = ctx.down.contains(&j.server);
            job_row(j, down)
        });
        // Inset, so a hovered row leaves the card's edge in view.
        Column::with_children(rows).spacing(2).padding(6).into()
    };
    section(title, body, accent)
}

/// How a job ended, or that it has not, for the chart.
const ENDS: [(&str, theme::Tint); 4] = [
    ("at the summit", theme::ok),
    ("marsh: failed", theme::bad),
    ("marsh: stopped", theme::muted),
    ("under way", theme::primary_color),
];

fn end_of(state: &str) -> &'static str {
    match state {
        "landed" => "at the summit",
        "failed" => "marsh: failed",
        "stopped" => "marsh: stopped",
        _ => "under way",
    }
}

/// The jobs started each day over the last 30 days, split by how they ended.
fn started(jobs: &[ServerJob], now: u64) -> Bars {
    let columns = chart::last_days(now, 30);
    let values = columns
        .iter()
        .map(|day| {
            let on_day: Vec<&ServerJob> = jobs
                .iter()
                .filter(|j| chart::day(j.job.created_at) == *day)
                .collect();
            let ended = |end: &str| {
                on_day
                    .iter()
                    .filter(|j| end_of(&j.job.state) == end)
                    .count()
            };
            ENDS.iter().map(|(end, _)| ended(end) as f64).collect()
        })
        .collect();
    Bars {
        columns,
        series: ENDS.iter().map(|(e, c)| (e.to_string(), c())).collect(),
        values,
        unit: |n| format!("{n}"),
    }
}

/// The counts that matter at a glance, and the jobs started by day.
fn overview<'a>(jobs: &[ServerJob]) -> Element<'a, Message> {
    let count = |states: &[&str]| {
        let n = jobs
            .iter()
            .filter(|j| states.contains(&j.job.state.as_str()));
        n.count().to_string()
    };
    let tiles = row![
        chart::tile(count(&["needs_you"]), "waiting for you", theme::warn()),
        chart::tile(count(&super::IN_PROGRESS), "under way", theme::link_color()),
        chart::tile(count(&["landed"]), "at the summit", theme::ok()),
        chart::tile(count(&["failed"]), "in the marsh", theme::bad()),
    ]
    .spacing(12);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let chart = chart::view("Journeys set out by day, last 30 days", started(jobs, now));
    column![tiles, chart].spacing(12).into()
}

fn filters<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let scope = scope(ctx);
    let shown = Some(state.place(scope).cloned().unwrap_or_else(|| EVERY.into()));
    let place = pick_list(state.places(scope), shown, |p| Message::Jobs(Msg::Place(p)))
        .style(theme::select);
    let search = text_input("Request or id", &state.search)
        .on_input(|s| Message::Jobs(Msg::Search(s)))
        .style(theme::field)
        .width(260);
    row![labeled("Region", place), labeled("Search", search)]
        .spacing(12)
        .into()
}

fn groups<'a>(state: &State, ctx: &Ctx) -> Element<'a, Message> {
    let all = state.shown(scope(ctx));
    let pick = |states: &[&str]| -> Vec<ServerJob> {
        all.iter()
            .filter(|j| states.contains(&j.job.state.as_str()))
            .cloned()
            .collect()
    };
    let (waiting, active, done) = (
        pick(&["needs_you"]),
        pick(&super::IN_PROGRESS),
        pick(&FINISHED),
    );
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
        format!("Under way ({})", active.len()),
        active,
        ctx,
        false,
    ));
    let arrow = if state.show_finished { "▾" } else { "▸" };
    let toggle = crate::lift::button(text(format!("{arrow} Home again ({})", done.len())).size(13))
        .style(theme::link)
        .on_press(Message::Jobs(Msg::ToggleFinished));
    list = list.push(toggle);
    if state.show_finished {
        list = list.push(group(
            format!("Home again ({})", done.len()),
            done,
            ctx,
            false,
        ));
    }
    list.into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let new = crate::lift::button("New journey")
        .padding([8, 12])
        .style(theme::primary)
        .on_press(Message::Go(Route::NewRequest));
    let about = format!(
        "The journeys on {}, the ones waiting for you first.",
        super::on(ctx)
    );
    let down: Vec<String> = ctx.down.iter().cloned().collect();
    let shown = state.shown(scope(ctx));
    let body = column![super::head("Journeys", about, vec![new.into()])]
        .extend(super::problem(&page_note(&down, &state.errors)))
        .extend([filters(state, ctx), overview(&shown), groups(state, ctx)])
        .spacing(16);
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

    fn job(id: &str, workspace: &str) -> ServerJob {
        ServerJob {
            server: "local".into(),
            job: JobSummary {
                id: id.into(),
                workspace: workspace.into(),
                request: format!("request {id}"),
                state: "queued".into(),
                ..JobSummary::default()
            },
        }
    }

    #[test]
    fn the_filter_and_the_search_narrow_the_list() {
        let mut state = State {
            jobs: vec![job("j1", "a"), job("j2", "b")],
            ..State::default()
        };
        state.jobs.push(ServerJob {
            server: "team".into(),
            ..job("j3", "c")
        });
        assert_eq!(state.places(None), [EVERY, "a", "b", "c"]);
        update(&mut state, Msg::Place("c".into()));
        let local = Some("local");
        assert_eq!(state.place(local), None, "team's workspace is not on local");
        assert_eq!(state.places(local), [EVERY, "a", "b"]);
        assert_eq!(state.shown(local).len(), 2);
        assert_eq!(state.shown(None).len(), 1, "c again, across servers");
        update(&mut state, Msg::Place("b".into()));
        assert_eq!(state.shown(local).len(), 1);
        update(&mut state, Msg::Place(EVERY.into()));
        update(&mut state, Msg::Search("REQUEST J1".into()));
        assert_eq!(state.shown(None)[0].job.id, "j1");
    }

    #[test]
    fn the_chart_counts_each_days_jobs_by_how_they_ended() {
        let at = |id: &str, state: &str, created_at| ServerJob {
            server: "local".into(),
            job: JobSummary {
                id: id.into(),
                state: state.into(),
                created_at,
                ..JobSummary::default()
            },
        };
        let today = 951_782_400;
        let jobs = [
            at("a", "landed", today),
            at("b", "needs_you", today + 60),
            at("c", "failed", today - 86_400),
        ];
        let bars = started(&jobs, today);
        assert_eq!(bars.columns.last().unwrap(), "2000-02-29");
        assert_eq!(
            bars.values[29],
            [1.0, 0.0, 0.0, 1.0],
            "landed, failed, stopped, open"
        );
        assert_eq!(bars.values[28], [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_long_request_is_cut_at_a_word_for_its_row() {
        assert_eq!(
            title("Add shout(x) to calc.py."),
            "Add shout(x) to calc.py."
        );
        assert_eq!(title("First line\nmore detail"), "First line …");
        let long = "word ".repeat(40);
        let shown = title(&long);
        assert!(shown.ends_with("word…"), "{shown}");
        assert!(shown.chars().count() <= TITLE_CHARS + 1);
    }
}
