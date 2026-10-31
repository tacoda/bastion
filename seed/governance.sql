-- The owner's first-run rules and grants (PLAN.md D26). Bastion runs this
-- file each time it opens the database. INSERT OR IGNORE writes a row only
-- when its key is missing, so a row the owner changed is kept. Edit this file
-- to change the defaults of a new database; edit the rows to change this one.

-- Constraints: rung is told, checked or enforced; source is owner.
INSERT OR IGNORE INTO constraints (id, rule, function, contains, rung, source) VALUES
  ('no-login-shell', 'Run commands directly, not through bash -lc.', 'sandbox::exec', 'bash -lc', 'enforced', 'owner'),
  ('keep-tests', 'Do not delete existing tests.', 'sandbox::exec', 'git rm', 'told', 'owner'),
  ('tests-kept', 'Every existing test still exists at review.', 'review::diff', 'deleted tests', 'checked', 'owner'),
  ('commit-work', 'Commit every change before you reply.', '', '', 'told', 'owner'),
  ('clean-tree', 'Leave a clean tree: no untracked files.', '', '', 'told', 'owner');

-- Capabilities: per stage, rung is denied, ask or allowed. A plan reads and
-- runs commands, and asks before it writes a file. A build may use the whole sandbox.
INSERT OR IGNORE INTO capabilities (stage, function, rung) VALUES
  ('plan', 'sandbox::*', 'allowed'),
  ('plan', 'sandbox::fs::write', 'ask'),
  ('build', 'sandbox::*', 'allowed');
