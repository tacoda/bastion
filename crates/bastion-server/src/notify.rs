//! The chat adapter (F9, D23): each notice posted as `{"text": ...}` to an
//! incoming webhook, the shape Slack, Mattermost and Discord's Slack mode
//! take. The webhook's address holds its credential, so it is kept in a
//! file only the server's user reads, and only a hint of it is ever shown.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use crate::machine::Machine;

/// The file under the server's data folder that holds the webhook address.
const FILE: &str = "notify-webhook";

pub struct Webhook {
    machine: Arc<dyn Machine>,
    path: PathBuf,
    http: reqwest::Client,
}

impl Webhook {
    pub fn new(machine: Arc<dyn Machine>, data: &std::path::Path) -> Self {
        Webhook {
            machine,
            path: data.join(FILE),
            http: reqwest::Client::new(),
        }
    }

    /// Keep `url` as the webhook, or forget it when empty.
    pub fn set(&self, url: &str) -> Result<(), String> {
        let url = url.trim();
        if !url.is_empty() && !url.starts_with("https://") {
            return Err("a webhook is an https:// address".into());
        }
        self.machine.remove(&self.path)?;
        if url.is_empty() {
            return Ok(());
        }
        self.machine.write_secret(&self.path, url.as_bytes())
    }

    /// The webhook, if one is set.
    fn url(&self) -> Option<String> {
        let kept = self.machine.read(&self.path).ok()?;
        Some(kept.trim().to_string()).filter(|u| !u.is_empty())
    }

    /// Enough of the webhook to recognize it, never enough to use it.
    pub fn hint(&self) -> Option<String> {
        self.url().map(|u| masked(&u))
    }

    /// Post `text` to the webhook. None set is not an error: there is
    /// nowhere to tell.
    pub async fn send(&self, text: &str) -> Result<(), String> {
        let Some(url) = self.url() else {
            return Ok(());
        };
        let sent = self.http.post(&url).json(&json!({ "text": text })).send();
        let answer = sent.await.map_err(|e| format!("the webhook: {e}"))?;
        match answer.status().is_success() {
            true => Ok(()),
            false => Err(format!("the webhook answered {}", answer.status())),
        }
    }
}

/// The host and the last four characters: which webhook, not the webhook.
fn masked(url: &str) -> String {
    let host = url
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or_default();
    let tail: String = url
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("https://{host}/…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::fake::FakeMachine;

    #[test]
    fn a_hint_names_the_host_and_hides_the_secret() {
        let url = "https://hooks.slack.com/services/T000/B000/abcdefgh1234";
        assert_eq!(masked(url), "https://hooks.slack.com/…1234");
    }

    #[test]
    fn only_an_https_webhook_is_kept_and_an_empty_one_forgets() {
        let machine = Arc::new(FakeMachine::new());
        let hook = Webhook::new(machine.clone(), std::path::Path::new("/data"));
        assert!(
            hook.set("http://plain.example/x")
                .unwrap_err()
                .contains("https://")
        );
        hook.set("https://hooks.example/x").unwrap();
        hook.set("").unwrap();
        let calls = machine.calls.lock().unwrap().clone();
        let at = "/data/notify-webhook";
        assert_eq!(
            calls,
            [
                format!("remove {at}"),
                format!("write-secret {at}"),
                format!("remove {at}")
            ]
        );
    }

    #[test]
    fn a_notice_is_posted_as_text_to_the_webhook() {
        crate::rt::block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let at = listener.local_addr().unwrap();
            let heard = tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let (mut conn, _) = listener.accept().await.unwrap();
                let mut got = vec![0; 4096];
                let n = conn.read(&mut got).await.unwrap();
                conn.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                    .await
                    .unwrap();
                String::from_utf8_lossy(&got[..n]).to_string()
            });
            let machine =
                FakeMachine::new().file("/data/notify-webhook", &format!("http://{at}/hook"));
            let hook = Webhook::new(Arc::new(machine), std::path::Path::new("/data"));
            hook.send("j1 landed").await.unwrap();
            let request = heard.await.unwrap();
            assert!(request.starts_with("POST /hook"), "{request}");
            assert!(request.ends_with(r#"{"text":"j1 landed"}"#), "{request}");
        });
    }
}
