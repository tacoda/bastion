//! The secrets a text holds, by kind, so a change that adds one is found
//! at review. A finding names what it found, never the secret itself.

/// Prefixes that start a token, and what the token is.
const TOKENS: [(&str, &str); 8] = [
    ("ghp_", "a GitHub token"),
    ("gho_", "a GitHub token"),
    ("ghs_", "a GitHub token"),
    ("github_pat_", "a GitHub token"),
    ("sk-ant-", "an Anthropic key"),
    ("sk-", "an API key"),
    ("xoxb-", "a Slack token"),
    ("xoxp-", "a Slack token"),
];
/// How long the part after a prefix must be to read as a token.
const TOKEN_LEN: usize = 16;

/// What secrets `text` holds, by kind, each named once.
pub fn secrets(text: &str) -> Vec<String> {
    let mut kinds: Vec<&str> = Vec::new();
    for word in text.split(|c: char| !in_token(c)) {
        match secret(word) {
            Some(kind) if !kinds.contains(&kind) => kinds.push(kind),
            _ => {}
        }
    }
    if text.contains("PRIVATE KEY-----") {
        kinds.push("a private key");
    }
    kinds.into_iter().map(|k| format!("it holds {k}")).collect()
}

/// Whether `c` can be part of a token.
fn in_token(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_-".contains(c)
}

/// What secret one word is, if it is one.
fn secret(word: &str) -> Option<&'static str> {
    let token = TOKENS.iter().find_map(|(prefix, kind)| {
        let rest = word.strip_prefix(prefix)?;
        (rest.len() >= TOKEN_LEN).then_some(*kind)
    });
    let aws = word.len() == 20
        && word.starts_with("AKIA")
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    token.or(aws.then_some("an AWS access key"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_named_by_kind_never_by_value() {
        assert!(secrets("Support the sk-ant- prefix in the key form").is_empty());
        let key = format!("ghp_{}", "a1".repeat(10));
        let found = secrets(&format!("token={key} and AKIAABCDEFGHIJKLMNOP"));
        assert_eq!(
            found,
            ["it holds a GitHub token", "it holds an AWS access key"]
        );
        assert!(found.iter().all(|f| !f.contains(&key)));
        assert_eq!(
            secrets("-----BEGIN RSA PRIVATE KEY-----"),
            ["it holds a private key"]
        );
    }
}
