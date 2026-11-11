//! Screenshots and recordings a turn keeps (F11): files under
//! /work/artifacts in its VM, copied out after the turn and recorded on the
//! job, one artifact line each.

/// Where a turn keeps the files it wants a person to see.
pub const DIR: &str = "/work/artifacts";

/// How an artifact names a kept file.
const KEPT: &str = "Kept from /work/artifacts: ";

/// The kinds of file kept, by extension: images and recordings.
const KINDS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "webm", "mp4"];

/// Whether `name` is a plain file name of a kind kept.
pub fn kept(name: &str) -> bool {
    let plain = !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.');
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    plain && ext.is_some_and(|e| KINDS.contains(&e.as_str()))
}

/// The artifact that records a kept file.
pub fn artifact(name: &str) -> String {
    format!("{KEPT}{name}")
}

/// The kept file an artifact records, if it records one.
pub fn named(body: &str) -> Option<&str> {
    body.strip_prefix(KEPT).filter(|n| kept(n))
}

/// The content type a kept file is served with.
pub fn content_type(name: &str) -> &'static str {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("webm") => "video/webm",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_and_recordings_are_kept_by_their_plain_names() {
        assert!(kept("home.png") && kept("login flow.WEBM"));
        for bad in ["notes.txt", "../x.png", "a/b.png", ".png", ""] {
            assert!(!kept(bad), "{bad}");
        }
        assert_eq!(named(&artifact("home.png")), Some("home.png"));
        assert_eq!(named("Kept from /work/artifacts: ../x.png"), None);
        assert_eq!(named("Plan: home.png"), None);
        assert_eq!(content_type("run.webm"), "video/webm");
    }
}
