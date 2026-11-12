# Bastion's vision

<!-- voice-register: informational -->
<!-- voice-english: ste -->

Bastion is an agent control plane. On each server, it manages a harness and
a factory: the rules, agents, skills, hooks and grants an agent works under,
and the workflows that run it. You send a request, and Bastion runs it as a
job through a workflow. The cost is plain: a job does only what its workflow
says, so a check you take out of a workflow is gone. Bastion runs on your
machine, on a remote server, or on both.

## Jobs and workflows

A workflow is a series of steps on a canvas. A step is a prompt, one of the
repository's agents or skills, or a shell script, and each step has one
text to edit. Each step also carries its own settings:

- **Its dial:** a sun when a person decides, a moon when it runs on its
  own.
- **Its model and its budget:** how often its failure may send the work
  back, and the most its turns may spend.
- **Its rules:** the constraints its agent works under, and the calls the
  agent may make.
- **Its links:** the results it passes on, and the step a failure goes to.

A job is one request run through one workflow. It runs the steps in order,
waits where a step's dial says a person decides, and ends with its branch
ready, merged or left for you. Bastion ships the common workflows with their
text filled in, and you can change every step of them.

## The harness and the factory

Other agent harnesses keep rules, agents, skills and hooks in files.
Bastion keeps the same items on the server, and many of them already work:
rules, checks, hooks, reviewers, notes and grants. You manage each one in
the app:

- **Promote or demote it.** A rule moves between feedforward, told to the
  agent before it acts, and feedback, a check, hook or reviewer that judges
  what the agent did. A grant moves between ask and allowed.
- **Turn it on or off.** One switch, and the item stays as it is.
- **Apply it.** Select one step, every step of a workflow, some workflows,
  or all workflows.

Nothing is global. "All workflows" is a selection, the same as one step,
and an item applies only where you selected it. An item with no selection
applies nowhere. So a step's Rules tab shows everything that applies at it,
and nothing applies from somewhere you can't see.

## A workspace's charter

A repository can bring its own items in its charter: `AGENTS.md` and
`.agents/`, with its rules, agents, skills and hooks. These items belong to
the project, so they apply only to jobs in that workspace. A step can use
them, and you read and edit them in the app the same way as Bastion's own.
An edit in the app changes the file in the repository. An agent's turn may
read the charter but may not change it.

## Every page: all servers, or one

Each page shows every server you added, side by side, or the one server you
pick at the top. All servers is one more choice in the server picker, so
every page filters the same way.

On All servers, each row names its server, and a change goes to that row's
server only. Two servers' rules sit side by side and never merge, because
each server's rules shape only its own jobs.

- **Records:** Jobs, Audit Log, Costs, Evals and Verification.
- **What runs:** Workflows and Workspaces. Workflows holds the steps and
  three tabs of rules: Constraints, Capabilities and Permissions. Each
  rules tab applies an item to the steps and workflows you select, and a
  step's Rules tab lists what applies there. Workspaces holds each
  workspace's settings and its charter.
- **The server itself:** Engine and Settings.

## What a server holds

Each server is a control plane of its own, and everything that shapes it
lives with it.

- **Workspaces:** the repositories it works in, each with its checks, its
  setup, its VM image, and what happens to a job's branch at the end.
- **Workflows:** its series of steps, each step with its settings.
- **Its harness and factory:** constraints, what the agent must keep;
  capabilities, what it may use; and permissions, what the gate lets a
  call do. Each item applies only where you selected it. A workspace's
  charter adds its own items, for that workspace's jobs.
- **Settings:** its model providers, its people and their roles, its
  notifications, and its secrets.
- **Records:** its jobs, its audit log, its evals, its costs, and what
  landed.

These are live controls, not a configuration file you edit and restart. A
server runs on the iii engine, and each value you change in the app changes
how that engine works for the next call or the next job: which model a turn
runs on, which calls an agent may make, which image a VM boots, what a step
does. Bastion's job is to turn the values you set into how iii behaves.

The app itself keeps one thing: the list of servers you added. That list is
the only setting that belongs to you and not to a server.

## Local and remote

A local server starts with the app and needs no credentials. A remote
server needs a token the server issued you, over TLS. Either kind runs the
same control plane, so a workflow written on one works on the other. A job
runs on one server; your jobs can run on many.

## What does not change

- Agent code runs in a VM, never on your machine. A step reaches the
  network only when its workflow says so, with the secrets it names.
- Every action names who took it, in the server's audit log.
- A seeded default is a starting point, never a lock.

## Not this

Not an editor: the only code you read in Bastion is the change a job
proposes. Not a web app.

## Next

Copy the Short workflow, give its build step its own model and its own
grants, and send it one request. The job page shows what each step did
under those settings.
