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

## After this plan

- A step is named `workflow/step`, and constraints and capabilities apply
  per workflow step, not per kind of step. This changes D42 and the gate.
- A model and a budget per step, chosen in the workflow editor: the most
  revisions a step may take, and the most it may spend.
- Workflows that branch: a step can name a step to go to on failure, so a
  workflow draws as a flowchart. Today a failure waits for you or goes back
  to the step that writes. This changes the core, so it needs a decision
  first.

## Open questions

- **Where the switcher sits on a narrow window.** The bar wraps. Should the
  switcher stay on the first row instead?
