# Bastion plan: more a step may do, more that checks it

<!-- voice-register: informational -->
<!-- voice-english: ste -->

A step's capabilities grow past iii functions. It may run named shell
commands, call more tools, and reach MCP servers. Its constraints grow past
rules: hooks run before its calls, and a reviewer agent must pass its change
before the job moves on. The cost is reach. Each new capability is a new
way for an agent to act, so each one starts ungranted and the gate decides
every call, the same as today.

The top-bar plan (U1 to U13) is done, and git history holds it. Decisions
are in [DECISIONS.md](DECISIONS.md), D49 to D52.

## Phases

Each phase passes CI, gets a live check in the app, and lands in a commit
before the next one starts.

### C1. Named commands and more tools (D49)

- A grant on `sandbox::exec` may name a command after the function:
  `sandbox::exec cargo test`. That grant covers a call only when the
  command line starts with those words.
- A grant that names a command never covers a line with shell operators
  (`;`, `&&`, `|`, `` ` ``, `$(`, `>`), because the operators would run a
  second command. A grant that names no command still covers every line.
- The most specific grant decides, as today: a named command wins over
  plain `sandbox::exec`.
- A turn's `functions.allow` is the four sandbox tools plus every function
  its step's grants name. Today it is the four tools only, so a grant on
  any other function can never be called.
- The step editor's grant form takes the command words after the
  function. The command lives in the grant's function, so the store and
  the API don't change.

Done 2026-11-11. Live, on Short's plan step with its own grants
`sandbox::fs::*` and `sandbox::exec git`: `git -C /work/repo log
--oneline -3` ran, and `ls -la /work/repo` was refused with "No grant
covers sandbox::exec at short/plan." The four sandbox tools are in every
turn's list by name, so this did not show whether the harness takes a
pattern such as `sandbox::*`. C4 checks that with the MCP tools.

### C2. Hooks per step (D50)

- A constraint may be a hook: a command that runs in the job's VM before
  each matching call at its steps. Exit 2 refuses the call, and its output
  is the reason.
- The charter's `PreToolUse` hooks still run. A step's hooks run after
  them.
- The Constraints section of the step editor adds and removes a hook. A
  hook that is only told doesn't run, and only an owner's constraint can
  be removed: a charter's changes in its repository.

Done 2026-11-11. Live, a hook at Short's plan step that exits 2: the turn's
`git log` was refused with "A hook refused this call: hook says no", and
the audit kept the hook's run. In the app, Bug fix's build step added
`./.agents/hooks/check.sh` as a hook and removed it again; the audit
recorded both.

### C3. A reviewer that must pass (D51)

- A constraint may name a reviewer: an agent in `.agents/agents/`. When
  its step finishes, Bastion runs one turn as that agent on the step's
  change.
- The reviewer answers pass or fail with its reasons. A fail fails the
  step, so the job goes along the failure link or back to the fixer, as
  D47 says. The step's revisions bound the loop.
- The reviewer's turn has no tools: it reads its agent file as the job's
  base has it, the request, and the change, on the server's plan model. A
  turn under review can't rewrite its reviewer.
- Its reply is an artifact on the step, and its cost counts toward the
  step's budget. A reviewer that can't run, or gives no verdict, leaves
  the job with a person.
- The step editor adds a reviewer and removes one made for that step.

Done 2026-11-11. Live, a `strict` agent that fails every change, on Short's
plan step: the job waited with "The reviewer strict failed it", and the
review was kept as "Review by strict". A plan step only reads, so a fail
there has no step to go back to and waits for a person. In the app, Bug
fix's build step added `strict` as a reviewer and removed it.

### C4. MCP servers (D52)

- iii ships no MCP bridge; its docs say to write a worker that bridges MCP
  into `trigger()` calls. `workers/bastion-mcp` is that worker, in Rust
  (D6).
- A server lists its MCP servers in Settings: a name, a URL, and the name
  of the secret that holds its token, if it needs one.
- The bridge registers each tool of a server as `mcp::<server>::<tool>`.
  A step's grant on `mcp::github::*` lets its agent call that server's
  tools, and C1 puts them in the turn's `functions.allow`.
- Only MCP servers that answer over HTTP, this round. A stdio server runs
  as a process on the host, outside the VM, and VISION.md says agent work
  never runs on your machine.

## Open questions

- **Plugins.** iii has no plugin. Is a plugin a bundle of the above (an
  agent, its skills, its hooks and its MCP servers) that a step takes as
  one?
- **Stdio MCP servers.** Should one run in the job's VM, so a step may use
  a local MCP server without breaking the VM rule?
- **Patterns in `functions.allow`.** I don't know yet whether the harness
  takes `mcp::github::*` or needs each function by name. C1 checks it live,
  and if it needs names, Bastion expands the pattern from the engine's
  function list.
