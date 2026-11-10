//! What the turns on this server cost (F5): in all, by workspace, by step
//! and by day, from the usage each turn's end metered.

use std::collections::BTreeMap;

use bastion_protocol::{CostsView, SessionView, Tally};

use super::factory::{FactoryCell, factory, name};
use crate::eval_store::Turn;

/// How many of the newest turns the page lists one by one.
const LISTED: usize = 200;

pub async fn costs(cell: &FactoryCell) -> Result<CostsView, String> {
    let all = factory(cell)?.store().all_turns().await?;
    Ok(tally(&all))
}

// ponytail: every turn is read to tally; a GROUP BY in SQL when a server keeps more than some thousands.
fn tally(all: &[(Turn, String)]) -> CostsView {
    let by = |key: &dyn Fn(&Turn, &str) -> String| {
        let mut groups: BTreeMap<String, Tally> = BTreeMap::new();
        for (t, ws) in all {
            let k = key(t, ws);
            add(groups.entry(k.clone()).or_insert_with(|| keyed(k)), t);
        }
        groups.into_values().collect::<Vec<_>>()
    };
    let mut total = keyed("all".into());
    all.iter().for_each(|(t, _)| add(&mut total, t));
    let mut by_day = by(&|t, _| day(t.ended_at));
    by_day.reverse();
    CostsView {
        total,
        by_workspace: by(&|_, ws| ws.to_string()),
        by_stage: by(&|t, _| name(t.stage)),
        by_day,
        sessions: all.iter().take(LISTED).map(session).collect(),
    }
}

fn keyed(key: String) -> Tally {
    Tally {
        key,
        ..Tally::default()
    }
}

fn add(sum: &mut Tally, t: &Turn) {
    let u = t.usage;
    sum.sessions += 1;
    sum.turns += u.turns;
    sum.calls += u.calls;
    sum.call_errors += u.call_errors;
    sum.cost_micros += u.cost_micros;
}

fn session((t, ws): &(Turn, String)) -> SessionView {
    SessionView {
        session: t.session.clone(),
        job: t.job.clone(),
        workspace: ws.clone(),
        stage: name(t.stage),
        turns: t.usage.turns,
        calls: t.usage.calls,
        call_errors: t.usage.call_errors,
        cost_micros: t.usage.cost_micros,
        ended_at: t.ended_at,
    }
}

/// The UTC day of `secs` since the epoch, as `YYYY-MM-DD`.
fn day(secs: u64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastion_core::Stage;
    use bastion_core::eval::Usage;

    fn turn(session: &str, stage: Stage, cost: u64, ended_at: u64) -> Turn {
        Turn {
            session: session.into(),
            job: "j1".into(),
            stage,
            usage: Usage {
                turns: 2,
                calls: 3,
                call_errors: 1,
                cost_micros: cost,
            },
            ended_at,
        }
    }

    #[test]
    fn a_day_is_its_utc_date() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_794_349_646), "2026-11-10");
        assert_eq!(day(951_782_400), "2000-02-29");
    }

    #[test]
    fn turns_are_tallied_in_all_by_workspace_by_step_and_by_day() {
        let all = vec![
            (turn("s3", Stage::Build, 300, 1_794_349_646), "b".into()),
            (turn("s2", Stage::Plan, 200, 1_794_349_000), "a".into()),
            (turn("s1", Stage::Plan, 100, 1_794_200_000), "a".into()),
        ];
        let view = tally(&all);
        assert_eq!((view.total.sessions, view.total.cost_micros), (3, 600));
        assert_eq!(view.total.calls, 9);
        let ws: Vec<_> = view
            .by_workspace
            .iter()
            .map(|t| (t.key.as_str(), t.cost_micros))
            .collect();
        assert_eq!(ws, [("a", 300), ("b", 300)]);
        assert_eq!(view.by_stage[1].key, "plan");
        assert_eq!(view.by_day[0].key, "2026-11-10", "newest day first");
        assert_eq!(view.by_day[0].sessions, 2);
        assert_eq!(view.sessions[0].session, "s3", "newest turn first");
    }
}
