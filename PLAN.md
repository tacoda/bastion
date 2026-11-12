# Bastion plan: nothing is global

<!-- voice-register: informational -->
<!-- voice-english: ste -->

Bastion manages a harness and a factory on each server: rules, agents,
skills, hooks, reviewers and grants. Each item applies only where you
select it: one step, a workflow's steps, some workflows, or all workflows
(D58). A workspace's charter brings its own items for its jobs, and you
edit them in the app; an edit is a commit on a branch, never on the base.
Every page shows all servers or one. The cost is the old default: a list
that selected nothing meant every step, so migration 10 writes that down
as all workflows before the default turns off.

The rules-pages plan (R1, R2, P1, P3) is done, and git history holds it.
Decisions are in [DECISIONS.md](DECISIONS.md), D54 to D58.

## Phases

Each phase passes CI, gets a live check in the app, and lands in a commit
before the next one starts.

### S1. Nothing is global

- An item with no selection applies nowhere, in core and in the desktop.
  A narrower setting no longer writes down what all workflows had.
- Migration 10 selects all workflows in each list that selected nothing,
  so each item works as it did.
- A new item made at all workflows carries `*`. A charter's rules and an
  approved test carry `*` for their job's steps.
- The Global view is named All workflows.

### S2. Two homes: Workflows and Workspaces, done

Everything a job runs under belongs to a workflow or to a workspace, so
the top bar loses Constraints, Capabilities and Permissions.

- Workflows gets tabs: Steps, Constraints, Capabilities and Permissions.
  The rules pages become its tabs, unchanged.
- A Workspaces page joins the top bar, with a module of its own. It holds
  each workspace's settings and its own charter, or "No charter", read-only
  for now: AGENTS.md, its allow and ask rules, its hooks, its skills and
  agents. The accounts move with it.
- Settings keeps what belongs to the server itself: servers, people,
  model providers, MCP servers, the factory, notifications and history.

### S3. Workflows and Workspaces across all servers, done

- On All servers, Workspaces and the three rules tabs show one section per
  server, named. A change goes to its section's server.
- A workflow's Steps tab stays on one server: a canvas edits one workflow.

### S4. Charter edits

- Edit a charter file in the app. The server commits the edit on a new
  branch of the workspace's repository, never on its base branch.
- An agent's turn still may not change the charter.

## Open questions

- **Plugins.** iii has no plugin. Is a plugin a bundle of the above (an
  agent, its skills, its hooks and its MCP servers) that a step takes as
  one?
- **Stdio MCP servers.** Should one run in the job's VM, so a step may use
  a local MCP server without breaking the VM rule?
- **Codified and ambient context.** Constraints and direction are codified
  context: written down, stable, and Bastion can scope and judge them.
  Ambient context comes from MCP servers and other outside sources: it is
  scattered, it changes over time, and it depends on state. How should
  Bastion handle it: where it came from, how fresh it is, and when to read
  it again? Today an MCP server is only a grant.
