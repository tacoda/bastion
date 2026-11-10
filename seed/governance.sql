-- The owner's first-run rules and grants (PLAN.md D26). Bastion runs this
-- file each time it opens the database. INSERT OR IGNORE writes a row only
-- when its key is missing, so a row the owner changed is kept. Edit this file
-- to change the defaults of a new database; edit the rows to change this one.

-- Constraints: rung is told, checked or enforced; source is owner.
INSERT OR IGNORE INTO constraints (id, rule, function, contains, rung, source) VALUES
  ('no-login-shell', 'Run commands directly, not through bash -lc.', 'sandbox::exec', 'bash -lc', 'enforced', 'owner'),
  ('keep-tests', 'Do not delete existing tests.', 'sandbox::exec', 'git rm', 'told', 'owner'),
  ('tests-kept', 'Every existing test still exists at review.', 'review::diff', 'deleted tests', 'checked', 'owner'),
  ('no-secrets', 'Commit no secret: no key, token or private key.', 'review::diff', 'secrets', 'enforced', 'owner'),
  ('assertions-kept', 'Keep every assertion a test makes.', 'review::diff', 'removed assertions', 'checked', 'owner'),
  ('no-skipped-tests', 'Do not skip or ignore a test.', 'review::diff', 'skip markers', 'checked', 'owner'),
  ('commit-work', 'Commit every change before you reply.', '', '', 'told', 'owner'),
  ('clean-tree', 'Leave a clean tree: no untracked files.', '', '', 'told', 'owner');

-- Capabilities: per stage, rung is denied, ask or allowed. A plan reads and
-- runs commands, and asks before it writes a file. A build may use the whole
-- sandbox. The full pipeline's read-only turns read and run commands, and may
-- not write a file; its turns that write code may use the whole sandbox, as a
-- build does. A workflow's skill and prompt steps may change the tree, so
-- they may too.
INSERT OR IGNORE INTO capabilities (stage, function, rung) VALUES
  ('plan', 'sandbox::*', 'allowed'),
  ('plan', 'sandbox::fs::write', 'ask'),
  ('build', 'sandbox::*', 'allowed'),
  ('scope', 'sandbox::*', 'allowed'),
  ('scope', 'sandbox::fs::write', 'denied'),
  ('discover', 'sandbox::*', 'allowed'),
  ('discover', 'sandbox::fs::write', 'denied'),
  ('define', 'sandbox::*', 'allowed'),
  ('define', 'sandbox::fs::write', 'denied'),
  ('refactor', 'sandbox::*', 'allowed'),
  ('refactor', 'sandbox::fs::write', 'denied'),
  ('code_review', 'sandbox::*', 'allowed'),
  ('code_review', 'sandbox::fs::write', 'denied'),
  ('prove', 'sandbox::*', 'allowed'),
  ('prove', 'sandbox::fs::write', 'denied'),
  ('comprehend', 'sandbox::*', 'allowed'),
  ('comprehend', 'sandbox::fs::write', 'denied'),
  ('learn', 'sandbox::*', 'allowed'),
  ('learn', 'sandbox::fs::write', 'denied'),
  ('implement', 'sandbox::*', 'allowed'),
  ('specify', 'sandbox::*', 'allowed'),
  ('ci_fix', 'sandbox::*', 'allowed'),
  ('falsify', 'sandbox::*', 'allowed'),
  ('skill', 'sandbox::*', 'allowed'),
  ('prompt', 'sandbox::*', 'allowed');

-- Workflows (D35): starting points for engineers, managers, and platform
-- and SRE teams. Each lands a reviewed change, a document under docs/ or code, and
-- waits for a person at land. A workflow's row is written only while none of
-- its steps are stored, so one the owner deleted stays deleted, and the
-- owner's later versions are kept.
INSERT OR IGNORE INTO workflows (id, name, version, updated_at)
SELECT column1, column2, 1, 0 FROM (VALUES
  ('research', 'Research'),
  ('bug-fix', 'Bug fix'),
  ('dependency-upgrade', 'Dependency upgrade'),
  ('test-coverage', 'Test coverage'),
  ('weekly-status', 'Weekly status'),
  ('decision-record', 'Decision record'),
  ('onboarding-guide', 'Onboarding guide'),
  ('incident-response', 'Incident response'),
  ('incident-postmortem', 'Incident postmortem'),
  ('runbook', 'Runbook'),
  ('infrastructure-change', 'Infrastructure change'))
WHERE NOT EXISTS (SELECT 1 FROM workflow_nodes WHERE workflow = column1);

INSERT OR IGNORE INTO workflow_nodes (workflow, version, id, stage, run, dial, note, x, y) VALUES
  ('incident-response', 1, 'recent-changes', 'command', 'git log --since="3 days ago" --no-merges --stat --date=iso --pretty=format:"%h %an %ad %s"', 'dark', 'The changes that could have caused the incident.', 0, 0),
  ('incident-response', 1, 'plan', 'plan', '', 'dark', 'Triage the incident in the request: its symptoms, when it started and what it hits. Match it against the recent changes, the code and the config. Name the most likely cause with its evidence, and plan the smallest change that stops the impact.', 220, 0),
  ('incident-response', 1, 'build', 'build', '', 'dark', 'Make the smallest change that stops the impact: a revert, a feature flag turned off, or a config rollback. Write what you changed, why, and how to check that the impact stopped into docs/incidents/, named for the date and the incident.', 440, 0),
  ('incident-response', 1, 'land', 'land', '', 'lights_on', '', 660, 0),
  ('research', 1, 'plan', 'plan', '', 'dark', 'Research the question in the request. Read the code, docs, tests and git history that bear on it. Cite files and lines, and say what you could not find out. Plan one task: write the report.', 0, 0),
  ('research', 1, 'build', 'build', '', 'dark', 'Write the findings as a report in docs/research/, named for the question. Change no other file.', 220, 0),
  ('research', 1, 'land', 'land', '', 'lights_on', '', 440, 0),
  ('bug-fix', 1, 'plan', 'plan', '', 'lights_on', 'Find the root cause of the bug in the request, and name the smallest failing test that shows it. Plan to write that test first, then fix the cause, not the symptom.', 0, 0),
  ('bug-fix', 1, 'build', 'build', '', 'dark', 'Write the failing test first and see it fail, then fix the root cause. Keep the test.', 220, 0),
  ('bug-fix', 1, 'land', 'land', '', 'lights_on', '', 440, 0),
  ('dependency-upgrade', 1, 'plan', 'plan', '', 'lights_on', 'List the outdated dependencies the request names, or all of them. Read each changelog for breaking changes between the two versions. Plan the upgrades from safest to riskiest, one task each.', 0, 0),
  ('dependency-upgrade', 1, 'build', 'build', '', 'dark', 'Upgrade one dependency per task and run the tests after each. Adapt the code to each breaking change in the notes.', 220, 0),
  ('dependency-upgrade', 1, 'land', 'land', '', 'lights_on', '', 440, 0),
  ('test-coverage', 1, 'plan', 'plan', '', 'dark', 'Find the code paths the request names, or the riskiest untested ones. For each, say why it matters and which test would cover it.', 0, 0),
  ('test-coverage', 1, 'build', 'build', '', 'dark', 'Add a test for each gap. Change no product code. If a test finds a bug, mark the test as expected to fail and say so.', 220, 0),
  ('test-coverage', 1, 'land', 'land', '', 'lights_on', '', 440, 0),
  ('weekly-status', 1, 'changes', 'command', 'git log --since="7 days ago" --no-merges --stat --pretty=format:"%h %an %s"', 'dark', 'What changed in the last week.', 0, 0),
  ('weekly-status', 1, 'status', 'prompt', '', 'dark', 'Write a status update for stakeholders from the changes: what shipped, what is in progress, and the risks or blockers. Group by theme, not by commit, in plain words.', 220, 0),
  ('weekly-status', 1, 'build', 'build', '', 'dark', 'Write the status update into docs/status/, named for today. Change no other file.', 440, 0),
  ('weekly-status', 1, 'land', 'land', '', 'lights_on', '', 660, 0),
  ('decision-record', 1, 'plan', 'plan', '', 'dark', 'Find what the decision in the request touches: the code, the constraints and the options. For each option, list its costs and risks in this codebase.', 0, 0),
  ('decision-record', 1, 'adr', 'prompt', '', 'lights_on', 'Write an architecture decision record from the options: context, the options considered, the decision, and its consequences. Recommend one option and say why.', 220, 0),
  ('decision-record', 1, 'build', 'build', '', 'dark', 'Write the decision record into docs/adr/ with the next free number. Change no other file.', 440, 0),
  ('decision-record', 1, 'land', 'land', '', 'lights_on', '', 660, 0),
  ('onboarding-guide', 1, 'plan', 'plan', '', 'dark', 'Map the repository for a new team member: what it does, how it is laid out, how to build, test and run it, and where to start. Cite files.', 0, 0),
  ('onboarding-guide', 1, 'build', 'build', '', 'dark', 'Write the map as docs/ONBOARDING.md. Change no other file.', 220, 0),
  ('onboarding-guide', 1, 'land', 'land', '', 'lights_on', '', 440, 0),
  ('incident-postmortem', 1, 'plan', 'plan', '', 'dark', 'Read the incident in the request: its timeline, symptoms and links. Find the code, config and changes that contributed, and build a timeline with times.', 0, 0),
  ('incident-postmortem', 1, 'postmortem', 'prompt', '', 'lights_on', 'Write a blameless postmortem from the timeline: summary, impact, timeline, contributing causes, what went well, and action items, each with an owner role and a priority.', 220, 0),
  ('incident-postmortem', 1, 'build', 'build', '', 'dark', 'Write the postmortem into docs/postmortems/, named for the date and the incident. Change no other file.', 440, 0),
  ('incident-postmortem', 1, 'land', 'land', '', 'lights_on', '', 660, 0),
  ('runbook', 1, 'plan', 'plan', '', 'dark', 'For the service in the request, read its code, config and deploy files. List how it fails, how each failure shows in logs and metrics, and how to check and recover.', 0, 0),
  ('runbook', 1, 'build', 'build', '', 'dark', 'Write a runbook into docs/runbooks/, named for the service: one section per failure with its signs, checks and recovery steps.', 220, 0),
  ('runbook', 1, 'land', 'land', '', 'lights_on', '', 440, 0),
  ('infrastructure-change', 1, 'plan', 'plan', '', 'lights_on', 'Plan the infrastructure change in the request in its infrastructure-as-code files. Name the blast radius, the order of changes, how to check each, and how to roll back.', 0, 0),
  ('infrastructure-change', 1, 'build', 'build', '', 'dark', 'Make the change in the infrastructure-as-code files only, following the plan. Do not apply it.', 220, 0),
  ('infrastructure-change', 1, 'land', 'land', '', 'lights_on', '', 440, 0);

INSERT OR IGNORE INTO workflow_results (workflow, version, node, n, name) VALUES
  ('incident-response', 1, 'recent-changes', 0, 'recent-changes'),
  ('incident-response', 1, 'plan', 0, 'triage'),
  ('research', 1, 'plan', 0, 'findings'),
  ('bug-fix', 1, 'plan', 0, 'root-cause'),
  ('dependency-upgrade', 1, 'plan', 0, 'upgrade-notes'),
  ('test-coverage', 1, 'plan', 0, 'gaps'),
  ('weekly-status', 1, 'changes', 0, 'changes'),
  ('weekly-status', 1, 'status', 0, 'status'),
  ('decision-record', 1, 'plan', 0, 'options'),
  ('decision-record', 1, 'adr', 0, 'adr'),
  ('onboarding-guide', 1, 'plan', 0, 'map'),
  ('incident-postmortem', 1, 'plan', 0, 'timeline'),
  ('incident-postmortem', 1, 'postmortem', 0, 'postmortem'),
  ('runbook', 1, 'plan', 0, 'failure-modes'),
  ('infrastructure-change', 1, 'plan', 0, 'change-plan');

INSERT OR IGNORE INTO workflow_links (workflow, version, source, target) VALUES
  ('incident-response', 1, 'recent-changes', 'plan'),
  ('incident-response', 1, 'plan', 'build'),
  ('incident-response', 1, 'build', 'land'),
  ('research', 1, 'plan', 'build'),
  ('research', 1, 'build', 'land'),
  ('bug-fix', 1, 'plan', 'build'),
  ('bug-fix', 1, 'build', 'land'),
  ('dependency-upgrade', 1, 'plan', 'build'),
  ('dependency-upgrade', 1, 'build', 'land'),
  ('test-coverage', 1, 'plan', 'build'),
  ('test-coverage', 1, 'build', 'land'),
  ('weekly-status', 1, 'changes', 'status'),
  ('weekly-status', 1, 'status', 'build'),
  ('weekly-status', 1, 'build', 'land'),
  ('decision-record', 1, 'plan', 'adr'),
  ('decision-record', 1, 'adr', 'build'),
  ('decision-record', 1, 'build', 'land'),
  ('onboarding-guide', 1, 'plan', 'build'),
  ('onboarding-guide', 1, 'build', 'land'),
  ('incident-postmortem', 1, 'plan', 'postmortem'),
  ('incident-postmortem', 1, 'postmortem', 'build'),
  ('incident-postmortem', 1, 'build', 'land'),
  ('runbook', 1, 'plan', 'build'),
  ('runbook', 1, 'build', 'land'),
  ('infrastructure-change', 1, 'plan', 'build'),
  ('infrastructure-change', 1, 'build', 'land');
