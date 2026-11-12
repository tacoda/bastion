# Bastion plan: rules at three scopes, in two forms

<!-- voice-register: informational -->
<!-- voice-english: ste -->

A constraint or a grant is one item, and three scopes turn it on or off:
all workflows, one workflow, and one step. The most specific setting wins,
so a rule can be on for all workflows, off for one workflow, and on again
at one of its steps. A rule also has two forms. An agent judges a told rule
or a reviewer's verdict; code judges a word match, a hook, or a review
check. Promoting a rule moves it from the first form to the second. The
cost is reading: each row now says where its setting comes from.

The capabilities plan (C1 to C4) is done, and git history holds it.
Decisions are in [DECISIONS.md](DECISIONS.md), D54 and D55.

## Phases

Each phase passes CI, gets a live check in the app, and lands in a commit
before the next one starts.

### G1. Scopes in the core and the server (D54)

- A constraint's and a grant's scope list holds settings: `*` for all
  workflows, `bug-fix` for one workflow, `bug-fix/build` for one step, and
  `!` before any of them for off.
- At a step, its own setting wins, then its workflow's, then the all
  workflows setting. A list with no `*` keeps today's meaning: on at every
  step when it names no step to turn on, and on only where it says
  otherwise.
- A grant is an item for one kind and one function, with its rung. A
  step's own grants no longer replace its kind's: a step turns items on or
  off. The gate folds the few step grants from before into items once.
- `PUT /scopes` sets an item's setting at a scope: on, off, or inherit.

Done 2026-11-12. Live, the gate folded the one step grant from before,
Bug fix's plan step's `sandbox::*`, into the prompt kind's item as
`* bug-fix/plan`, and the audit kept the fold. `PUT /scopes` turned "Keep
shell use small" off for Bug fix, as `* !bug-fix`, and back to inherit.

### G2. Levels on the Workflows page

- A level bar: All workflows › the workflow › the step. Picking a step on
  the canvas opens its level, and the canvas background opens the
  workflow's.
- Each level shows the same Rules view: the charter, the harness and the
  factory. Each item has an Inherit | On | Off switch, says what it
  inherits, and keeps its rung switch.
- The Add bar adds an item on at the level it is on.

Done 2026-11-12. Live, at Bug fix "Keep shell use small" went Off, as
`* !bug-fix`; Bug fix's build step then showed it as "Inherit · off", and
On there gave `* !bug-fix bug-fix/build`. At All workflows the canvas
hides and each item shows On | Off, with Remove. A "here" label sets the
scope switch apart from the rung switch beside it.

### G3. One rule, two forms (D55)

- A constraint shows who judges it: an agent (a told rule, a reviewer) or
  code (a word match, a hook, or a review check).
- Promoting a told rule asks for its deterministic form: the words a call
  holds, or a hook command. Demoting keeps that form and stops running it.
- A grant's rung stays ask or allowed.

## Open questions

- **Plugins.** iii has no plugin. Is a plugin a bundle of the above (an
  agent, its skills, its hooks and its MCP servers) that a step takes as
  one?
- **Stdio MCP servers.** Should one run in the job's VM, so a step may use
  a local MCP server without breaking the VM rule?
