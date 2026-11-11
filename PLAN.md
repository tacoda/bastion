# Bastion plan: one bar, every server

<!-- voice-register: informational -->
<!-- voice-english: ste -->

The sidebar goes. A bar across the top holds the castle, the pages, and the
server switcher. The switcher picks one server, or all of them on the pages
that show records: Jobs, Audit Log and Costs. Those pages then add up what
every server holds. The pages get one clean layout, and the records get
graphs. The cost is width: ten pages and a switcher must fit in one row, so
the page names get short and the bar wraps on a narrow window.

The steps-only plan (S1 to S4) is done; git history holds it. Decisions are
in [DECISIONS.md](DECISIONS.md), and VISION.md says which pages are records
and which are rules.

## Phases

Each phase passes CI, is checked live in the app, and is committed before
the next.

### U1. The top bar

- Remove the sidebar. The bar shows the castle, the page names, the count
  of jobs that wait, and the switcher.
- The switcher lists each server with a dot for its stream, and "All
  servers" on a records page.
- If "All servers" is picked and you open a rules page, the page shows the
  last server you picked alone.
- The Jobs page follows the switcher, and its own server filter goes.
- Exit, live: switch between `local` and all servers on Jobs, and open a
  rules page from "All servers".

Done 2026-11-11. The bar holds the castle, the ten pages with the count of
jobs that wait, and the switcher as one button per server. Live: Jobs
switched from all servers to `local` and back, and Workflows opened from
"All servers" showed `local` alone.

### U2. Records across servers

- Audit Log and Costs read every server when "All servers" is picked.
- Each entry, turn and tally names its server. Costs add up by workspace,
  step and day across servers.
- A server that does not answer is named, and the rest still show.
- Exit, live: Costs and Audit Log show `local` and a second server
  together.

Done 2026-11-11. The Audit Log is its own page now; Constraints still reads
one server's audit for each constraint's history. Live: with `two` added,
Costs showed both servers' totals, a workspace named with its server, and
the days added up; the Audit Log showed both chains verified and their
entries in one list. An export writes on one server, so it waits for one.
The run found that opening a records page read only one server, because
the page read under the route it left; that is fixed.

### U3. Graphs

- Costs: spend by day as bars, each bar split by server.
- Jobs: how many jobs are in each state, and jobs started by day.
- Audit Log: entries by day, split by kind, with the gate's denials marked.
- Graphs are drawn on iced's canvas, with no new dependency.
- Exit, live: each graph matches the numbers in its page's table.

Done 2026-11-11. One stacked bar chart over the last 30 days, with a
legend and a hover line that names the day and each part, serves all three
pages; Jobs and the Audit Log also get number tiles. Live: on Costs, the
tallest day reads $0.1937 as its table does, and hovering 11-06 reads
$0.0658, local $0.0628 and two $0.0029. The Audit Log's tiles read 206
entries, 1 denial and 21 decisions, as its tallies do. Its chart covers
only the entries the page read, the newest 200 per server, and says so.

### U4. Clean pages

- One header on every page: the title, one line that says what the page
  is, and the page's actions on the right.
- Cards with the same padding and spacing everywhere; tables with aligned
  columns.
- Page by page, starting with Jobs and the job page.
- Exit, live: every page is checked at the window's smallest size and at
  full screen.

Done 2026-11-11, in a first pass. Every page has one head: its title, a
line that says what it shows and on which server, and its actions. A
problem line shows only when there is one, so an empty one leaves no gap.
Verification gets tiles for jobs landed, escapes and the rate; a job's
title is its request's first line, with the request in full under it. The
ledes that still named stages, pipelines or review are rewritten. Live:
every page checked at full size, and Jobs at 720 by 480, where the bar
wraps to two rows and nothing is cut.

### U5. Evals and Verification across servers

- Evals and Verification are records pages too, and stay in the bar.
- Each eval and each landed job names its server and opens on it.
- A new eval runs on the last server picked, and the form names it.
- An escape goes to the server that landed its commit, so it can be
  reported from "All servers".
- Exit, live: both pages show `local` and `two` together.

Done 2026-11-11. Live, under "All servers": Evals listed each eval with its
server and opened one on `local`, and Verification named each workspace
with its server and added up 17 landed and 2 escapes. `two` has no eval
and no landed job yet, so its rows could not show; the merge is covered by
tests. A turn's steps on Costs now open on the turn's own server too.

### U6. Eval charts: A against B

- An eval's page compares A and B in total, measure by measure: grade,
  cost, seconds, turns, calls, refused, held and flagged. Each pair is
  scaled to its larger value and says how B differs from A.
- Grade, cost, time, turns and calls are colored better or worse.
  Refusals, holds and flags are not, as an enforced rung is meant to raise
  them.
- A second chart compares each request's grade under A and B.

Done 2026-11-11. Live on "no-wc told vs enforced": B cost 40% less, took
25% less time, 28% fewer turns and 48% fewer calls, refused 5 calls to A's
0, and graded the same, 7. The run found that a change from zero read
"same" and that a zero drew a sliver of bar; both are fixed.

### U7. A model and a budget per step (D43)

- A step's options: its model, the most revisions its failure sends back,
  and the most its turns may spend.
- The step's model wins over the job's and the server's.
- Past its spend, the factory starts no more turns at the step by itself;
  a person's retry, note or answer still runs.
- The workflow editor shows the three options on a step.

Done 2026-11-11. Live: a copy of Short kept its options through a save
(plan with a model and $0.50, checks with one revision). With the plan
step set to `anthropic/claude-haiku-4-5-20251001` and the server's default
on DeepSeek, the plan turn ran on Anthropic, as its session records; a
first run with DeepSeek on the step landed `quadruple(x)` on `demo-local`.
The spending limit and the revision count are covered by tests, not run
live. The user-facing word "stack" is now "engine" everywhere.

### U8. Rules per workflow step (D44)

- A step is known as `workflow/step`, and each turn tells the gate which.
- A grant may be for one workflow step; it wins over its kind's grant.
- A constraint may apply at some steps only; with none, at every step.
  An agent is told only the rules that apply at its step.
- Capabilities lists the steps' own grants with a form to add one, and
  each constraint shows its steps with a picker to add one.

Done 2026-11-11. Migration 6 rebuilt the grants table with the step in
its key. Live: with `sandbox::fs::write` denied at the test workflow's
build step, the gate refused the build's two writes there; the agent then
wrote through `sandbox::exec`, which its kind still allows, and the job
landed. So a step's grant binds only the function it names: to stop all
writes at a step, deny `sandbox::exec` there too. A refusal now names the
step. The grant showed under "For one workflow step" and its Remove took
it back.

### U9. No command steps (D45)

- A step is a prompt, an agent, a skill or a shell script.
- Migration 7 turns each command step into a prompt that reads its
  command's file, and the grants for commands go.

Done 2026-11-11. Each rebuild reads the old stage `command` as its own
time meant it: a shell step before migration 4, a repository's command
after. Live: a workflow saved with a command step before the restart came
back after migration 7 as a prompt that reads
`.claude/commands/release-notes.md`, its own words after; no command row
was left.

### U10. The charter is AGENTS.md and .agents/ (D46)

- The charter is `AGENTS.md` and `.agents/settings.json`; `CLAUDE.md` and
  `.claude/` are no longer read, and a turn may not write `AGENTS.md` or
  anything under `.agents/`.
- An agent step reads `.agents/agents/<name>.md`, a skill step
  `.agents/skills/<name>/SKILL.md`.
- The harness, the rules on an agent's calls, stays Bastion's own.

Done 2026-11-11. Live: with `AGENTS.md` and `.agents/settings.json`
committed to `demo-local`, the server read both as its charter and its
rule that denies `git push`. A workspace with only `.claude/` files, such
as `bastion-gh`, shows no charter until it has `.agents/`.

### U11. Workflows that branch (D47)

- A link may be a failure link. When its step fails, the job goes along
  it instead of back to the step that writes.
- A failure link orders nothing, so it may go back; a step only failure
  links reach is skipped otherwise.
- The failing step's revisions bound a loop; past them a person decides.
- The canvas draws a failure link red and dashed; a picked link has an
  "On failure" box.

Done 2026-11-11. Live, on a copy of Short whose checks exit 1 and whose
failure link goes to a lights-on `report` step: plan and build ran, the
checks failed, the job went to `report` instead of back to build,
`report` ran, and the job waited there; it was stopped so nothing
landed. The canvas drew the link red and dashed.

Known gap: the job's steps show a step that failed and branched as done,
as the job keeps no outcome per step.

## Open questions

- **Where the switcher sits on a narrow window.** The bar wraps. Should the
  switcher stay on the first row instead?
