//! A trail's wear on the Map: how many journeys took it, how many reached
//! the summit and how many the marsh, and at each waypoint how many wait
//! there and how many ended there.

use bastion_protocol::JobSummary;

/// A trail's journeys, as the Map counts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Worn {
    pub journeys: usize,
    pub summit: usize,
    pub marsh: usize,
}

fn lost(state: &str) -> bool {
    matches!(state, "failed" | "stopped")
}

/// The wear on trail `workflow`.
pub fn trail(journeys: &[JobSummary], workflow: &str) -> Worn {
    let mine = journeys
        .iter()
        .filter(|j| j.workflow.as_deref() == Some(workflow));
    mine.fold(Worn::default(), |w, j| Worn {
        journeys: w.journeys + 1,
        summit: w.summit + usize::from(j.state == "landed"),
        marsh: w.marsh + usize::from(lost(&j.state)),
    })
}

/// At waypoint `key`, `workflow/step`: the journeys waiting there for a
/// person, and the journeys that ended there in the marsh.
pub fn waypoint(journeys: &[JobSummary], key: &str) -> (usize, usize) {
    let Some((workflow, step)) = key.split_once('/') else {
        return (0, 0);
    };
    let here = journeys
        .iter()
        .filter(|j| j.workflow.as_deref() == Some(workflow) && j.node.as_deref() == Some(step));
    here.fold((0, 0), |(waiting, ended), j| {
        (
            waiting + usize::from(j.state == "needs_you"),
            ended + usize::from(lost(&j.state)),
        )
    })
}

/// The line under a trail's name.
pub fn line(w: Worn) -> String {
    match w.journeys {
        0 => "no journeys yet".into(),
        1 => format!(
            "1 journey · {} at the summit · {} in the marsh",
            w.summit, w.marsh
        ),
        n => format!(
            "{n} journeys · {} at the summit · {} in the marsh",
            w.summit, w.marsh
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journey(state: &str, at: Option<&str>) -> JobSummary {
        JobSummary {
            state: state.into(),
            node: at.map(String::from),
            workflow: Some("bug-fix".into()),
            ..JobSummary::default()
        }
    }

    #[test]
    fn a_trail_counts_its_journeys_and_where_they_wait_or_end() {
        let all = [
            journey("landed", None),
            journey("failed", Some("build")),
            journey("needs_you", Some("plan")),
            JobSummary {
                workflow: Some("short".into()),
                ..journey("landed", None)
            },
        ];
        let worn = trail(&all, "bug-fix");
        assert_eq!(
            worn,
            Worn {
                journeys: 3,
                summit: 1,
                marsh: 1
            }
        );
        assert_eq!(waypoint(&all, "bug-fix/plan"), (1, 0));
        assert_eq!(waypoint(&all, "bug-fix/build"), (0, 1));
        assert_eq!(waypoint(&all, "short/plan"), (0, 0));
        assert_eq!(line(worn), "3 journeys · 1 at the summit · 1 in the marsh");
        assert_eq!(line(Worn::default()), "no journeys yet");
    }
}
