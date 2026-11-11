//! A request as a person writes it: what should change, its kind, the
//! criteria it must meet, the links that explain it, and the screenshots
//! that show it. The factory keeps it as one text, which a brief quotes.

/// The kinds a request may name.
pub const KINDS: [&str; 4] = ["feature", "bug", "chore", "docs"];

/// Where a job's screenshots are in its VM.
pub const SCREENSHOTS_DIR: &str = "/work/request";

/// The job result its screenshots' names are kept under, one per line.
pub const SCREENSHOTS: &str = "screenshots";

/// The image kinds a screenshot may be, by its name's extension.
const IMAGES: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

#[derive(Clone, Debug, Default)]
pub struct Request<'a> {
    pub text: &'a str,
    pub kind: Option<&'a str>,
    pub criteria: Option<&'a str>,
    pub links: &'a [String],
    pub screenshots: &'a [String],
}

/// The request as one text: what should change, then each part it names
/// under its own heading. Refused when a part cannot be used as given.
pub fn compose(r: &Request) -> Result<String, String> {
    problem(r).map_or(Ok(()), Err)?;
    let mut text = r.text.trim().to_string();
    let blank = |s: &&str| !s.trim().is_empty();
    if let Some(kind) = r.kind.filter(blank) {
        text.push_str(&format!("\n\nKind: {}", kind.trim()));
    }
    if let Some(criteria) = r.criteria.filter(blank) {
        text.push_str(&format!("\n\nAcceptance criteria:\n{}", criteria.trim()));
    }
    let listed = |items: &[String], at: &str| -> String {
        items
            .iter()
            .map(|i| format!("\n- {at}{}", i.trim()))
            .collect()
    };
    if !r.links.is_empty() {
        text.push_str(&format!("\n\nLinks:{}", listed(r.links, "")));
    }
    if !r.screenshots.is_empty() {
        let at = format!("{SCREENSHOTS_DIR}/");
        text.push_str(&format!("\n\nScreenshots:{}", listed(r.screenshots, &at)));
    }
    Ok(text)
}

/// Why a request cannot be used as given.
fn problem(r: &Request) -> Option<String> {
    if r.text.trim().is_empty() {
        return Some("a request needs to say what should change".into());
    }
    if let Some(kind) = r
        .kind
        .map(str::trim)
        .filter(|k| !k.is_empty() && !KINDS.contains(k))
    {
        return Some(format!("{kind:?} is not a kind: {}", KINDS.join(", ")));
    }
    let web = |l: &&String| l.starts_with("https://") || l.starts_with("http://");
    if let Some(link) = r.links.iter().find(|l| !web(l)) {
        return Some(format!("{link:?} is not a web address"));
    }
    let shot = r.screenshots.iter().find(|s| !screenshot_name(s));
    shot.map(|s| format!("{s:?} is not an image's file name"))
}

/// A plain file name with an image's extension: nothing that leaves the
/// screenshots' folder.
pub fn screenshot_name(name: &str) -> bool {
    let plain = !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.');
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    plain && ext.is_some_and(|e| IMAGES.contains(&e.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_says_its_kind_criteria_links_and_screenshots() {
        let links = ["https://example.com/issue/4".to_string()];
        let shots = ["broken.png".to_string()];
        let r = Request {
            text: "The total is wrong ",
            kind: Some("bug"),
            criteria: Some("The total of 2 and 2 is 4."),
            links: &links,
            screenshots: &shots,
        };
        assert_eq!(
            compose(&r).unwrap(),
            "The total is wrong\n\nKind: bug\n\nAcceptance criteria:\nThe total of 2 and 2 is 4.\
             \n\nLinks:\n- https://example.com/issue/4\n\nScreenshots:\n- /work/request/broken.png"
        );
        let plain = Request {
            text: "Add x",
            ..Request::default()
        };
        assert_eq!(compose(&plain).unwrap(), "Add x");
    }

    #[test]
    fn a_request_part_that_cannot_be_used_is_refused() {
        let bad = |r: Request| compose(&r).unwrap_err();
        assert!(bad(Request::default()).contains("what should change"));
        assert!(
            bad(Request {
                text: "x",
                kind: Some("epic"),
                ..Request::default()
            })
            .contains("not a kind")
        );
        let links = ["file:///etc/passwd".to_string()];
        assert!(
            bad(Request {
                text: "x",
                links: &links,
                ..Request::default()
            })
            .contains("web address")
        );
        for name in ["../x.png", "a/b.png", ".png", "notes.txt"] {
            assert!(!screenshot_name(name), "{name}");
        }
        assert!(screenshot_name("Screen Shot.PNG"));
    }
}
