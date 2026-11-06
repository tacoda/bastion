//! The Workflows page (PLAN.md D35, W2): each workflow drawn on a canvas.
//! An admin drags a step, links two by dragging from a step's right edge to
//! another step, adds a step from the palette, opens a step to set what it
//! runs, its dial, its note and its results, and saves a new version. A
//! built-in workflow can only be copied, and nobody but an admin edits.

use std::collections::BTreeMap;

use bastion_protocol::{LinkView, NodeView, PersonView, WorkflowView, command};
use leptos::ev::PointerEvent;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Serialize;

use crate::ipc::{self, None};
use crate::pages::factory_settings::{BUTTON, read};
use crate::route::mark;

/// A step's box on the canvas, and the margin around the drawing.
const W: i32 = 168;
const H: i32 = 56;
const PAD: i32 = 24;
const INPUT: &str = "rounded border border-line bg-bg px-2 py-1 text-sm text-ink";
const SECTION: &str = "mt-4 grid gap-2 rounded-xl border border-line bg-panel p-4";
/// The steps that are not Bastion stages, the preferred building block first.
const CUSTOM: [&str; 3] = ["skill", "prompt", "command"];

#[derive(Serialize)]
struct Save {
    id: String,
    workflow: WorkflowView,
}

#[derive(Serialize)]
struct CopyOf {
    from: String,
    name: String,
}

#[derive(Serialize)]
struct Id {
    id: String,
}

/// What the canvas has picked: nothing, a step by id, or a link.
#[derive(Clone, Debug, PartialEq)]
enum Pick {
    Nothing,
    Step(String),
    Link(String, String),
}

/// A drag under way: moving step `node`, or drawing a link out of it.
/// `start` is where the pointer went down, `origin` the point it moved.
#[derive(Clone, Debug, PartialEq)]
struct Drag {
    node: String,
    linking: bool,
    start: (i32, i32),
    origin: (i32, i32),
    now: (i32, i32),
}

impl Drag {
    fn new(node: String, linking: bool, start: (i32, i32), origin: (i32, i32)) -> Drag {
        Drag {
            node,
            linking,
            start,
            origin,
            now: start,
        }
    }

    /// Where the dragged point is now, never left of or above the canvas.
    fn moved(&self) -> (i32, i32) {
        let x = self.origin.0 + self.now.0 - self.start.0;
        let y = self.origin.1 + self.now.1 - self.start.1;
        (x.max(0), y.max(0))
    }
}

/// The canvas's state: the workflow drawn, what is picked, any drag, and
/// each step's state by node id when a job runs it.
#[derive(Clone, Copy)]
struct Board {
    draft: RwSignal<Option<WorkflowView>>,
    picked: RwSignal<Pick>,
    drag: RwSignal<Option<Drag>>,
    marks: Signal<BTreeMap<String, String>>,
}

fn at(e: &PointerEvent) -> (i32, i32) {
    (e.client_x(), e.client_y())
}

fn node<'a>(w: &'a WorkflowView, id: &str) -> Option<&'a NodeView> {
    w.nodes.iter().find(|n| n.id == id)
}

/// A step id from `base` that `w` does not use yet.
fn fresh_id(w: &WorkflowView, base: &str) -> String {
    let taken = |id: &str| w.nodes.iter().any(|n| n.id == id);
    let mut id = base.to_string();
    let mut n = 1;
    while taken(&id) {
        n += 1;
        id = format!("{base}{n}");
    }
    id
}

/// Add a lights-on step of `stage` right of the others.
fn add_step(w: &mut WorkflowView, stage: &str) {
    let x = w.nodes.iter().map(|n| n.x + 220).max().unwrap_or(0);
    w.nodes.push(NodeView {
        id: fresh_id(w, stage),
        stage: stage.into(),
        dial: "lights_on".into(),
        x,
        ..NodeView::default()
    });
}

fn add_link(w: &mut WorkflowView, from: &str, to: &str) {
    let exists = w.links.iter().any(|l| l.from == from && l.to == to);
    if from != to && !exists {
        w.links.push(LinkView {
            from: from.into(),
            to: to.into(),
        });
    }
}

/// Remove step `id` and every link to or from it.
fn remove_step(w: &mut WorkflowView, id: &str) {
    w.nodes.retain(|n| n.id != id);
    w.links.retain(|l| l.from != id && l.to != id);
}

/// The Bastion stages the built-in workflows use, in their order.
fn stages(all: &[WorkflowView]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for n in all.iter().filter(|w| w.built_in).flat_map(|w| &w.nodes) {
        if !found.contains(&n.stage) {
            found.push(n.stage.clone());
        }
    }
    found
}

fn words(word: &str) -> String {
    word.replace('_', " ")
}

/// The canvas size that holds every step.
fn extent(w: Option<&WorkflowView>) -> (i32, i32) {
    let nodes = w.map(|w| w.nodes.as_slice()).unwrap_or_default();
    let x = nodes.iter().map(|n| n.x).max().unwrap_or(0);
    let y = nodes.iter().map(|n| n.y).max().unwrap_or(0);
    ((x + W + 2 * PAD).max(800), (y + H + 2 * PAD).max(240))
}

fn edit(draft: RwSignal<Option<WorkflowView>>, f: impl FnOnce(&mut WorkflowView)) {
    draft.update(|d| {
        if let Some(w) = d {
            f(w);
        }
    });
}

fn edit_step(draft: RwSignal<Option<WorkflowView>>, id: &str, f: impl FnOnce(&mut NodeView)) {
    edit(draft, |w| {
        if let Some(n) = w.nodes.iter_mut().find(|n| n.id == id) {
            f(n);
        }
    });
}

/// A step's first line: its stage, or a skill's or command's name.
fn title(n: &NodeView) -> String {
    match n.stage.as_str() {
        "skill" | "command" if !n.run.is_empty() => format!("{} · {}", n.stage, n.run),
        _ => words(&n.stage),
    }
}

/// A step's second line: its dial and the results it gives.
fn subtitle(n: &NodeView) -> String {
    let dial = if n.dial == "dark" {
        "dark"
    } else {
        "lights-on"
    };
    match n.results.is_empty() {
        true => dial.into(),
        false => format!("{dial} → {}", n.results.join(", ")),
    }
}

/// The link from `l.from`'s right edge to `l.to`'s left edge.
fn link_line(
    w: &WorkflowView,
    l: &LinkView,
    picked: RwSignal<Pick>,
) -> Option<impl IntoView + use<>> {
    let (f, t) = (node(w, &l.from)?, node(w, &l.to)?);
    let (x1, y1, x2, y2) = (f.x + W, f.y + H / 2, t.x, t.y + H / 2);
    let d = format!("M{x1} {y1} C{} {y1} {} {y2} {x2} {y2}", x1 + 60, x2 - 60);
    let pick = Pick::Link(l.from.clone(), l.to.clone());
    let look = match picked.get() == pick {
        true => "stroke-primary",
        false => "stroke-muted",
    };
    Some(view! {
        <g class="cursor-pointer" on:pointerdown=move |_| picked.set(pick.clone())>
            <path d=d.clone() fill="none" stroke="transparent" stroke-width="12" />
            <path d=d fill="none" class=look stroke-width="2" marker-end="url(#arrow)" />
        </g>
    })
}

/// One step's box. An editable box moves when dragged, and its right edge
/// starts a link that ends on the box the pointer is let go over.
fn step_box(n: &NodeView, board: Board, editable: bool) -> impl IntoView + use<> {
    let Board {
        draft,
        picked,
        drag,
        marks,
    } = board;
    let (x, y, id) = (n.x, n.y, n.id.clone());
    let state = marks.with(|m| m.get(&id).map(|s| mark(s)));
    let look = match picked.get() == Pick::Step(id.clone()) {
        true => "fill-bg stroke-primary",
        false => "fill-bg stroke-line",
    };
    let (moving, linking, target) = (id.clone(), id.clone(), id.clone());
    let down = move |e: PointerEvent| {
        picked.set(Pick::Step(moving.clone()));
        if editable {
            drag.set(Some(Drag::new(moving.clone(), false, at(&e), (x, y))));
        }
    };
    let up = move |_: PointerEvent| {
        if let Some(d) = drag.get_untracked().filter(|d| d.linking) {
            edit(draft, |w| add_link(w, &d.node, &target));
        }
    };
    let port = move |e: PointerEvent| {
        e.stop_propagation();
        let from = (x + W, y + H / 2);
        drag.set(Some(Drag::new(linking.clone(), true, at(&e), from)));
    };
    view! {
        <g transform=format!("translate({x},{y})") class="cursor-grab" on:pointerdown=down on:pointerup=up>
            <rect width=W height=H rx="8" class=look stroke-width="2" />
            <text x="10" y="23" class="fill-ink text-sm font-semibold">
                {state.map(|(symbol, color)| view! { <tspan class=color style="fill: currentColor">{format!("{symbol} ")}</tspan> })}
                {title(n)}
            </text>
            <text x="10" y="43" class="fill-muted text-xs">{subtitle(n)}</text>
            {editable.then(|| view! {
                <circle cx=W cy=H / 2 r="7" class="cursor-crosshair fill-primary" on:pointerdown=port>
                    <title>"Drag to the next step to link them"</title>
                </circle>
            })}
        </g>
    }
}

/// Follow the pointer: move the dragged step, or the end of a new link.
fn follow(draft: RwSignal<Option<WorkflowView>>, drag: RwSignal<Option<Drag>>, e: &PointerEvent) {
    let Some(mut d) = drag.get_untracked() else {
        return;
    };
    d.now = at(e);
    if !d.linking {
        let (x, y) = d.moved();
        edit_step(draft, &d.node, |n| (n.x, n.y) = (x, y));
    }
    drag.set(Some(d));
}

/// The workflow's steps and links, drawn to scale.
fn canvas(
    draft: RwSignal<Option<WorkflowView>>,
    picked: RwSignal<Pick>,
    editable: Signal<bool>,
    marks: Signal<BTreeMap<String, String>>,
) -> impl IntoView {
    let drag = RwSignal::new(None::<Drag>);
    let follow = move |e: PointerEvent| follow(draft, drag, &e);
    let size = move || draft.with(|d| extent(d.as_ref()));
    let links = move || {
        draft.with(|d| {
            d.as_ref().map(|w| {
                w.links
                    .iter()
                    .map(|l| link_line(w, l, picked))
                    .collect_view()
            })
        })
    };
    let steps = move || {
        let on = editable.get();
        draft.with(|d| {
            d.as_ref().map(|w| {
                w.nodes
                    .iter()
                    .map(|n| {
                        step_box(
                            n,
                            Board {
                                draft,
                                picked,
                                drag,
                                marks,
                            },
                            on,
                        )
                    })
                    .collect_view()
            })
        })
    };
    let pending = move || {
        drag.get().filter(|d| d.linking).map(|d| {
            let (x, y) = d.moved();
            view! { <line x1=d.origin.0 y1=d.origin.1 x2=x y2=y class="stroke-primary" stroke-width="2" stroke-dasharray="4 4" /> }
        })
    };
    view! {
        <div class="overflow-auto rounded-xl border border-line bg-panel">
            <svg width=move || size().0 height=move || size().1 class="touch-none select-none"
                role="img" aria-label="The workflow's steps and the links between them"
                on:pointermove=follow on:pointerup=move |_| drag.set(None) on:pointerleave=move |_| drag.set(None)>
                <defs>
                    <marker id="arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="8" markerHeight="8" orient="auto">
                        <path d="M0 0L10 5L0 10z" class="fill-muted" />
                    </marker>
                </defs>
                <g transform=format!("translate({PAD},{PAD})")>{links}{steps}{pending}</g>
            </svg>
        </div>
    }
}

/// A workflow drawn read-only, each step marked with its state by node id.
pub(crate) fn drawing(
    workflow: Signal<Option<WorkflowView>>,
    marks: Signal<BTreeMap<String, String>>,
) -> impl IntoView {
    let draft = RwSignal::new(None);
    Effect::new(move || draft.set(workflow.get()));
    canvas(
        draft,
        RwSignal::new(Pick::Nothing),
        Signal::stored(false),
        marks,
    )
}

/// A labelled text box for one of a step's fields, kept on change.
fn field(
    label: &'static str,
    value: String,
    long: bool,
    keep: impl Fn(String) + Copy + 'static,
) -> impl IntoView {
    let input = match long {
        true => view! { <textarea class=INPUT rows="4" on:change=move |e| keep(event_target_value(&e))>{value}</textarea> }.into_any(),
        false => view! { <input class=INPUT value=value on:change=move |e| keep(event_target_value(&e)) /> }.into_any(),
    };
    view! { <label class="grid gap-1 text-sm text-muted">{label}{input}</label> }
}

/// The picked step, opened: what it runs, its dial, its note and results.
fn step_detail(
    draft: RwSignal<Option<WorkflowView>>,
    n: NodeView,
    editable: bool,
) -> impl IntoView {
    let id = StoredValue::new(n.id.clone());
    let set = move |f: fn(&mut NodeView, String)| {
        move |v: String| edit_step(draft, &id.get_value(), |n| f(n, v))
    };
    let run = match n.stage.as_str() {
        "skill" => Some("Skill: the folder under .claude/skills"),
        "command" => Some("Command line, run in the repository"),
        _ => None,
    };
    let note = match n.stage == "prompt" {
        true => "Instructions",
        false => "Note to the agent",
    };
    let dark = n.dial == "dark";
    view! {
        <fieldset class=SECTION disabled=!editable>
            <legend class="px-1 font-semibold">{title(&n)}<span class="ml-2 text-sm text-muted">{n.id.clone()}</span></legend>
            {run.map(|label| field(label, n.run.clone(), false, set(|n, v| n.run = v)))}
            <label class="flex items-center gap-2 text-sm text-muted">"Dial"
                <select class=INPUT on:change=move |e| { let v = event_target_value(&e); edit_step(draft, &id.get_value(), |n| n.dial = v) }>
                    <option value="lights_on" selected=!dark>"lights-on: a person decides"</option>
                    <option value="dark" selected=dark>"dark: runs on its own"</option>
                </select>
            </label>
            {field(note, n.note.clone(), true, set(|n, v| n.note = v))}
            {field("Results it gives, comma-separated", n.results.join(", "), false, set(|n, v| {
                n.results = v.split(',').map(str::trim).filter(|r| !r.is_empty()).map(String::from).collect();
            }))}
            <button class=format!("{BUTTON} w-fit") on:click=move |_| edit(draft, |w| remove_step(w, &id.get_value()))>"Remove step"</button>
        </fieldset>
    }
}

/// What the canvas has picked, opened below it.
fn picked_detail(
    draft: RwSignal<Option<WorkflowView>>,
    picked: RwSignal<Pick>,
    editable: Signal<bool>,
) -> impl IntoView {
    move || match picked.get() {
        Pick::Step(id) => draft
            .with(|d| d.as_ref().and_then(|w| node(w, &id)).cloned())
            .map(|n| step_detail(draft, n, editable.get()).into_any()),
        Pick::Link(from, to) if editable.get() => Some(
            view! {
                <div class=SECTION>
                    <p class="m-0">{format!("The link from {from} to {to}")}</p>
                    <button class=format!("{BUTTON} w-fit") on:click=move |_| {
                        edit(draft, |w| w.links.retain(|l| !(l.from == from && l.to == to)));
                        picked.set(Pick::Nothing);
                    }>"Remove link"</button>
                </div>
            }
            .into_any(),
        ),
        _ => None,
    }
}

/// Add a skill, a prompt, a command, or a Bastion stage not used yet.
fn palette(draft: RwSignal<Option<WorkflowView>>, stages: Signal<Vec<String>>) -> impl IntoView {
    let unused = move || {
        let used: Vec<String> = draft.with(|d| {
            d.iter()
                .flat_map(|w| &w.nodes)
                .map(|n| n.stage.clone())
                .collect()
        });
        stages.get().into_iter().filter(move |s| !used.contains(s))
    };
    view! {
        <div class="mb-3 flex flex-wrap items-center gap-2 text-sm">
            <span class="text-muted">"Add a step"</span>
            {CUSTOM.map(|s| view! { <button class=BUTTON on:click=move |_| edit(draft, |w| add_step(w, s))>{s}</button> })}
            <select class=INPUT aria-label="Add a Bastion stage" on:change=move |e| {
                let s = event_target_value(&e);
                if !s.is_empty() { edit(draft, |w| add_step(w, &s)) }
            }>
                <option value="" selected=true>"a Bastion stage…"</option>
                {move || unused().map(|s| view! { <option value=s.clone()>{words(&s)}</option> }).collect_view()}
            </select>
        </div>
    }
}

/// Where an action says how it went, and how the page reads again after.
#[derive(Clone, Copy)]
struct Answer {
    note: RwSignal<String>,
    reload: Callback<()>,
}

impl Answer {
    /// Run `cmd`, then say what `then` says and read the workflows again,
    /// or say why not.
    fn act<T: serde::de::DeserializeOwned + 'static, A: Serialize + 'static>(
        self,
        cmd: &'static str,
        args: A,
        then: impl FnOnce(T) -> String + 'static,
    ) {
        spawn_local(async move {
            match ipc::call::<T, String>(cmd, args).await {
                Ok(v) => {
                    self.note.set(then(v));
                    self.reload.run(());
                }
                Err(e) => self.note.set(e.text()),
            }
        });
    }
}

/// Copy the shown workflow under a new name, and delete one that is not
/// built in. Admins only.
fn manage(
    draft: RwSignal<Option<WorkflowView>>,
    chosen: RwSignal<String>,
    answer: Answer,
) -> impl IntoView {
    let name = RwSignal::new(String::new());
    let copy = move |_| {
        let args = CopyOf {
            from: chosen.get_untracked(),
            name: name.get_untracked(),
        };
        let then = move |id: String| {
            chosen.set(id);
            "Copied. Change it, then save.".into()
        };
        answer.act(command::WORKFLOW_COPY, args, then);
    };
    let delete = move |_| {
        let args = Id {
            id: chosen.get_untracked(),
        };
        let then = move |()| {
            chosen.set(String::new());
            "Deleted. Its versions stay in the history.".into()
        };
        answer.act(command::WORKFLOW_DELETE, args, then);
    };
    let built_in = move || draft.with(|d| d.as_ref().is_none_or(|w| w.built_in));
    view! {
        <div class="mb-3 flex flex-wrap items-center gap-2 text-sm">
            <input class=INPUT placeholder="Name of the copy" aria-label="Name of the copy" bind:value=name />
            <button class=BUTTON disabled=move || name.get().trim().is_empty() on:click=copy>"Copy"</button>
            <Show when=move || !built_in()>
                <button class=BUTTON on:click=delete>"Delete"</button>
            </Show>
        </div>
    }
}

/// The shown workflow's facts: built in or not, its version and its level.
fn facts(w: &WorkflowView) -> String {
    let kind = match w.built_in {
        true => "built in, copy it to change it",
        false => "yours to change",
    };
    let level = words(&w.level).replace("lights on", "lights-on");
    format!("{kind} · version {} · runs {level}", w.version)
}

#[component]
pub fn WorkflowsPage() -> impl IntoView {
    let (all, reload) = read::<Vec<WorkflowView>>(command::WORKFLOWS);
    let me = LocalResource::new(|| async {
        ipc::call::<PersonView, String>(command::ME, None {})
            .await
            .is_ok_and(|p| p.role == "admin")
    });
    let admin = Signal::derive(move || me.get() == Some(true));
    let chosen = RwSignal::new(String::new());
    let draft = RwSignal::new(None::<WorkflowView>);
    let picked = RwSignal::new(Pick::Nothing);
    let note = RwSignal::new(String::new());
    // Show the chosen workflow, or the first, as the server has it. A draft
    // stays until another is chosen or the server keeps a new version.
    Effect::new(move || {
        let Some(list) = all.get() else { return };
        let want = chosen.get();
        let fresh = match want.is_empty() {
            true => list.first(),
            false => list.iter().find(|w| w.id == want),
        };
        let Some(fresh) = fresh.cloned() else { return };
        let same = |d: &WorkflowView| (&d.id, d.version) == (&fresh.id, fresh.version);
        if !draft.with_untracked(|d| d.as_ref().is_some_and(same)) {
            chosen.set(fresh.id.clone());
            draft.set(Some(fresh));
            picked.set(Pick::Nothing);
        }
    });
    let editable = Signal::derive(move || {
        admin.get() && draft.with(|d| d.as_ref().is_some_and(|w| !w.built_in))
    });
    let answer = Answer { note, reload };
    let known = Signal::derive(move || all.with(|a| stages(a.as_deref().unwrap_or_default())));
    let save = move |_| {
        let Some(w) = draft.get_untracked() else {
            return;
        };
        let args = Save {
            id: w.id.clone(),
            workflow: w,
        };
        let then = |v: u64| format!("Saved as version {v}.");
        answer.act(command::WORKFLOW_SAVE, args, then);
    };
    view! {
        <h1 class="mt-0 text-2xl font-bold">"Workflows"</h1>
        <div class="mb-3 flex flex-wrap items-center gap-3 text-sm">
            <select class=INPUT aria-label="Workflow" on:change=move |e| chosen.set(event_target_value(&e))>
                {move || all.get().unwrap_or_default().into_iter().map(|w| {
                    let here = w.id.clone();
                    view! { <option value=w.id selected=move || chosen.get() == here>{w.name}</option> }
                }).collect_view()}
            </select>
            <span class="text-muted">{move || draft.with(|d| d.as_ref().map(facts))}</span>
        </div>
        <Show when=move || admin.get()>{manage(draft, chosen, answer)}</Show>
        <Show when=move || editable.get()>{palette(draft, known)}</Show>
        {canvas(draft, picked, editable, Signal::stored(BTreeMap::new()))}
        {picked_detail(draft, picked, editable)}
        <Show when=move || editable.get()>
            <button class="mt-4 rounded-lg bg-primary px-3 py-2 font-semibold text-white hover:bg-primary-hover" on:click=save>"Save a new version"</button>
        </Show>
        <p class="m-0 mt-2 text-sm text-muted" role="status">{move || note.get()}</p>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow() -> WorkflowView {
        let mut w = WorkflowView::default();
        add_step(&mut w, "plan");
        add_step(&mut w, "skill");
        add_step(&mut w, "skill");
        w
    }

    #[test]
    fn a_new_step_gets_a_free_id_right_of_the_others() {
        let w = flow();
        let ids: Vec<(&str, i32)> = w.nodes.iter().map(|n| (n.id.as_str(), n.x)).collect();
        assert_eq!(ids, [("plan", 0), ("skill", 220), ("skill2", 440)]);
    }

    #[test]
    fn a_link_is_kept_once_and_goes_with_its_step() {
        let mut w = flow();
        add_link(&mut w, "plan", "skill");
        add_link(&mut w, "plan", "skill");
        add_link(&mut w, "skill", "skill");
        add_link(&mut w, "skill", "skill2");
        assert_eq!(w.links.len(), 2);
        remove_step(&mut w, "skill");
        assert!(w.links.is_empty());
        assert_eq!(w.nodes.len(), 2);
    }

    #[test]
    fn a_drag_moves_by_the_pointer_and_stays_on_the_canvas() {
        let mut d = Drag::new("plan".into(), false, (100, 100), (20, 30));
        d.now = (150, 90);
        assert_eq!(d.moved(), (70, 20));
        d.now = (0, 0);
        assert_eq!(d.moved(), (0, 0));
    }
}
