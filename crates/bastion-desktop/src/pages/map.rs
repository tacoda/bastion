//! The Map (L5): each server drawn as a chart. Its workspaces are harbor
//! towns along the coast, where requests come ashore; its workflows are
//! routes across the land, each step a landmark, each route ending at its
//! summit. A badge on a landmark counts the rules that apply there.
//! Pressing a landmark opens a card over the chart, beside it: what the
//! step does (what it runs, who decides, its model, its instructions, the
//! results it gives) and what applies there. On All servers each server
//! has its chart.

use std::collections::BTreeMap;

use bastion_protocol::api;
use bastion_protocol::{FieldNoteView, JobSummary, NodeView, WorkflowView};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::widget::{Column, column, container, pin, row, scrollable, stack, text};
use iced::{Element, Fill, Point, Rectangle, Renderer, Size, Task, Theme, mouse};

use super::step_rules::{self, At, Rules};
use super::{Ctx, carto, scrolled, wear};
use crate::app::Message;
use crate::route::Route;
use crate::theme;

/// Where the land starts, past the sea of requests.
const SHORE: f32 = 70.0;
/// The column of harbor towns, and where the routes begin.
const TOWNS: f32 = SHORE + 60.0;
const WEST: f32 = SHORE + 230.0;
const EAST: f32 = 150.0;
const TOP: f32 = 120.0;
const ROW: f32 = 120.0;
const REACH: f32 = 22.0;

/// One step's place on the chart: `key` is `workflow/step`.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub key: String,
    pub stage: String,
    pub at: Point,
}

/// The workflows in name order, each with its steps in canvas order.
fn routes(rules: &Rules) -> Vec<(&WorkflowView, Vec<&NodeView>)> {
    let mut flows: Vec<&WorkflowView> = rules.workflows().iter().collect();
    flows.sort_by(|a, b| a.name.cmp(&b.name));
    flows
        .into_iter()
        .map(|w| {
            let mut steps: Vec<&NodeView> = w.nodes.iter().collect();
            steps.sort_by_key(|n| (n.x, n.y));
            (w, steps)
        })
        .collect()
}

/// The foot of each step's landmark on a chart `width` wide.
fn route_places(rules: &Rules, width: f32) -> Vec<Place> {
    let span = (width - WEST - EAST).max(1.0);
    let rows = routes(rules).into_iter().enumerate();
    rows.flat_map(|(row, (w, steps))| {
        let n = steps.len().max(1) as f32;
        let base = TOP + row as f32 * ROW + 60.0;
        steps.into_iter().enumerate().map(move |(i, s)| Place {
            key: format!("{}/{}", w.id, s.id),
            stage: s.stage.clone(),
            at: Point::new(
                WEST + (i as f32 + 0.5) / n * span,
                base - (i as f32 / n) * 30.0 + (i as f32 * 2.1).sin() * 6.0,
            ),
        })
    })
    .collect()
}

/// The prefix a harbor town's key carries: `town:<workspace>`.
const TOWN: &str = "town:";

/// Each workspace's harbor town, at the foot of its drawing.
fn town_places(rules: &Rules) -> impl Iterator<Item = Place> + '_ {
    rules.charters().iter().enumerate().map(|(i, c)| Place {
        key: format!("{TOWN}{}", c.workspace),
        stage: "town".into(),
        at: Point::new(TOWNS, TOP + i as f32 * ROW + 50.0),
    })
}

/// Every place a person can press: each step's landmark and each town.
pub fn places(rules: &Rules, width: f32) -> Vec<Place> {
    let mut all = route_places(rules, width);
    all.extend(town_places(rules));
    all
}

/// How tall the chart is: room for every route and every town.
/// Room along the foot of the chart for its legend.
const LEGEND: f32 = 70.0;

fn height(rules: &Rules) -> f32 {
    let rows = rules.workflows().len().max(rules.charters().len()).max(1);
    TOP + rows as f32 * ROW + 40.0 + LEGEND
}

/// The landmark under `p`, if one is near enough to press.
fn under(places: &[Place], p: Point) -> Option<&Place> {
    let middle = |m: &Place| Point::new(m.at.x, m.at.y - 16.0);
    places.iter().find(|m| middle(m).distance(p) <= REACH)
}

/// A landmark pressed: its `workflow/step`, its kind, where it stands, and
/// how wide the chart was, so the card can sit beside it.
#[derive(Clone, Debug)]
pub struct Picked {
    key: String,
    stage: String,
    at: Point,
    width: f32,
}

/// The card's width, and its tallest before it scrolls.
const CARD: f32 = 440.0;
const CARD_MAX: f32 = 340.0;

/// Where the card goes: right of the landmark, or left when that would
/// run off the chart; never past the chart's foot.
fn card_at(p: &Picked, height: f32) -> Point {
    let right = p.at.x + 34.0;
    let x = if right + CARD > p.width - 16.0 {
        p.at.x - 34.0 - CARD
    } else {
        right
    };
    let y = (p.at.y - 60.0).clamp(16.0, (height - CARD_MAX - 16.0).max(16.0));
    Point::new(x.max(16.0), y)
}

/// One server's chart. A press publishes the landmark picked.
struct Chart<'a> {
    rules: &'a Rules,
    picked: Option<&'a str>,
    /// The server's recent journeys, for each trail's wear.
    journeys: &'a [JobSummary],
}

/// A count of the rules on at a landmark, as a red seal.
fn badge(frame: &mut Frame, at: Point, n: usize) {
    seal(frame, Point::new(at.x + 16.0, at.y - 40.0), n, theme::bad());
}

/// A count in a round seal of `color`; nothing for none.
fn seal(frame: &mut Frame, seal: Point, n: usize, color: iced::Color) {
    if n == 0 {
        return;
    }
    frame.fill(&Path::circle(seal, 9.0), color);
    frame.fill_text(Text {
        content: n.to_string(),
        position: seal,
        color: theme::panel_color(),
        size: 11.0.into(),
        font: carto::SERIF,
        align_x: iced::alignment::Horizontal::Center.into(),
        align_y: iced::alignment::Vertical::Center,
        ..Text::default()
    });
}

/// Each workspace as a harbor town on the shore, with its charter or none.
fn towns(frame: &mut Frame, rules: &Rules, hovered: Option<&str>) {
    for (c, p) in rules.charters().iter().zip(town_places(rules)) {
        let at = p.at;
        carto::outpost(frame, at);
        let note = match (hovered == Some(p.key.as_str()), c.files.is_empty()) {
            (true, _) => "press to see what it has learned",
            (false, true) => "no charter",
            (false, false) => "with its charter",
        };
        carto::name(frame, Point::new(at.x, at.y + 8.0), &c.workspace, note);
    }
}

/// A workflow's route: its name, the way out through its steps, and its
/// summit.
/// A trail's wear at each waypoint: an amber seal for the journeys waiting
/// there, a red cross with a count for those that ended there.
fn worn(frame: &mut Frame, journeys: &[JobSummary], p: &Place) {
    let (waiting, ended) = wear::waypoint(journeys, &p.key);
    seal(
        frame,
        Point::new(p.at.x - 18.0, p.at.y - 40.0),
        waiting,
        theme::warn(),
    );
    if ended > 0 {
        let at = Point::new(p.at.x + 24.0, p.at.y - 6.0);
        carto::cross(frame, at, theme::bad());
        let count = ended.to_string();
        carto::name(frame, Point::new(at.x + 14.0, at.y - 6.0), &count, "");
    }
}

/// The legend along the foot of the chart: what each mark means.
fn legend(frame: &mut Frame, size: Size) {
    let y = size.height - LEGEND + 30.0;
    let mut x = SHORE + 60.0;
    let mut item = |frame: &mut Frame, draw: &dyn Fn(&mut Frame, Point), words: &str| {
        draw(frame, Point::new(x, y));
        carto::caption(frame, Point::new(x + 24.0, y - 14.0), words);
        x += 60.0 + words.len() as f32 * 6.5;
    };
    item(frame, &|f, at| carto::outpost(f, at), "a region");
    item(
        frame,
        &|f, at| carto::mountain(f, at, 24.0, false),
        "an agent's waypoint",
    );
    item(frame, &|f, at| carto::castle(f, at, false), "a script");
    item(
        frame,
        &|f, at| seal(f, Point::new(at.x, at.y - 8.0), 3, theme::bad()),
        "rules on here",
    );
    item(
        frame,
        &|f, at| seal(f, Point::new(at.x, at.y - 8.0), 2, theme::warn()),
        "journeys waiting",
    );
    item(
        frame,
        &|f, at| carto::cross(f, Point::new(at.x, at.y - 8.0), theme::bad()),
        "ended in the marsh",
    );
}

fn route(frame: &mut Frame, w: &WorkflowView, row: usize, width: f32, steps: &[Point], note: &str) {
    let base = TOP + row as f32 * ROW + 60.0;
    let start = Point::new(WEST - 60.0, base + 6.0);
    let end = Point::new(width - EAST * 0.55, base - 40.0);
    let way = Stroke {
        line_dash: LineDash {
            segments: &[9.0, 6.0],
            offset: 0,
        },
        ..Stroke::default()
    };
    let stops: Vec<Point> = std::iter::once(start)
        .chain(steps.iter().copied())
        .chain(std::iter::once(Point::new(end.x, end.y + 30.0)))
        .collect();
    let path = Path::new(|p| {
        p.move_to(stops[0]);
        for pair in stops.windows(2) {
            let mid = Point::new(
                (pair[0].x + pair[1].x) / 2.0,
                pair[0].y.min(pair[1].y) - 12.0,
            );
            p.quadratic_curve_to(mid, pair[1]);
        }
    });
    frame.stroke(&path, way.with_width(2.5).with_color(theme::link_color()));
    carto::name(
        frame,
        Point::new(start.x + 10.0, base - 64.0),
        &w.name,
        note,
    );
    carto::mountain(frame, Point::new(end.x, end.y + 30.0), 54.0, false);
    carto::flag(frame, Point::new(end.x, end.y - 24.0), theme::bad());
}

impl canvas::Program<Picked> for Chart<'_> {
    /// The landmark the pointer is over.
    type State = Option<String>;

    // iced's trait fixes this signature.
    fn update(
        &self,
        hovered: &mut Option<String>,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Picked>> {
        let places = places(self.rules, bounds.width);
        let over = cursor.position_in(bounds).and_then(|p| under(&places, p));
        let press = matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        );
        if let (true, Some(p)) = (press, over) {
            let picked = Picked {
                key: p.key.clone(),
                stage: p.stage.clone(),
                at: p.at,
                width: bounds.width,
            };
            return Some(canvas::Action::publish(picked).and_capture());
        }
        let key = over.map(|p| p.key.clone());
        if key != *hovered {
            *hovered = key;
            return Some(canvas::Action::request_redraw());
        }
        None
    }

    // iced's trait fixes this signature.
    fn draw(
        &self,
        hovered: &Option<String>,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = Size::new(bounds.width, height(self.rules));
        let mut frame = Frame::new(renderer, bounds.size());
        carto::rhumbs(
            &mut frame,
            Point::new(size.width * 0.6, size.height * 0.5),
            size,
        );
        carto::coast(&mut frame, SHORE, size);
        towns(&mut frame, self.rules, hovered.as_deref());
        let all = places(self.rules, size.width);
        for (row, (w, _)) in routes(self.rules).into_iter().enumerate() {
            let prefix = format!("{}/", w.id);
            let mine: Vec<Point> = all
                .iter()
                .filter(|p| p.key.starts_with(&prefix))
                .map(|p| p.at)
                .collect();
            let note = wear::line(wear::trail(self.journeys, &w.id));
            route(&mut frame, w, row, size.width, &mine, &note);
        }
        for p in all.into_iter().filter(|p| p.stage != "town") {
            let lit = Some(p.key.as_str()) == self.picked || hovered.as_deref() == Some(&p.key);
            if p.stage == "shell" {
                carto::castle(&mut frame, p.at, false);
            } else {
                carto::mountain(&mut frame, p.at, if lit { 44.0 } else { 36.0 }, false);
            }
            let at = At {
                scope: &p.key,
                stage: &p.stage,
            };
            badge(&mut frame, p.at, step_rules::count(self.rules, &at));
            worn(&mut frame, self.journeys, &p);
            let step = p
                .key
                .split_once('/')
                .map_or("", |(_, s)| s)
                .replace('_', " ");
            let note = if lit { "press to see what applies" } else { "" };
            carto::name(&mut frame, Point::new(p.at.x, p.at.y + 8.0), &step, note);
        }
        let corner = Point::new(size.width - 52.0, size.height - 60.0);
        carto::compass(&mut frame, corner, 24.0);
        legend(&mut frame, size);
        carto::border(&mut frame, size);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        hovered: &Option<String>,
        _: Rectangle,
        _: mouse::Cursor,
    ) -> mouse::Interaction {
        match hovered {
            Some(_) => mouse::Interaction::Pointer,
            None => mouse::Interaction::default(),
        }
    }
}

/// One server's rules, and the step picked on its chart.
#[derive(Default)]
struct Server {
    rules: Rules,
    /// The server's field notes, newest first (D59).
    atlas: Vec<FieldNoteView>,
    /// The server's recent journeys.
    journeys: Vec<JobSummary>,
    picked: Option<Picked>,
    /// The card shows what applies, not what the step does.
    applies: bool,
}

/// Each server shown, by its name.
#[derive(Default)]
pub struct State {
    servers: BTreeMap<String, Server>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Rules(String, step_rules::Msg),
    /// A landmark picked on a server's chart.
    Pick(String, Picked),
    /// Close the card on a server's chart.
    Close(String),
    /// Show what applies (true) or what the step does (false).
    Tab(String, bool),
    /// A server's field notes; an older server has none to give.
    Atlas(String, Result<Vec<FieldNoteView>, String>),
    Journeys(String, Result<Vec<JobSummary>, String>),
}

fn on(server: String) -> impl Fn(step_rules::Msg) -> Message + Clone {
    move |m| Message::Map(Msg::Rules(server.clone(), m))
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let each = ctx.shown().into_iter().map(|name| {
        let here = ctx.on(&name);
        let (who, whose) = (name.clone(), name.clone());
        let atlas = Task::perform(here.clone().get(name.clone(), api::Atlas), move |r| {
            Message::Map(Msg::Atlas(who.clone(), r))
        });
        let journeys = Task::perform(here.clone().get(name.clone(), api::Jobs), move |r| {
            Message::Map(Msg::Journeys(whose.clone(), r))
        });
        Task::batch([step_rules::load(&here).map(on(name)), atlas, journeys])
    });
    Task::batch(each)
}

pub fn update(state: &mut State, msg: Msg, ctx: &Ctx) -> Task<Message> {
    match msg {
        Msg::Rules(_, step_rules::Msg::Go) => return Task::done(Message::Go(Route::Harness)),
        Msg::Rules(name, m) => {
            let rules = &mut state.servers.entry(name.clone()).or_default().rules;
            let at = step_rules::level(true);
            return step_rules::update(rules, m, &at, &ctx.on(&name)).map(on(name));
        }
        Msg::Pick(name, picked) => state.servers.entry(name).or_default().picked = Some(picked),
        Msg::Close(name) => state.servers.entry(name).or_default().picked = None,
        Msg::Atlas(name, notes) => {
            state.servers.entry(name).or_default().atlas = notes.unwrap_or_default()
        }
        Msg::Journeys(name, all) => {
            state.servers.entry(name).or_default().journeys = all.unwrap_or_default()
        }
        Msg::Tab(name, applies) => state.servers.entry(name).or_default().applies = applies,
    }
    Task::none()
}

/// A server's chart, and under it what applies at the step picked.
fn section<'a>(name: String, s: &'a Server, named: bool) -> Element<'a, Message> {
    let chart = Chart {
        rules: &s.rules,
        picked: s.picked.as_ref().map(|p| p.key.as_str()),
        journeys: &s.journeys,
    };
    let h = height(&s.rules);
    let who = name.clone();
    let canvas = Element::from(Canvas::new(chart).width(Fill).height(h))
        .map(move |p| Message::Map(Msg::Pick(who.clone(), p)));
    let head = named.then(|| text(name.clone()).size(18).font(theme::BOLD).into());
    let mut map = stack![canvas];
    if let Some(p) = &s.picked {
        let at = card_at(p, h);
        map = map.push(
            pin(card(s, p, name.clone()))
                .x(at.x)
                .y(at.y)
                .width(Fill)
                .height(h),
        );
    }
    Column::new().spacing(10).extend(head).push(map).into()
}

/// One shelf of a workspace's atlas: its notes about one thing.
fn shelf<'a>(title: &'a str, notes: Vec<&'a FieldNoteView>) -> Option<Element<'a, Message>> {
    if notes.is_empty() {
        return None;
    }
    let rows = notes.into_iter().map(|n| {
        let mark = if n.landed { "landed" } else { "the marsh" };
        let whence = format!("{} · {} · {mark} · {}", n.by, n.request, super::ago(n.at));
        column![
            text(n.text.clone()).size(13),
            text(whence).size(12).color(theme::muted())
        ]
        .spacing(2)
        .into()
    });
    let head = text(title).size(12).font(theme::BOLD).color(theme::muted());
    Some(Column::new().spacing(8).push(head).extend(rows).into())
}

/// A workspace's atlas: its field notes, about the workspace and about
/// Bastion (D59).
fn atlas_card<'a>(atlas: &'a [FieldNoteView], ws: &'a str, name: String) -> Element<'a, Message> {
    let title = text(format!("What {ws} has learned"))
        .size(15)
        .font(theme::BOLD);
    let close = crate::lift::button(text("Close").size(13))
        .style(theme::link)
        .on_press(Message::Map(Msg::Close(name)));
    let head = row![title, iced::widget::space::horizontal(), close].align_y(iced::Center);
    let mine = |about: &'a str| -> Vec<&'a FieldNoteView> {
        let all = atlas.iter().filter(|n| n.workspace == ws);
        all.filter(|n| n.about == about).collect()
    };
    let shelves = [
        shelf("Lessons, in its charter", mine("lesson")),
        shelf(
            "Facts, in its corpus",
            [mine("fact"), mine("workspace")].concat(),
        ),
        shelf("About Bastion", mine("bastion")),
    ];
    let mut body = Column::new()
        .spacing(14)
        .extend(shelves.into_iter().flatten());
    if !atlas.iter().any(|n| n.workspace == ws) {
        let none = "Nothing learned yet. Notes are signed when a journey comes home.";
        body = body.push(text(none).size(13).color(theme::muted()));
    }
    let body = scrollable(body).height(iced::Length::Shrink);
    container(column![head, container(body).max_height(CARD_MAX)].spacing(8))
        .padding(12)
        .width(CARD)
        .style(theme::panel)
        .into()
}

/// The step a key names, `workflow/step`.
fn node<'a>(rules: &'a Rules, key: &str) -> Option<&'a NodeView> {
    let (w, step) = key.split_once('/')?;
    let flow = rules.workflows().iter().find(|f| f.id == w)?;
    flow.nodes.iter().find(|n| n.id == step)
}

/// One labeled line of the card, or nothing when it has no value.
fn fact<'a>(label: &'a str, value: String) -> Option<Element<'a, Message>> {
    (!value.trim().is_empty()).then(|| {
        row![
            text(label).size(12).color(theme::muted()).width(96),
            text(value).size(13).width(Fill)
        ]
        .spacing(8)
        .into()
    })
}

/// What a step's reach allows, in words.
fn reach(n: &NodeView) -> String {
    let mut all = Vec::new();
    if n.reach.reads_only {
        all.push("reads only".to_string());
    }
    if n.reach.network {
        all.push("network".to_string());
    }
    all.extend(n.reach.secrets.iter().map(|s| format!("secret {s}")));
    all.join(" · ")
}

/// A step's budget: the most its turns may spend, and how often it may
/// send work back.
fn budget(n: &NodeView) -> String {
    let parts = [
        n.options
            .spend_micros
            .map(|m| format!("${:.2}", m as f64 / 1e6)),
        n.options.revisions.map(|r| format!("{r} sends back")),
    ];
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    parts.join(" · ")
}

/// What the step does: what it runs, who decides, its model and budget,
/// its reach, the results it gives, and its instructions.
pub(super) fn does(n: &NodeView) -> Element<'_, Message> {
    let who = match n.dial.as_str() {
        "dark" => "it runs on its own",
        _ => "a person decides",
    };
    let facts = [
        fact("Runs", super::drawing::kind_line(n)),
        fact("Who decides", who.into()),
        fact("Model", n.options.model.clone()),
        fact("Budget", budget(n)),
        fact("Reach", reach(n)),
        fact("Gives", n.results.join(", ")),
    ];
    let mut body = Column::new().spacing(6).extend(facts.into_iter().flatten());
    if !n.note.trim().is_empty() {
        let note = container(text(n.note.clone()).size(13).font(theme::MONO))
            .padding(8)
            .width(Fill)
            .style(theme::panel);
        let label = text("Instructions").size(12).color(theme::muted());
        body = body.push(label).push(note);
    }
    body.into()
}

/// The card over the chart for the landmark picked: what the step does, or
/// what applies there.
fn card<'a>(s: &'a Server, p: &'a Picked, name: String) -> Element<'a, Message> {
    if let Some(ws) = p.key.strip_prefix(TOWN) {
        return atlas_card(&s.atlas, ws, name);
    }
    let at = At {
        scope: &p.key,
        stage: &p.stage,
    };
    let title = text(p.key.replace('/', " › ")).size(15).font(theme::BOLD);
    let close = crate::lift::button(text("Close").size(13))
        .style(theme::link)
        .on_press(Message::Map(Msg::Close(name.clone())));
    let tab = |label: &'a str, applies: bool| {
        let msg = Message::Map(Msg::Tab(name.clone(), applies));
        step_rules::choice(label, s.applies == applies, Some(msg))
    };
    let tabs = row![tab("What it does", false), tab("What applies", true)].spacing(6);
    let head = row![title, iced::widget::space::horizontal(), close].align_y(iced::Center);
    let body: Element<'a, Message> = match (s.applies, node(&s.rules, &p.key)) {
        (false, Some(n)) => does(n),
        (false, None) => text("This step is gone from its workflow.").size(13).into(),
        (true, _) => step_rules::view(&s.rules, &at).map(on(name)),
    };
    let body = scrollable(body).height(iced::Length::Shrink);
    container(column![head, tabs, container(body).max_height(CARD_MAX)].spacing(8))
        .padding(12)
        .width(CARD)
        .style(theme::panel)
        .into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let shown = ctx.shown();
    let named = shown.len() > 1;
    let sections = shown
        .into_iter()
        .filter_map(|name| state.servers.get(&name).map(|s| section(name, s, named)));
    let about = "Each server's regions along the coast, and its trails inland. Press a waypoint to see what it does and what applies, or a harbor to see what its region has learned.";
    let body = column![super::head("Map", about, vec![])]
        .extend(sections)
        .spacing(16);
    scrolled(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_finds_the_landmark_and_nothing_else() {
        let places = [Place {
            key: "bug-fix/plan".into(),
            stage: "prompt".into(),
            at: Point::new(300.0, 200.0),
        }];
        let hit = under(&places, Point::new(304.0, 186.0));
        assert_eq!(hit.map(|p| p.key.as_str()), Some("bug-fix/plan"));
        assert!(under(&places, Point::new(10.0, 10.0)).is_none());
    }
}
