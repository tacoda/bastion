# Bastion plan: a layout people can read

<!-- voice-register: informational -->
<!-- voice-english: ste -->

Bastion works the way it should, but its pages don't say what it is. A
person should see three things at once: the servers they use, what each
server runs (workspaces, workflows and the harness), and the jobs moving
through it. Most of their time goes to jobs, so the job pages come first
and every setting is one click from the job it affects. The cost is a
rename and a move of most pages; the server, the API and the rules stay as
they are.

The rules plans (R1, R2, P1, P3, S1 to S4) are done, and git history holds
them. Decisions are in [DECISIONS.md](DECISIONS.md), D54 to D58.

## What the review found

### What works

- The core loop: a request runs through a workflow as a job, waits where a
  person decides, and lands with its checks.
- The governance: every call is judged at its step, every action is in the
  audit chain, and nothing is global (D58).
- Records across servers: Jobs, Audit Log, Costs, Evals and Verification
  read one server or all of them.

### What is hard

- **Ten top-bar pages of equal weight.** Jobs, the daily page, sits beside
  Engine and Verification, which a person opens once a month. The bar shows
  no order: what to set up, what to watch, what to check.
- **Too many words for one idea.** Constraint, capability, permission,
  grant, rule, note, check, hook, reviewer, item, feedforward, feedback,
  rung, layer, harness, factory, dial, lights-on and dark. A new person
  can't tell which are kinds and which are settings.
- **One change, five places.** A step's model is on the step, the server's
  model is in Settings, MCP servers are in Settings but used as grants in
  Permissions, and the factory lines (merge, network, send back) show in
  Permissions but are set on the workspace and the step.
- **The factory lines are global.** They break D58, and the page can't
  change them.
- **Walls of text.** The Workspaces lede is 120 words; most pages explain
  themselves in a paragraph instead of in the controls.
- **No first-run path.** Nothing says "add a workspace, pick a workflow,
  send a request."
- **One theme.** D4 allows only dark, and there is no way to add another.

## The model, in plain words

Each word below is one thing, and the app uses no other word for it.

| Thing | What it is | Where it lives |
|---|---|---|
| **Server** | A Bastion, local or remote. Holds everything below. | The switcher |
| **Workspace** | A repository, its checks, its setup, and how a job lands. | Workspaces |
| **Charter** | A workspace's own rules, agents and skills, in its repository. | On its workspace |
| **Workflow** | Steps on a canvas. Runs on any workspace. | Workflows |
| **Step** | A prompt, an agent, a skill or a script, with who decides. | On its workflow |
| **Guidance** | What the agent is told before it acts: rules, notes, skills, agents. | Harness |
| **Check** | What judges the agent's work: word checks, hooks, reviewers. Blocks. | Harness |
| **Permission** | What a call may do: ask or allowed. | Harness |
| **Tool** | Where calls go: the engine's functions, commands, MCP servers. | Harness |
| **Plugin** | An installed bundle of skills, agents, commands, hooks and MCP servers. | Harness |
| **Request** | What a person asks for. | Jobs |
| **Job** | One request run through one workflow on one workspace. | Jobs |

Guidance, Check and Permission replace feedforward, feedback and the three
tabs' names in the app. The decisions keep the precise terms.

## Hidden things to name

These exist in the code or the decisions but have no name or no home.

- **Request with parts.** A job has `parent`, `part` and `after_job`: a
  request can already split into jobs that run in order. The app shows each
  job alone. Name it a request with its jobs, and show them together.
- **Attempt.** A job retries, revives and sends work back (`back_to`,
  `failures`, `ci_fixes`). Each pass is an attempt with its own turns and
  cost, and the job page should list them.
- **Decision.** A waiting job, a held call, an approval and a charter
  branch to merge are all "a person decides". Name them decisions and put
  them in one inbox.
- **Landing.** How a job ends (merge, pull request, leave the branch) and
  the factory lines are one policy. It belongs to the workspace, with a
  step's override, never to the server.
- **Model choice.** A model is set on the server, the job and the step,
  with a budget beside it. Name it once, and show where it comes from.
- **Tool.** MCP servers, engine functions and commands are what a
  permission points at. Today they live in three places.
- **Lesson.** An escape found after a job landed should become a check, so
  the same miss can't land twice. Verification and the harness meet here.
- **Field note.** What a job taught, signed by a person when it lands or
  stops. Together the notes are the atlas.
- **Outcome.** A job already keeps `criteria`: the summit picked before it
  leaves. The job page should show it first.
- **Experiment.** An eval is a harness change tried on past requests. A
  rule could offer "try this on the last 10 requests" before it goes live.
- **Proposal.** A charter edit is a branch to merge, and a workflow edit is
  a new version. Both are changes waiting to be accepted.

## Agent primitives: parity with Claude Code, kept small

Bastion offers the primitives a coding-agent harness offers, so a team's
setup moves over without a rewrite.

| Claude Code | Bastion | Kind |
|---|---|---|
| `CLAUDE.md`, `AGENTS.md` | Charter `AGENTS.md`, and rules a step is told | Guidance |
| Skills | Skills: a charter's `.agents/skills`, or a plugin's | Guidance |
| Subagents | Agents: a step that runs one, or a reviewer | Guidance, Check |
| Hooks | Hooks before a call, and when a step ends | Check |
| Permissions: allow, ask, deny | Permissions: allowed or ask; no grant refuses | Permission |
| MCP servers | MCP servers, as tools a permission names | Tool |
| Plugins and marketplaces | Plugins: installed per server, then applied | Plugin |
| Output styles | Rules a step is told | Guidance |
| Slash commands | Commands: a charter's or a plugin's, run by a prompt step | Guidance |

**Plugins.** A plugin is a git repository with a manifest that lists its
skills, agents, commands, hooks and MCP servers, as a Claude Code plugin does. A
person installs it on a server, from a URL or a marketplace list, at a
pinned commit. Installing it adds nothing to a job: each part shows on the
Harness page, off, and a person applies it to steps like any other item
(D58). Updating a plugin is a new pinned commit, so a job always records
which version ran. Caveman is the first plugin to try: its skills and its
prompt hook map onto guidance and a hook.

## The look and feel: an expedition

[The software expedition](https://blog.tacoda.dev/the-software-expedition-ca3631d61530)
gives the app its picture. Every landmark in it is already a thing in
Bastion, so the app draws them without new words: a person never has to
say "summit" to use it.

| On the map | In Bastion |
|---|---|
| The outpost | Where requests arrive: Jobs, an issue, a schedule |
| The summit | The outcome a job must reach, picked before it leaves |
| The guide | The harness: models, tools, the loop |
| Base camp | The job's worktree and VM |
| The charter | The workspace's charter (the same word) |
| The checkpoint | A commit the checks passed |
| The range of reality | Failing checks, a reviewer, a person sending work back |
| The final peak | The job lands |
| The track | The turns and the audit log |
| Specimens | The change and its artifacts |
| Field notes | What the job taught, one sentence signed by a person |
| The marsh | A job stopped or failed, said out loud |
| The atlas | Everything learned, where the next request starts |

What this changes in the app:

- **A job is a trail, not a list.** The job page draws the route: the
  workflow's steps as landmarks, each pass back as a peak climbed again,
  each commit the checks passed as a checkpoint. Where the job is now is a
  marker on the trail.
- **Workflows are routes.** Bastion's difference is many workflows, so the
  Map draws them as routes across a server's ground, from the outpost to
  the final peak, and a request picks its route at the outpost.
- **Field notes are a new thing.** When a job lands or stops, a person can
  sign one sentence: what reality said. A stopped job asks for one, so the
  marsh is said out loud. Notes feed the atlas, the start of the next
  request.
- **A third theme, `bastion-chart`.** The old-world chart's parchment and
  ink, for people who want the map to look like one. Ink #3b2a1a, paper
  #efe4c8, red #9b2c1f.
- **Tactile.** Landmarks are things to press: hover lifts them, a step on
  the trail opens in place, and a route can be followed with the keys.

## The layout

### The bar

Seven entries instead of ten, in the order a person uses them. The server
switcher stays where it is and works as it does now.

`Jobs · Map · Workflows · Workspaces · Harness · Insights · Settings`

- **Jobs** is home: the inbox, a new request, and every job.
- **Map** shows what each server runs, as a picture.
- **Workflows**, **Workspaces** and **Harness** are where a person sets
  things up.
- **Insights** holds the records: Audit Log, Costs, Evals, Verification.
- **Settings** holds this app's settings (its theme and its servers) and
  the server's: people, models and providers, notifications, settings
  history, and the engine's health.

### Jobs: where the day goes

- **New request** stays a page of its own: it gathers a lot up front.
- **Decisions first**: each job waiting on you, each held call, each branch
  to merge, with its one button (Approve, Allow, Refuse, Merge).
- **In progress** and **Finished** below, as now.
- On All servers, each row names its server, as now.

### A job: a run you can read

- The workflow drawn as progress at the top, the current step marked.
- One line under it says what the job waits for and offers its action.
- Tabs: Steps (each step's turns and artifacts), Changes (the diff), Rules
  (what applied at each step, read-only), Cost, Audit.
- **Change it from here.** A refused call offers "Allow this at this step".
  A step offers "Edit this step" and "Add a rule here". Each opens the right
  editor with the step already picked. This is how a person makes a
  specific change without knowing where it lives.

### Map: every piece, at a glance

- One column per server on All servers, else one server.
- Workspaces on the left, each with a charter mark. Workflows in the
  middle, each with its steps. The harness on the right: guidance, checks,
  permissions and tools.
- A line joins an item to each step it applies at, so "what applies where"
  is a picture, not a list.
- Clicking any node opens its editor in a side panel.
- Later: shade each step by its recent jobs, where they wait and where
  they fail.

### Harness: one page for what governs an agent

- One list with two filters: kind (Guidance, Check, Permission, Tool) and
  source (Bastion, or a workspace's charter).
- Each item shows where it applies as chips: one step, a workflow, some
  workflows, all workflows. Clicking the chips opens the picker.
- Promote and demote stay on the item: Guidance or Check, Ask or Allowed.
- MCP servers move here from Settings, as tools.
- Plugins sit here too: Install, then each part listed under its plugin,
  off until applied. Update and Remove are on the plugin.

### Workspaces

- One card each: repository and branch, checks, landing, and its charter
  with Edit.
- The settings form opens in a side panel, not under the list.
- The lede shrinks to one sentence; each field explains itself.

## Phases

Each phase passes CI, gets a live check in the app, and lands in a commit
before the next one starts.

### L0. Mockup

- A clickable mockup of the bar, Jobs, a job, the Map and the Harness, in
  light and dark, before any code changes.

### L1. Words and theme

- A glossary in DECISIONS.md, and every page's text moved to the words in
  the table above.
- Themes: about 230 color uses in 32 files read constants today. They
  become a named palette picked at run time, chosen in Settings and kept by
  the app. The first two are `bastion-dark` and `bastion-light`; more can
  be added later. D4 changes.
- Shorter ledes on every page.

### L1b. The chart theme

- `bastion-chart`, beside the first two.

### L2. The bar, done

- Six entries now, the Map joins in L5. Insights takes Audit Log, Costs,
  Evals and Verification as tabs. Settings takes Engine as Health.
- The Harness page replaces the three rules tabs, with the kind filter.

### L3. Jobs as home

- The decisions inbox: waiting jobs, held calls and charter branches to
  merge. New request stays its own page.

### L4. The job page

- Progress first, one action line, the tabs, and "change it from here".
- Attempts listed, and a request's parts shown together.

### L5. The Map, first pass done

- Done: a chart per server. Workspaces are harbor towns on the coast,
  workflows are routes through their steps to a summit, a red seal counts
  the rules on at each step, and pressing a landmark lists what applies.
- Next: edit in place from the list, and draw the harness items and
  plugins on the chart.

### L6. Landing per workspace

- The factory lines move to the workspace, with a step's override (D58).

### L7. Plugins

- Install a plugin from a git URL at a pinned commit, on one server. Its
  skills, agents, commands, hooks and MCP servers show on Harness, off.
- Each job records the plugin commits it ran with.

### L8. First run

- A checklist on Jobs until it's done: add a workspace, check a model,
  send a first request.

### X. Exploration: learning, discovery, feedback

Next, notes become charter changes (D61, replacing D59's atlas):

- Done: signing a note asks its kind: a lesson, a fact, or about Bastion.
  A lesson goes to `.agents/rules/<name>.md`; a fact goes to
  `.agents/corpus/<name>.md` with its paths inferred from the journey's
  change and the paths the note names. Each is committed on a branch of
  its own for a person to merge.
- Done: a lesson can be a hook or a permission (allow, ask, deny),
  merged into `.agents/settings.json` with all it already holds.
- Done: a note signed while the journey is under way is written onto its
  own branch when it lands; after it came home, on a branch of its own.
- Done: the corpus is loaded by path: when an agent opens a path a corpus
  entry names, it is given that entry, once per turn.
- The Map's harbor town shows the region's rules and corpus, read from
  the repository.
- Done: a note about Bastion appears on the Guide under "Learned on
  journeys"; one press makes it guidance on the trail its journey took.
- Next: offer a check as well as guidance, at the waypoint it came from.

The app should feel like an expedition, and improve itself from what jobs
teach.

- Done: the trail's way home to the atlas, field notes about the
  workspace or Bastion (D59), and the marsh on every trail.
- Done: on the Map, each harbor town opens its workspace's atlas, with
  its notes about the workspace and about Bastion.
- Done: a trail's wear on the Map: its journeys, at the summit and in the
  marsh, and at each waypoint the journeys waiting there and those that
  ended there.
- A lesson from the atlas: a Bastion note offers "make this a check" or
  "add this as guidance", applied at the step it came from.
- Done: everything clickable lifts a little on hover, as the landmarks do.
- Words: a workflow is a trail; a job is a journey along it.
- Done: a legend along the foot of the Map.

## Open questions

- **Plugin hooks that need the host.** A Claude Code hook runs on the
  person's machine; a Bastion hook runs in the job's VM. A plugin hook that
  expects the host's files or tools may not work there.
- **Stdio MCP servers.** Should one run in the job's VM, so a step may use
  a local MCP server without breaking the VM rule?
- **Codified and ambient context.** Guidance and checks are codified
  context: written down, stable, and Bastion can scope and judge them.
  Ambient context comes from MCP servers and other outside sources: it is
  scattered, it changes over time, and it depends on state. How should
  Bastion handle it: where it came from, how fresh it is, and when to read
  it again? Today an MCP server is only a grant.
