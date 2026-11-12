//! The Map (L5): each server drawn as a chart. Its workspaces are harbor
//! towns along the coast, where requests come ashore; its workflows are
//! routes across the land, each step a landmark, each route ending at its
//! summit. A badge on a landmark counts the rules that apply there.
//! Pressing a landmark opens what applies at that step. On All servers
//! each server has its chart.

use std::collections::BTreeMap;

use bastion_protocol::{NodeView, WorkflowView};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::widget::{Column, column, text};
use iced::{Element, Fill, Point, Rectangle, Renderer, Size, Task, Theme, mouse};

use super::step_rules::{self, At, Rules};
use super::{Ctx, carto, scrolled};
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
pub fn places(rules: &Rules, width: f32) -> Vec<Place> {
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

/// How tall the chart is: room for every route and every town.
fn height(rules: &Rules) -> f32 {
    let rows = rules.workflows().len().max(rules.charters().len()).max(1);
    TOP + rows as f32 * ROW + 40.0
}

/// The landmark under `p`, if one is near enough to press.
fn under(places: &[Place], p: Point) -> Option<&Place> {
    let middle = |m: &Place| Point::new(m.at.x, m.at.y - 16.0);
    places.iter().find(|m| middle(m).distance(p) <= REACH)
}

/// One server's chart. A press publishes the step's key and its kind.
struct Chart<'a> {
    rules: &'a Rules,
    picked: Option<&'a str>,
}

/// A count of the rules on at a landmark, as a red seal.
fn badge(frame: &mut Frame, at: Point, n: usize) {
    if n == 0 {
        return;
    }
    let seal = Point::new(at.x + 16.0, at.y - 40.0);
    frame.fill(&Path::circle(seal, 9.0), theme::bad());
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
fn towns(frame: &mut Frame, rules: &Rules) {
    for (i, c) in rules.charters().iter().enumerate() {
        let at = Point::new(TOWNS, TOP + i as f32 * ROW + 50.0);
        carto::outpost(frame, at);
        let note = if c.files.is_empty() {
            "no charter"
        } else {
            "with its charter"
        };
        carto::name(frame, Point::new(at.x, at.y + 8.0), &c.workspace, note);
    }
}

/// A workflow's route: its name, the way out through its steps, and its
/// summit.
fn route(frame: &mut Frame, w: &WorkflowView, row: usize, width: f32, steps: &[Point]) {
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
    let steps = format!("{} steps", w.nodes.len());
    carto::name(
        frame,
        Point::new(start.x + 10.0, base - 64.0),
        &w.name,
        &steps,
    );
    carto::mountain(frame, Point::new(end.x, end.y + 30.0), 54.0, false);
    carto::flag(frame, Point::new(end.x, end.y - 24.0), theme::bad());
}

impl canvas::Program<(String, String)> for Chart<'_> {
    /// The landmark the pointer is over.
    type State = Option<String>;

    // iced's trait fixes this signature.
    fn update(
        &self,
        hovered: &mut Option<String>,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<(String, String)>> {
        let places = places(self.rules, bounds.width);
        let over = cursor.position_in(bounds).and_then(|p| under(&places, p));
        let press = matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        );
        if let (true, Some(p)) = (press, over) {
            let picked = (p.key.clone(), p.stage.clone());
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
        towns(&mut frame, self.rules);
        let all = places(self.rules, size.width);
        for (row, (w, _)) in routes(self.rules).into_iter().enumerate() {
            let prefix = format!("{}/", w.id);
            let mine: Vec<Point> = all
                .iter()
                .filter(|p| p.key.starts_with(&prefix))
                .map(|p| p.at)
                .collect();
            route(&mut frame, w, row, size.width, &mine);
        }
        for p in all {
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
    picked: Option<(String, String)>,
}

/// Each server shown, by its name.
#[derive(Default)]
pub struct State {
    servers: BTreeMap<String, Server>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Rules(String, step_rules::Msg),
    /// A step picked on a server's chart: its `workflow/step` and its kind.
    Pick(String, (String, String)),
}

fn on(server: String) -> impl Fn(step_rules::Msg) -> Message + Clone {
    move |m| Message::Map(Msg::Rules(server.clone(), m))
}

pub fn load(ctx: &Ctx) -> Task<Message> {
    let each = ctx
        .shown()
        .into_iter()
        .map(|name| step_rules::load(&ctx.on(&name)).map(on(name)));
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
    }
    Task::none()
}

/// A server's chart, and under it what applies at the step picked.
fn section<'a>(name: String, s: &'a Server, named: bool) -> Element<'a, Message> {
    let chart = Chart {
        rules: &s.rules,
        picked: s.picked.as_ref().map(|(k, _)| k.as_str()),
    };
    let h = height(&s.rules);
    let who = name.clone();
    let canvas = Element::from(Canvas::new(chart).width(Fill).height(h))
        .map(move |p| Message::Map(Msg::Pick(who.clone(), p)));
    let head = named.then(|| text(name.clone()).size(18).font(theme::BOLD).into());
    let mut body = Column::new().spacing(10).extend(head).push(canvas);
    if let Some((key, stage)) = &s.picked {
        let at = At { scope: key, stage };
        let title = text(format!("What applies at {}", key.replace('/', " › ")))
            .size(15)
            .font(theme::BOLD);
        let list = step_rules::view(&s.rules, &at).map(on(name));
        body = body.push(column![title, list].spacing(8));
    }
    body.into()
}

pub fn view<'a>(state: &'a State, ctx: &Ctx) -> Element<'a, Message> {
    let shown = ctx.shown();
    let named = shown.len() > 1;
    let sections = shown
        .into_iter()
        .filter_map(|name| state.servers.get(&name).map(|s| section(name, s, named)));
    let about = "Each server's workspaces along the coast, and its workflows as routes. Press a landmark to see what applies there.";
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
