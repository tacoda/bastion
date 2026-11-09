//! The data both N14 spikes draw, so their numbers compare: 200 jobs, a
//! 20-step workflow, a 2,000-line diff, and the theme's colors (D4).

pub const BG: &str = "#0f0d17";
pub const PANEL: &str = "#19162a";
pub const INK: &str = "#ece9f5";
pub const MUTED: &str = "#a39cb8";
pub const LINE: &str = "#2c2742";
pub const PRIMARY: &str = "#7c3aed";
pub const LINK: &str = "#c4b5fd";
pub const OK: &str = "#4ade80";
pub const BAD: &str = "#f87171";
pub const WARN: &str = "#fbbf24";

pub const PAGES: [&str; 10] = [
    "Jobs", "Workflows", "Constraints", "Capabilities", "Audit Log", "Evals", "Engine", "Settings", "Diff", "",
];

const STAGES: [&str; 5] = ["plan", "build", "checks", "review", "land"];
const STATES: [&str; 4] = ["running", "needs_you", "landed", "failed"];

#[derive(Clone)]
pub struct Job {
    pub id: String,
    pub request: String,
    pub stage: &'static str,
    pub state: &'static str,
}

pub fn jobs() -> Vec<Job> {
    (0..200)
        .map(|i| Job {
            id: format!("j{:08x}", 0x3b46_0000u32 + i * 7919),
            request: format!("Add a function number {i} to calc.py, with a test in test_calc.py."),
            stage: STAGES[i as usize % STAGES.len()],
            state: STATES[(i as usize / 3) % STATES.len()],
        })
        .collect()
}

/// The next state a pushed change gives a job.
pub fn next_state(state: &str) -> &'static str {
    let i = STATES.iter().position(|s| *s == state).unwrap_or(0);
    STATES[(i + 1) % STATES.len()]
}

pub struct Step {
    pub name: String,
    pub x: f64,
    pub y: f64,
}

pub const STEP_W: f64 = 168.0;
pub const STEP_H: f64 = 56.0;

/// 20 steps in four rows of five, each linked to the next and some to the
/// step two ahead.
pub fn workflow() -> (Vec<Step>, Vec<(usize, usize)>) {
    let steps = (0..20)
        .map(|i| Step {
            name: format!("step {i} · lights-on"),
            x: 24.0 + (i % 5) as f64 * 210.0,
            y: 24.0 + (i / 5) as f64 * 120.0,
        })
        .collect();
    let mut links: Vec<(usize, usize)> = (0..19).map(|i| (i, i + 1)).collect();
    links.extend((0..18).step_by(3).map(|i| (i, i + 2)));
    (steps, links)
}

/// One line of a diff and its mark: '+', '-' or ' '.
pub fn diff() -> Vec<(char, String)> {
    (0..2000)
        .map(|i| {
            let mark = match i % 7 {
                0 | 1 => '+',
                2 => '-',
                _ => ' ',
            };
            (mark, format!("{i:5}  def function_{i}(a, b):  return a * {i} + b  # line {i}"))
        })
        .collect()
}

pub fn now_ms() -> u128 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |d| d.as_millis())
}

/// Median, 95th percentile and worst of `samples`, in milliseconds.
pub fn summary(name: &str, mut samples: Vec<f64>) -> String {
    samples.sort_by(f64::total_cmp);
    let at = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
    format!(
        "RESULT {name} n={} median={:.2} p95={:.2} max={:.2}",
        samples.len(),
        at(0.5),
        at(0.95),
        at(1.0)
    )
}
