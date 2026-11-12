//! A unified diff, one block per changed file. Clicking a line
//! starts a comment on it, as `file:line: `; a shift-click on another line
//! of the same file widens it to a range, as `file:from-to: `.

use iced::widget::{Column, button, column, container, row, text};
use iced::{Color, Element, Fill};

use crate::theme;

struct Line {
    text: String,
    /// The line's number in the new file, when it has one.
    new: Option<u32>,
}

struct File {
    path: String,
    lines: Vec<Line>,
}

/// The lines a comment is about: one line of a file, or a range of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Pick {
    pub path: String,
    pub from: u32,
    pub to: u32,
}

impl Pick {
    pub fn line(path: String, n: u32) -> Self {
        Pick {
            path,
            from: n,
            to: n,
        }
    }

    /// This pick widened to take in line `n`, if `n` is in the same file.
    pub fn to(&self, path: &str, n: u32) -> Option<Self> {
        (self.path == path).then(|| Pick {
            path: path.to_string(),
            from: self.from.min(n),
            to: self.to.max(n),
        })
    }

    /// How a note names it: `file:4: ` or `file:4-9: `.
    pub fn reference(&self) -> String {
        match self.from == self.to {
            true => format!("{}:{}: ", self.path, self.from),
            false => format!("{}:{}-{}: ", self.path, self.from, self.to),
        }
    }

    fn holds(&self, path: &str, n: u32) -> bool {
        self.path == path && (self.from..=self.to).contains(&n)
    }
}

/// Split a `git diff` into files, numbering each line as the new file has it.
fn parse(diff: &str) -> Vec<File> {
    let mut files: Vec<File> = Vec::new();
    let mut next = 0u32;
    for raw in diff.lines() {
        if raw.starts_with("diff --git ") {
            let path = raw.rsplit(" b/").next().unwrap_or(raw).to_string();
            files.push(File {
                path,
                lines: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        let header = [
            "+++ ",
            "--- ",
            "index ",
            "new file mode",
            "deleted file mode",
            "similarity",
            "rename ",
            "old mode",
            "new mode",
        ];
        if header.iter().any(|h| raw.starts_with(h)) {
            continue;
        }
        let new = if let Some(hunk) = raw.strip_prefix("@@ ") {
            // "@@ -a,b +c,d @@": the new file's numbering starts at c.
            next = hunk
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            None
        } else if raw.starts_with('-') {
            None
        } else {
            next += 1;
            Some(next - 1)
        };
        file.lines.push(Line {
            text: raw.to_string(),
            new,
        });
    }
    files
}

/// A line's color: added, removed, a hunk header, or as it was.
fn line_color(text: &str) -> Color {
    match text.chars().next() {
        Some('+') => theme::ok(),
        Some('-') => theme::bad(),
        Some('@') => theme::muted(),
        _ => theme::ink(),
    }
}

/// The adapter appends work the turn left uncommitted after the diff.
fn split(diff: &str) -> (&str, Option<&str>) {
    match diff.split_once("\n\nNot committed, so it will not land:\n") {
        Some((d, rest)) => (d, Some(rest)),
        None => (diff, None),
    }
}

// ponytail: every line is a widget; draw only the visible lines when a diff of a few thousand lines starts slowly.
fn file_view<'a, M: Clone + 'a>(
    file: File,
    comment: fn(String, u32) -> M,
    pick: Option<&Pick>,
) -> Element<'a, M> {
    let lines = file.lines.into_iter().map(|line| {
        let number = text(line.new.map(|n| n.to_string()).unwrap_or_default())
            .font(theme::MONO)
            .size(13)
            .width(40)
            .color(theme::muted());
        let body = text(line.text.clone())
            .font(theme::MONO)
            .size(13)
            .color(line_color(&line.text));
        let at = line.new.map(|n| comment(file.path.clone(), n));
        let picked = line
            .new
            .is_some_and(|n| pick.is_some_and(|p| p.holds(&file.path, n)));
        let line = button(row![number, body].spacing(8))
            .padding([0, 4])
            .width(Fill)
            .style(move |t, status| match picked {
                true => theme::picked_row(t, status),
                false => theme::row_button(t, status),
            });
        line.on_press_maybe(at).into()
    });
    let lines = Column::with_children(lines);
    let head = container(text(file.path.clone()).font(theme::MONO).size(13)).padding([6, 12]);
    let body = container(lines)
        .padding([4, 0])
        .width(Fill)
        .style(theme::code);
    container(column![head, body]).style(theme::panel).into()
}

/// The diff, one block per file; `comment` carries the file and the line
/// clicked, and the lines of `pick` show as picked.
pub fn view<'a, M: Clone + 'a>(
    diff: &str,
    comment: fn(String, u32) -> M,
    pick: Option<&Pick>,
) -> Element<'a, M> {
    let (diff, uncommitted) = split(diff);
    let mut all = Column::new().spacing(12);
    if let Some(files) = uncommitted {
        let warning = text(format!("Not committed, so it will not land: {files}"))
            .size(13)
            .color(theme::warn());
        all = all.push(container(warning).padding(8).style(theme::accented));
    }
    let files = parse(diff);
    if files.is_empty() {
        return all
            .push(text(diff.to_string()).size(13).color(theme::muted()))
            .into();
    }
    let blocks = files.into_iter().map(|f| file_view(f, comment, pick));
    all.extend(blocks).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/calc.py b/calc.py\nindex 1..2 100644\n--- a/calc.py\n+++ b/calc.py\n@@ -1,2 +1,5 @@\n def add(a, b):\n     return a + b\n+\n+def divide(a, b):\n+    return a / b\n";

    #[test]
    fn lines_get_their_numbers_in_the_new_file() {
        let files = parse(DIFF);
        assert_eq!(files[0].path, "calc.py");
        let numbered: Vec<(Option<u32>, &str)> = files[0]
            .lines
            .iter()
            .map(|l| (l.new, l.text.as_str()))
            .collect();
        assert_eq!(numbered[0], (None, "@@ -1,2 +1,5 @@"));
        assert_eq!(numbered[1], (Some(1), " def add(a, b):"));
        assert_eq!(numbered[4], (Some(4), "+def divide(a, b):"));
    }

    #[test]
    fn uncommitted_work_is_split_off() {
        let (diff, rest) = split("d\n\nNot committed, so it will not land:\nN.txt");
        assert_eq!((diff, rest), ("d", Some("N.txt")));
        assert_eq!(split("d"), ("d", None));
    }

    #[test]
    fn file_headers_are_not_lines() {
        let files = parse(
            "diff --git a/N.txt b/N.txt\nnew file mode 100644\nindex 0000000..1\n--- /dev/null\n+++ b/N.txt\n@@ -0,0 +1 @@\n+hi\n",
        );
        let texts: Vec<&str> = files[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["@@ -0,0 +1 @@", "+hi"]);
        assert_eq!(files[0].lines[1].new, Some(1));
    }

    #[test]
    fn a_pick_widens_within_its_file_only() {
        let one = Pick::line("calc.py".into(), 12);
        assert_eq!(one.reference(), "calc.py:12: ");
        let range = one.to("calc.py", 8).unwrap();
        assert_eq!(range.reference(), "calc.py:8-12: ");
        assert!(range.holds("calc.py", 10) && !range.holds("calc.py", 13));
        assert_eq!(one.to("other.py", 20), None);
    }
}
