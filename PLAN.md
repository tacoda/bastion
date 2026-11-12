# Bastion plan: rules by who judges them

<!-- voice-register: informational -->
<!-- voice-english: ste -->

A step's rules sort on three questions. Is it a constraint or a
capability? Is it feedforward, which guides the agent before it acts, or
feedback, which checks what it did? And does it come from the
workspace's repository or from Bastion? Each item is on or off at a step,
or for every step of its workflow. Told, checked and enforced go: a
feedforward rule is said to the agent, a feedback one always blocks, and
a grant keeps ask or allowed. The cost is the old "checked" rules, which
noted a break and let it pass; they start off, so nothing changes until
you turn one on.

The scopes plan (G1 to G3) is done, and git history holds it. Decisions
are in [DECISIONS.md](DECISIONS.md), D54 to D56.

## Phases

Each phase passes CI, gets a live check in the app, and lands in a commit
before the next one starts.

### R1. The model on the server (D56)

- A constraint is feedforward (stored as told) or feedback (stored as
  enforced). A reviewer is feedback: it reads what the step did. Migration
  9 turns each checked one into feedback, off for every workflow.
- A constraint row may be a capability: a note told to the agent as what
  it may do, such as a skill to use. The brief lists them under "You may".
- The charter listing adds the repository's skills and agents, which a
  workspace brings as feedforward capabilities.
- Setting a mode refuses checked.

Done 2026-11-12. Live, migration 9 turned the three checked rules (tests
kept, assertions kept, no skipped tests) into feedback that is off, as
`!*`. The charter listing shows demo-local's agent `strict`. The brief
lists "You may" before its rules.

### R2. The rules view by type and mode

- Each level shows Constraints and Capabilities, each split into
  feedforward and feedback, with a Workspace or Bastion chip on every row.
- A Bastion rule has a Feedforward | Feedback switch; going to feedback
  asks for its form, as D55 says. A grant keeps Ask | Allowed.
- The levels are the workflow and the step. What applies to every
  workflow shows as inherited at the workflow; editing it for all
  workflows waits.

### R3. Docs

- VISION.md and DECISIONS.md say how rules sort.

## Open questions

- **Plugins.** iii has no plugin. Is a plugin a bundle of the above (an
  agent, its skills, its hooks and its MCP servers) that a step takes as
  one?
- **Stdio MCP servers.** Should one run in the job's VM, so a step may use
  a local MCP server without breaking the VM rule?
- **Rules for every workflow.** Whether some items should be set once for
  all workflows, and where that lives.
- **Codified and ambient context.** Constraints and direction are codified
  context: written down, stable, and Bastion can scope and judge them.
  Ambient context comes from MCP servers and other outside sources: it is
  scattered, it changes over time, and it depends on state. How should
  Bastion handle it: where it came from, how fresh it is, and when to read
  it again? Today an MCP server is only a grant.
