//! A unified diff, one block per changed file (PLAN.md A4). Clicking a line
//! starts a comment on it, as `file:line: `, in the box the caller passes.

use leptos::prelude::*;

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

fn line_class(text: &str) -> &'static str {
    match text.chars().next() {
        Some('+') => "bg-ok/10 text-ok",
        Some('-') => "bg-bad/10 text-bad",
        Some('@') => "text-muted",
        _ => "",
    }
}

#[component]
pub fn DiffView(diff: String, comment: WriteSignal<String>) -> impl IntoView {
    // The adapter appends work the turn left uncommitted after the diff.
    let (diff, uncommitted) = match diff.split_once("\n\nNot committed, so it will not land:\n") {
        Some((d, rest)) => (d.to_string(), Some(rest.to_string())),
        None => (diff, None),
    };
    let warning = uncommitted.map(|files| {
        view! {
            <p class="m-0 rounded border border-warn p-2 text-sm text-warn">
                "Not committed, so it will not land: "<code>{files}</code>
            </p>
        }
    });
    let files = parse(&diff);
    if files.is_empty() {
        return view! { {warning}<p class="m-0 text-sm text-muted">{diff}</p> }.into_any();
    }
    view! {
        <div class="grid gap-3">
            {warning}
            {files.into_iter().map(|file| {
                let path = file.path.clone();
                view! {
                    <details open class="rounded border border-line">
                        <summary class="cursor-pointer px-3 py-1.5 font-mono text-sm">{file.path.clone()}</summary>
                        <div class="overflow-x-auto bg-bg font-mono text-xs [contain:inline-size]">
                            {file.lines.into_iter().map(|line| {
                                let at = line.new.map(|n| format!("{path}:{n}: "));
                                let number = line.new.map(|n| n.to_string()).unwrap_or_default();
                                let class = line_class(&line.text);
                                view! {
                                    <button class={format!("grid w-full grid-cols-[40px_1fr] text-left hover:outline hover:outline-1 hover:outline-primary {}", class)}
                                        title="Comment on this line"
                                        disabled=at.is_none()
                                        on:click=move |_| if let Some(at) = at.clone() {
                                            comment.update(|t| {
                                                if !t.is_empty() && !t.ends_with('\n') { t.push('\n'); }
                                                t.push_str(&at);
                                            });
                                        }>
                                        <span class="select-none pr-2 text-right text-muted">{number}</span>
                                        <span class="whitespace-pre">{line.text}</span>
                                    </button>
                                }
                            }).collect_view()}
                        </div>
                    </details>
                }
            }).collect_view()}
        </div>
    }
    .into_any()
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
    fn file_headers_are_not_lines() {
        let files = parse(
            "diff --git a/N.txt b/N.txt\nnew file mode 100644\nindex 0000000..1\n--- /dev/null\n+++ b/N.txt\n@@ -0,0 +1 @@\n+hi\n",
        );
        let texts: Vec<&str> = files[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["@@ -0,0 +1 @@", "+hi"]);
        assert_eq!(files[0].lines[1].new, Some(1));
    }
}
