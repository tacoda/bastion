//! A unified diff, one block per changed file (PLAN.md A4). Clicking a line
//! starts a comment on it, as `file:line: `.

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
        Some('+') => theme::OK,
        Some('-') => theme::BAD,
        Some('@') => theme::MUTED,
        _ => theme::INK,
    }
}

/// The adapter appends work the turn left uncommitted after the diff.
fn split(diff: &str) -> (&str, Option<&str>) {
    match diff.split_once("\n\nNot committed, so it will not land:\n") {
        Some((d, rest)) => (d, Some(rest)),
        None => (diff, None),
    }
}

// ponytail: every line is a widget; draw only the visible lines when a diff of a few thousand lines starts slowly (N14).
fn file_view<'a, M: Clone + 'a>(file: File, comment: fn(String) -> M) -> Element<'a, M> {
    let lines = file.lines.into_iter().map(|line| {
        let number = text(line.new.map(|n| n.to_string()).unwrap_or_default())
            .width(40)
            .color(theme::MUTED);
        let body = text(line.text.clone()).color(line_color(&line.text));
        let at = line.new.map(|n| comment(format!("{}:{n}: ", file.path)));
        let line = button(row![number, body].spacing(8))
            .padding([0, 4])
            .width(Fill)
            .style(theme::row_button);
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

/// The diff, one block per file; `comment` carries `file:line: ` for a click.
pub fn view<'a, M: Clone + 'a>(diff: &str, comment: fn(String) -> M) -> Element<'a, M> {
    let (diff, uncommitted) = split(diff);
    let mut all = Column::new().spacing(12);
    if let Some(files) = uncommitted {
        let warning = text(format!("Not committed, so it will not land: {files}"))
            .size(13)
            .color(theme::WARN);
        all = all.push(container(warning).padding(8).style(theme::accented));
    }
    let files = parse(diff);
    if files.is_empty() {
        return all
            .push(text(diff.to_string()).size(13).color(theme::MUTED))
            .into();
    }
    let blocks = files.into_iter().map(|f| file_view(f, comment));
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
}
