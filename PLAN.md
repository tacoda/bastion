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

### U3. Graphs

- Costs: spend by day as bars, each bar split by server.
- Jobs: how many jobs are in each state, and jobs started by day.
- Audit Log: entries by day, split by kind, with the gate's denials marked.
- Graphs are drawn on iced's canvas, with no new dependency.
- Exit, live: each graph matches the numbers in its page's table.

### U4. Clean pages

- One header on every page: the title, one line that says what the page
  is, and the page's actions on the right.
- Cards with the same padding and spacing everywhere; tables with aligned
  columns.
- Page by page, starting with Jobs and the job page.
- Exit, live: every page is checked at the window's smallest size and at
  full screen.

## After this plan

- A model per step, chosen in the workflow editor.
- Workflows that branch: a step can name a step to go to on failure, so a
  workflow draws as a flowchart. Today a failure waits for you or goes back
  to the step that writes. This changes the core, so it needs a decision
  first.

## Open questions

- **Where the switcher sits on a narrow window.** The bar wraps. Should the
  switcher stay on the first row instead?
