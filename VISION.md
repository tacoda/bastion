# Bastion's vision

<!-- voice-register: informational -->

Bastion is a desktop app a developer uses to run jobs through software
factories. A factory is a Bastion server, on your machine or a remote one,
and you can use several at once. You define what each factory does: its
workflows, the capabilities its agents get, and the constraints they work
under. Bastion seeds the common workflows with their text filled in, and
you can change every step of them. The cost is honest: a factory does only
what its workflows say, so a gate you remove is gone.

## Two levels: each server, and all of them

Every page shows one server, the one you pick at the top. Some pages can
also show every server at once, and those are the ones about records.

**Records, per server or across all.** Jobs, Audit Log and Costs read one
server, or every server you added, side by side. All servers is one more
choice in the server picker, so you filter by server the same way on each
of these pages. You open a job, an entry or a turn and act on it wherever
it lives.

**Rules, one server at a time.** Workflows, Constraints, Capabilities,
Evals, Verification, Engine and Settings belong to the server you pick. A
rules page never mixes two servers, because each server's rules shape only
its own jobs, and a sum of two rule sets means nothing.

## What a server holds

A server is a factory, and everything that shapes it lives with it.

- **Workspaces:** the repositories it works in, each with its checks, its
  setup, its VM image, and what happens to a job's branch at the end.
- **Workflows:** series of steps. A step is a prompt, one of the
  repository's agents, skills or commands, or a shell script. Each step has
  one text to edit, a dial (a sun when a person decides, a moon when it runs
  on its own), and the results it passes on.
- **Capabilities and constraints:** what an agent may call, and the rules
  its work must keep, each on a ladder from told to enforced.
- **Settings:** its model providers, its people and their roles, its
  notifications, and its secrets.
- **Records:** its audit log, its evals, its costs, and what landed.

These are live controls, not a configuration file you edit and restart.
A server runs on the iii engine, and each value you change in the app
changes how that engine works for the next call or the next job: which
model a turn runs on, which calls an agent may make, which image a VM
boots, what a step does. Bastion's job is to turn the values you set into
how iii behaves.

The app itself keeps one thing: the list of servers you added. That list
is the only setting that belongs to you and not to a server.

## Local and remote

A local server starts with the app and needs no credentials. A remote
server needs a token the server issued you, over TLS. Either kind runs the
same factory, so a workflow written on one works on the other. A job runs
on one server; your jobs can run on many.

## What does not change

- Agent code runs in a VM, never on your machine. A step reaches the
  network only when its workflow says so, with the secrets it names.
- Every action names who took it, in the server's audit log.
- A seeded default is a starting point, never a lock.

## Not this

Not an editor: the only code you read in Bastion is the change a job
proposes. Not a web app. Not a hosted service, for now.

## Next

Go page by page, on the server level first: each page shows the server it
belongs to, and its settings move with that server.
