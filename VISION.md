# Bastion's vision

<!-- voice-register: informational -->
<!-- voice-english: ste -->

Bastion is an agent control plane. You define workflows and send requests,
and Bastion fulfills each request as a job: agents work through the
workflow's steps, under the rules you set on each step. Jobs and workflows
are the two ideas everything else serves. The cost is plain: a job does only
what its workflow says, so a check you take out of a workflow is gone.
Bastion runs on your machine, on a remote server, or on both at once.

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

## Rules move onto the workflow

Bastion used to split its rules into two layers. The harness bound an
agent's calls. The factory bound the work around them: what a step may do,
and when a change lands. Both now live more and more on a workflow and its
steps, and Bastion applies them as each step runs. I think the two layers
will collapse into one, with Bastion applying whatever the workflow and its
steps set. The split may still be useful, though, and that isn't settled;
the next few workflows people write will settle it.

A repository brings its own rules too: its charter, in `AGENTS.md` and
`.agents/settings.json`. A turn may read the charter but may not change it.

## Two levels: each server, and all of them

Every page shows one server, the one you pick at the top. Some pages can
also show every server at once, and those are the ones about records.

**Records, per server or across all.** Jobs, Audit Log, Costs, Evals and
Verification read one server, or every server you added, side by side. All
servers is one more choice in the server picker, so you filter by server the
same way on each of these pages. You open a job, an entry or a turn and act
on it wherever it lives.

**Rules, one server at a time.** Workflows, Constraints, Capabilities,
Engine and Settings belong to the server you pick. A rules page never mixes
two servers, because each server's rules shape only its own jobs, and a sum
of two rule sets means nothing.

## What a server holds

Each server is a control plane of its own, and everything that shapes it
lives with it.

- **Workspaces:** the repositories it works in, each with its checks, its
  setup, its VM image, and what happens to a job's branch at the end.
- **Workflows:** its series of steps, each step with its settings.
- **Capabilities and constraints:** what an agent may call, each grant ask
  or allowed, and the rules its work must keep, each from told to enforced.
  Both apply to every step of a kind, or to one workflow step.
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
