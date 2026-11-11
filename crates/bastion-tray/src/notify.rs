//! The operating system's own notification for a notice (F9): through
//! `osascript` on macOS and `notify-send` on Linux, so the client needs no
//! notification library. A machine without either shows none.

use std::process::{Command, Stdio};

/// Show `text` as a notification from Bastion, without waiting for it.
pub fn show(text: &str) {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("osascript");
        c.args(["-e", &applescript(text)]);
        c
    } else {
        let mut c = Command::new("notify-send");
        c.args(["Bastion", first_line(text)]);
        c
    };
    let quiet = command.stdout(Stdio::null()).stderr(Stdio::null());
    if let Err(e) = quiet.spawn() {
        eprintln!("no notification shown: {e}");
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

/// The AppleScript that shows `text`, quoted so that nothing in it is run.
fn applescript(text: &str) -> String {
    let quoted = first_line(text).replace('\\', "\\\\").replace('"', "\\\"");
    format!("display notification \"{quoted}\" with title \"Bastion\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_is_quoted_so_its_text_cannot_run() {
        assert_eq!(
            applescript("j1 landed: Add \"x\" \\ y\ndo shell script \"rm\""),
            r#"display notification "j1 landed: Add \"x\" \\ y" with title "Bastion""#
        );
    }
}
