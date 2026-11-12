//! A region's corpus (D61): the why behind its code (why it is shaped this
//! way, what surprised someone, which scenario it serves), each in
//! `.agents/corpus/<name>.md`. A lesson says what to do; the corpus says
//! why. An entry opens with the paths it is relevant to, and an agent is
//! given it only when it opens one of them.
//!
//! ```text
//! ---
//! paths: src/billing/**, docs/billing.md
//! ---
//! Invoices round half to even: the ledger reconciles to the bank's.
//! ```

/// Where a repository keeps its corpus.
pub const DIR: &str = ".agents/corpus";

/// A path glob, split into its parts: `*` takes any characters but `/`,
/// `**` any number of whole parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glob(Vec<String>);

/// A path in the repository, split into its parts. The sandbox mounts the
/// repository at `/work/repo`, so that prefix is dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoPath(Vec<String>);

impl Glob {
    pub fn new(text: &str) -> Glob {
        Glob(RepoPath::new(text).0)
    }

    pub fn matches(&self, path: &RepoPath) -> bool {
        parts(&self.0, &path.0)
    }
}

impl RepoPath {
    pub fn new(text: &str) -> RepoPath {
        let text = text.strip_prefix("/work/repo/").unwrap_or(text);
        let text = text.trim_start_matches("./");
        RepoPath(text.split('/').map(String::from).collect())
    }
}

fn parts(glob: &[String], path: &[String]) -> bool {
    match (glob.first().map(String::as_str), path.first()) {
        (None, None) => true,
        (Some("**"), _) => parts(&glob[1..], path) || (!path.is_empty() && parts(glob, &path[1..])),
        (Some(g), Some(p)) => part(g.as_bytes(), p.as_bytes()) && parts(&glob[1..], &path[1..]),
        _ => false,
    }
}

/// Whether one part of a path matches one part of a glob, `*` within it,
/// compared byte by byte.
fn part(glob: &[u8], name: &[u8]) -> bool {
    let Some(star) = glob.iter().position(|b| *b == b'*') else {
        return glob == name;
    };
    let (head, tail) = (&glob[..star], &glob[star + 1..]);
    let Some(rest) = name.strip_prefix(head) else {
        return false;
    };
    (0..=rest.len()).any(|i| part(tail, &rest[i..]))
}

/// A corpus file: its name in `.agents/corpus/` and its text.
pub struct File<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

/// One corpus entry: its name, the globs it is relevant to, and its why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub paths: Vec<Glob>,
    pub text: String,
}

impl Entry {
    /// Whether the entry is relevant to `path`.
    pub fn covers(&self, path: &RepoPath) -> bool {
        self.paths.iter().any(|g| g.matches(path))
    }
}

/// An entry from its file. An entry with no `paths:` line is relevant
/// nowhere, so it is never given.
pub fn parse(file: File) -> Entry {
    let (head, text) = front_matter(&file);
    let line = head.lines().find_map(|l| l.trim().strip_prefix("paths:"));
    let globs = line.into_iter().flat_map(|list| list.split(','));
    let globs = globs.map(|g| g.trim().trim_matches(['"', '\'']));
    Entry {
        name: file.name.trim_end_matches(".md").into(),
        paths: globs.filter(|g| !g.is_empty()).map(Glob::new).collect(),
        text: text.trim().into(),
    }
}

/// The front matter between two `---` lines at the top, and the rest.
fn front_matter<'a>(file: &File<'a>) -> (&'a str, &'a str) {
    let Some(rest) = file.text.trim_start().strip_prefix("---") else {
        return ("", file.text);
    };
    match rest.split_once("\n---") {
        Some((head, body)) => (head, body.trim_start_matches('-')),
        None => ("", file.text),
    }
}

/// The entries relevant to `path` that `given` does not hold yet.
pub fn relevant<'a>(entries: &'a [Entry], path: &RepoPath, given: &[String]) -> Vec<&'a Entry> {
    let fresh = entries.iter().filter(|e| !given.contains(&e.name));
    fresh.filter(|e| e.covers(path)).collect()
}

/// The entries as an agent is given them, ahead of what its call returned.
pub fn told(entries: &[&Entry]) -> String {
    let each = entries
        .iter()
        .map(|e| format!("<corpus name=\"{}\">\n{}\n</corpus>", e.name, e.text));
    each.collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(case: (&str, &str)) -> bool {
        Glob::new(case.0).matches(&RepoPath::new(case.1))
    }

    #[test]
    fn globs_match_whole_parts_and_stars_within_one() {
        assert!(hits(("src/billing/**", "src/billing/invoice.rs")));
        assert!(hits((
            "src/billing/**",
            "/work/repo/src/billing/tax/vat.rs"
        )));
        assert!(hits(("src/**/mod.rs", "src/a/b/mod.rs")));
        assert!(hits(("src/*.rs", "src/main.rs")));
        assert!(hits(("docs/billing.md", "./docs/billing.md")));
        assert!(!hits(("src/*.rs", "src/a/main.rs")), "* stays in one part");
        assert!(!hits(("src/billing/**", "src/billingx/a.rs")));
        assert!(!hits(("docs/billing.md", "docs/billing.mdx")));
    }

    #[test]
    fn an_entry_reads_its_paths_and_is_given_once_where_it_is_relevant() {
        let text =
            "---\npaths: src/billing/**, \"docs/billing.md\"\n---\nInvoices round half to even.\n";
        let e = parse(File {
            name: "rounding.md",
            text,
        });
        assert_eq!(e.name, "rounding");
        assert_eq!(
            e.paths,
            [Glob::new("src/billing/**"), Glob::new("docs/billing.md")]
        );
        assert_eq!(e.text, "Invoices round half to even.");
        let all = [e];
        let billing = RepoPath::new("/work/repo/src/billing/a.rs");
        let hit = relevant(&all, &billing, &[]);
        assert_eq!(hit.len(), 1);
        assert!(relevant(&all, &RepoPath::new("src/main.rs"), &[]).is_empty());
        assert!(relevant(&all, &billing, &["rounding".into()]).is_empty());
        assert!(told(&hit).contains("<corpus name=\"rounding\">"));
        let loose = File {
            name: "loose.md",
            text: "No front matter.",
        };
        assert!(parse(loose).paths.is_empty());
    }
}
