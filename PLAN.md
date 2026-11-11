# Bastion plan: a workflow is its steps

<!-- voice-register: informational -->
<!-- voice-english: ste -->

A job runs its workflow's steps in order, and nothing else. A step is one of
five kinds, and each kind has one text to edit. Bastion seeds the common
workflows with that text filled in, and the owner can change all of it. The
stage concept goes away: no step has behavior that its text does not say.
The cost is the safety the built-in stages gave. Landing, pull requests,
merges, quality gates and acceptance tests become text the owner can read
and change, not code Bastion enforces.

Decisions are in [DECISIONS.md](DECISIONS.md) and cited as `DECISIONS.md
D<n>`. D39 to D42 below are new; the stage decisions they replace say so.

## The decisions

- **D39. A workflow is a series of steps.** A step is a prompt, an agent
  (`.claude/agents/<name>.md`), a skill (`.claude/skills/<name>`), a command
  (`.claude/commands/<name>.md`), or a shell script. Each has a dial (a sun
  or a moon), the results it gives, and the results linked into it. A job
  ends when its last step passes. This replaces the stages in D18 and D35.
- **D40. A shell step runs in the job's VM.** By default the VM has no
  network. A step can ask for the network and name the vault's secrets it
  gets as environment variables, such as `GH_TOKEN`. Code from the job's
  branch never runs on the host.
- **D41. A job ends with its branch ready, and the workspace says what
  happens next.** The VM's commits come back to the job's branch on the host
  after every step. When the last step passes, a workspace set to merge
  merges the branch into its base on the host; a workspace set to leave it
  keeps the branch. A pull request is a shell step that pushes and runs
  `gh pr create`.
- **D42. Governance is per step.** A step either may change files or only
  reads. The capability ladder applies to a step's kind, and a step can
  narrow it. Dials, notices and evals name steps, not stages.

## Phases

Each phase passes CI, is checked live, and is committed before the next.

### S1. Steps run on their own

- Shell steps ask for the network and name secrets (D40); the factory
  makes the VM to match.
- A job whose workflow has only the five kinds runs to its end, then
  finishes by the workspace's setting (D41). The landing check that wants a
  land stage accepts such a workflow.
- The workspace gets its finish setting: merge, or leave the branch.
- Exit, live: a workflow of a planning prompt, a building prompt, a test
  shell step and a network shell step that opens a pull request on
  `ianparo/bastion-demo`.

Done 2026-11-11. On `demo-gh`, a workflow of a planning prompt, a
building prompt, a test shell step and a network shell step ran to its
end. Plan gave its `plan` result, build committed, the test step failed
on the repository's own broken test and was shipped anyway, and the
network step pushed with `GH_TOKEN` from the vault and opened PR #2 on
`ianparo/bastion-demo`. The job then finished, and the workspace left its
branch. The run found that an approved finish left the job waiting; that
is fixed. The test workflow's own script misread GitHub's reply.

### S2. The seeded workflows are steps

- Each seeded workflow becomes the five kinds, with its text filled in.
- The plan and build prompts start from today's briefs, and review is a
  prompt. The pull request, the CI wait and the merge are `gh` shell scripts.
- "Runs as" lists only the five kinds.
- Exit, live: the seeded short workflow lands a change on `demo-local`, and
  the seeded pull request workflow opens and merges a pull request.

Done 2026-11-11. Short, Pull request, Full and the full workflow without
acceptance tests ship as steps, saved at startup while no workflow has
their id; the eleven older seeded workflows turn into steps while their
first version is unchanged. Live: Short landed `negate(x)` on
`demo-local`, and Pull request planned, built, passed its checks, opened
PR #3 on `ianparo/bastion-demo` and merged it. The run found that the
sandbox stops a command after five minutes, so the CI step waits four and
a half and then asks for a retry; GitHub's own CI for the PR failed, and
the step was shipped anyway to merge. It also found that a failed store
transaction was silent, that a deleted workflow's next version collided
with its kept ones, and that a result said again empty dropped the first;
all three are fixed.

### S3. Everything names steps

- Governance per step (D42), dials per step only, notices and the job
  page by step id, evals by step.
- Exit, live: a read-only step's write is refused and noted in the audit.

Done 2026-11-11, in part. An agent's step can only read: the gate refuses
its file writes, and the seeded planning and criteria steps read only. A
notice names a workflow's step. Live: a read-only prompt told to write
`NOTES.md` on `demo-local` was refused, and the audit log reads "deny
(This step only reads: it may not write a file.) sandbox::fs::write
/work/repo/NOTES.md". The settings' dials per stage and the evals' arms
per stage move to S4, as they reach only the stage code it deletes.

### S4. The stage code goes

- Delete the built-in stages and the pipelines.
- Delete the features that read a stage's output. These are scope splits,
  discover questions, the acceptance tests, UAT sign-off, learn, escapes,
  and the CI and deploy watches.
- A migration moves stored jobs and workflows to steps.
- DECISIONS.md marks each replaced decision, and the README says what
  Bastion is now.
- Exit: no `Stage` in the code but the five kinds, and every CI step passes.

Done 2026-11-11. The pipelines, the stage machine, scope splits, discover
questions, acceptance tests, UAT sign-off, learn, the presets, the forge
setting and the CI and deploy watches are gone. Escapes stay: a revert of
a landed commit still counts against its job. Migration 5 deletes the
nodes of stages Bastion ran itself, with their links and results, and
makes every other stage a prompt; a stage that only read becomes a prompt
that only reads. Each artifact now names the step it came from, so the job
page shows a step's own reports. Live: the dev database moved to version 5
with only prompt and shell nodes left, and a Short job landed
`triple(x)` on `demo-local`. The run found that a build's diff took the
next step's kind, and that resetting a shipped workflow left it gone until
a restart; both are fixed.

## Open questions

- **What learn and escapes become.** Both read a landed job's history. They
  could come back as a seeded prompt step that reads the job's record.
</content>
</invoke>
