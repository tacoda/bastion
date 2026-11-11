//! The governance rows: the checkpoint dials, constraints, capabilities,
//! held calls and the audit chain. `Store` extends this trait.

use bastion_core::audit::{self, Entry};
use bastion_core::govern::{Call, Capability, Constraint};
use bastion_core::{Dial, Dials, Stage};
use serde_json::{Value, json};

use crate::store::{Answer, HeldCall, SqlStore, name, parse};

pub trait GovernStore {
    /// The checkpoint dials. A checkpoint with no row asks.
    fn dials(&self) -> Answer<'_, Dials>;
    fn constraints(&self) -> Answer<'_, Vec<Constraint>>;
    /// Add a constraint, or replace the one with its id.
    fn set_constraint<'a>(&'a self, constraint: &'a Constraint) -> Answer<'a, ()>;
    fn capabilities(&self) -> Answer<'_, Vec<Capability>>;
    /// Add a grant, or replace the one for its stage and function.
    fn set_capability<'a>(&'a self, grant: &'a Capability) -> Answer<'a, ()>;
    fn hold<'a>(&'a self, call: &'a HeldCall) -> Answer<'a, ()>;
    /// The calls held for a job, oldest first.
    fn held<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<HeldCall>>;
    /// Take a held call off the list. True only for the one caller that
    /// removed it, so two people settling the same call cannot both run it.
    fn claim<'a>(&'a self, id: &'a str) -> Answer<'a, bool>;
    /// The whole audit chain, oldest first.
    fn audit(&self) -> Answer<'_, Vec<Entry>>;
    /// The gate's entries for one job, oldest first.
    fn gate_entries<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Entry>>;
    /// Every entry of one kind, oldest first.
    fn entries_of<'a>(&'a self, kind: &'a str) -> Answer<'a, Vec<Entry>>;
    /// The newest entry, the one the next appends after.
    fn audit_tail(&self) -> Answer<'_, Option<Entry>>;
    /// Add the next entry. A second entry with the same `seq` is refused, so
    /// two writers cannot fork the chain.
    fn append_audit<'a>(&'a self, entry: &'a Entry) -> Answer<'a, ()>;
}

const UPSERT_CONSTRAINT: &str = "INSERT INTO constraints (id, rule, function, contains, rung, source) VALUES (?, ?, ?, ?, ?, ?)
    ON CONFLICT (id) DO UPDATE SET rule = excluded.rule, function = excluded.function, contains = excluded.contains, rung = excluded.rung, source = excluded.source";
const UPSERT_CAPABILITY: &str = "INSERT INTO capabilities (stage, function, rung) VALUES (?, ?, ?)
    ON CONFLICT (stage, function) DO UPDATE SET rung = excluded.rung";

fn constraint_params(c: &Constraint) -> Value {
    json!([
        c.id,
        c.rule,
        c.function,
        c.contains,
        name(c.rung),
        name(c.source)
    ])
}

fn capability_params(g: &Capability) -> Value {
    json!([name(g.stage), g.function, name(g.rung)])
}

pub(crate) fn constraint(r: &Value) -> Result<Constraint, String> {
    Ok(Constraint {
        id: parse(r, "id")?,
        rule: parse(r, "rule")?,
        function: parse(r, "function")?,
        contains: parse(r, "contains")?,
        rung: parse(r, "rung")?,
        source: parse(r, "source")?,
    })
}

pub(crate) fn capability(r: &Value) -> Result<Capability, String> {
    Ok(Capability {
        stage: parse(r, "stage")?,
        function: parse(r, "function")?,
        rung: parse(r, "rung")?,
    })
}

pub(crate) fn held_call(row: &Value) -> Result<HeldCall, String> {
    let arguments: String = parse(row, "arguments")?;
    Ok(HeldCall {
        id: parse(row, "id")?,
        job: parse(row, "job")?,
        session: parse(row, "session")?,
        turn: parse(row, "turn")?,
        call: Call {
            content: String::new(),
            function: parse(row, "function")?,
            text: parse(row, "text")?,
        },
        arguments: serde_json::from_str(&arguments)
            .map_err(|e| format!("column arguments: {e}"))?,
        at: parse(row, "at")?,
    })
}

pub(crate) fn entry(row: &Value) -> Result<Entry, String> {
    Ok(Entry {
        seq: parse(row, "seq")?,
        at: parse(row, "at")?,
        record: audit::Record {
            kind: parse(row, "kind")?,
            subject: parse(row, "subject")?,
            detail: parse(row, "detail")?,
        },
        prev: parse(row, "prev")?,
        hash: parse(row, "hash")?,
    })
}

impl GovernStore for SqlStore {
    fn dials(&self) -> Answer<'_, Dials> {
        Box::pin(async move {
            let mut dials = Dials::default();
            for row in self
                .query("SELECT checkpoint, dial FROM dials", json!([]))
                .await?
            {
                let dial: Dial = parse(&row, "dial")?;
                let judged: &[Stage] = match row["checkpoint"].as_str() {
                    Some("approve") => &[Stage::Plan],
                    Some("review") => &[Stage::Build, Stage::CodeReview],
                    _ => &[],
                };
                for stage in judged {
                    dials.set(*stage, dial);
                }
            }
            Ok(dials)
        })
    }

    fn constraints(&self) -> Answer<'_, Vec<Constraint>> {
        Box::pin(async move {
            let sql =
                "SELECT id, rule, function, contains, rung, source FROM constraints ORDER BY id";
            self.rows(sql, json!([]), constraint).await
        })
    }

    fn set_constraint<'a>(&'a self, c: &'a Constraint) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(UPSERT_CONSTRAINT, constraint_params(c))
                .await
                .map(|_| ())
        })
    }

    fn capabilities(&self) -> Answer<'_, Vec<Capability>> {
        Box::pin(async move {
            let sql = "SELECT stage, function, rung FROM capabilities ORDER BY stage, function";
            self.rows(sql, json!([]), capability).await
        })
    }

    fn set_capability<'a>(&'a self, g: &'a Capability) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(UPSERT_CAPABILITY, capability_params(g))
                .await
                .map(|_| ())
        })
    }

    fn hold<'a>(&'a self, h: &'a HeldCall) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO held_calls (id, job, session, turn, function, text, arguments, at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                json!([h.id, h.job, h.session, h.turn, h.call.function, h.call.text, h.arguments.to_string(), h.at]),
            )
            .await
            .map(|_| ())
        })
    }

    fn held<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<HeldCall>> {
        Box::pin(async move {
            let sql = "SELECT id, job, session, turn, function, text, arguments, at FROM held_calls WHERE job = ? ORDER BY at, id";
            self.rows(sql, json!([job]), held_call).await
        })
    }

    fn claim<'a>(&'a self, id: &'a str) -> Answer<'a, bool> {
        Box::pin(async move {
            let sql = "DELETE FROM held_calls WHERE id = ? RETURNING id";
            Ok(!self.execute(sql, json!([id])).await?.is_empty())
        })
    }

    fn audit(&self) -> Answer<'_, Vec<Entry>> {
        Box::pin(async move {
            let sql =
                "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log ORDER BY seq";
            self.rows(sql, json!([]), entry).await
        })
    }

    fn gate_entries<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Entry>> {
        Box::pin(async move {
            let sql = "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log WHERE kind = 'gate' AND subject = ? ORDER BY seq";
            self.rows(sql, json!([job]), entry).await
        })
    }

    fn entries_of<'a>(&'a self, kind: &'a str) -> Answer<'a, Vec<Entry>> {
        Box::pin(async move {
            let sql = "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log WHERE kind = ? ORDER BY seq";
            self.rows(sql, json!([kind]), entry).await
        })
    }

    fn audit_tail(&self) -> Answer<'_, Option<Entry>> {
        Box::pin(async move {
            let sql = "SELECT seq, at, kind, subject, detail, prev, hash FROM audit_log ORDER BY seq DESC LIMIT 1";
            Ok(self.rows(sql, json!([]), entry).await?.pop())
        })
    }

    fn append_audit<'a>(&'a self, e: &'a Entry) -> Answer<'a, ()> {
        Box::pin(async move {
            self.execute(
                "INSERT INTO audit_log (seq, at, kind, subject, detail, prev, hash) VALUES (?, ?, ?, ?, ?, ?, ?)",
                json!([e.seq, e.at, e.record.kind, e.record.subject, e.record.detail, e.prev, e.hash]),
            )
            .await
            .map(|_| ())
        })
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use crate::store::fake::{FakeStore, matching};

    impl GovernStore for FakeStore {
        fn dials(&self) -> Answer<'_, Dials> {
            let dials = self.dials.lock().unwrap().clone();
            Box::pin(async move { Ok(dials) })
        }

        fn constraints(&self) -> Answer<'_, Vec<Constraint>> {
            let all = self.constraints.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn set_constraint<'a>(&'a self, c: &'a Constraint) -> Answer<'a, ()> {
            let mut all = self.constraints.lock().unwrap();
            all.retain(|x| x.id != c.id);
            all.push(c.clone());
            Box::pin(async { Ok(()) })
        }

        fn capabilities(&self) -> Answer<'_, Vec<Capability>> {
            let all = self.capabilities.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn set_capability<'a>(&'a self, g: &'a Capability) -> Answer<'a, ()> {
            let mut all = self.capabilities.lock().unwrap();
            all.retain(|x| (x.stage, &x.function) != (g.stage, &g.function));
            all.push(g.clone());
            Box::pin(async { Ok(()) })
        }

        fn hold<'a>(&'a self, h: &'a HeldCall) -> Answer<'a, ()> {
            self.held.lock().unwrap().push(h.clone());
            Box::pin(async { Ok(()) })
        }

        fn held<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<HeldCall>> {
            let found = matching(&self.held, |h| h.job == job);
            Box::pin(async move { Ok(found) })
        }

        fn claim<'a>(&'a self, id: &'a str) -> Answer<'a, bool> {
            let mut held = self.held.lock().unwrap();
            let before = held.len();
            held.retain(|h| h.id != id);
            let claimed = held.len() < before;
            Box::pin(async move { Ok(claimed) })
        }

        fn audit(&self) -> Answer<'_, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn gate_entries<'a>(&'a self, job: &'a str) -> Answer<'a, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            let of_job = all
                .into_iter()
                .filter(|e| e.record.kind == "gate" && e.record.subject == job)
                .collect();
            Box::pin(async move { Ok(of_job) })
        }

        fn entries_of<'a>(&'a self, kind: &'a str) -> Answer<'a, Vec<Entry>> {
            let all = self.audit.lock().unwrap().clone();
            let of_kind = all.into_iter().filter(|e| e.record.kind == kind).collect();
            Box::pin(async move { Ok(of_kind) })
        }

        fn audit_tail(&self) -> Answer<'_, Option<Entry>> {
            let last = self.audit.lock().unwrap().last().cloned();
            Box::pin(async move { Ok(last) })
        }

        fn append_audit<'a>(&'a self, e: &'a Entry) -> Answer<'a, ()> {
            if *self.stall_audit.lock().unwrap() {
                return Box::pin(std::future::pending());
            }
            let mut all = self.audit.lock().unwrap();
            let result = if all.iter().any(|x| x.seq == e.seq) {
                Err(format!("audit entry {} exists", e.seq))
            } else {
                all.push(e.clone());
                Ok(())
            };
            Box::pin(async move { result })
        }
    }
}
