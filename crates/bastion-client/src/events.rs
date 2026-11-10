//! A Bastion server's pushes (PLAN.md D29): its `GET /events` stream, read
//! again with `Last-Event-ID` after it drops.

use std::time::Duration;

use bastion_protocol::{Change, Server};
use futures::{SinkExt, StreamExt};

/// What a server's stream says.
#[derive(Clone, Debug)]
pub enum Feed {
    /// The stream is open: the server answers.
    Up,
    /// The stream dropped: the server cannot be reached until it is back.
    Down,
    Change(Change),
    /// Changes were missed: read everything again.
    Resync,
}

/// Follow `server`'s pushes into `out`, each tagged with its name, for as
/// long as `out` is open: read the stream, and after it drops say so and
/// read it again.
pub async fn follow(server: Server, mut out: Out) {
    let http = crate::client::stream_http();
    let mut last = None;
    while !out.is_closed() {
        let _ = read(&http, &server, &mut last, &mut out).await;
        let _ = out.send((server.name.clone(), Feed::Down)).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Where a server's pushes go.
pub type Out = futures::channel::mpsc::Sender<(String, Feed)>;

/// Read one connection until it ends.
async fn read(
    http: &reqwest::Client,
    server: &Server,
    last: &mut Option<String>,
    out: &mut Out,
) -> Result<(), String> {
    let url = format!("{}/events", server.url.trim_end_matches('/'));
    let mut request = http.get(url).bearer_auth(&server.token);
    if let Some(id) = last.as_deref() {
        request = request.header("Last-Event-ID", id);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("events: status {}", response.status()));
    }
    let send = |feed| (server.name.clone(), feed);
    let _ = out.send(send(Feed::Up)).await;
    // A reconnect may have missed changes the server did not replay.
    if last.is_some() {
        let _ = out.send(send(Feed::Resync)).await;
    }
    let mut body = response.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = body.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk.map_err(|e| e.to_string())?));
        while let Some(end) = buffer.find("\n\n") {
            let block: String = buffer.drain(..end + 2).collect();
            for feed in parse(&block, last) {
                let _ = out.send(send(feed)).await;
            }
        }
    }
    Ok(())
}

/// One server-sent event: an id and a change, or a request to read again.
fn parse(block: &str, last: &mut Option<String>) -> Vec<Feed> {
    let mut feeds = Vec::new();
    for line in block.lines() {
        if let Some(id) = line.strip_prefix("id: ") {
            *last = Some(id.to_string());
        } else if line == "event: resync" {
            feeds.push(Feed::Resync);
        } else if let Some(change) = line
            .strip_prefix("data: ")
            .and_then(|d| serde_json::from_str(d).ok())
        {
            feeds.push(Feed::Change(change));
        }
    }
    feeds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_keeps_its_id_and_a_resync_asks_to_read_again() {
        let mut last = None;
        let feeds = parse(
            "id: 7\ndata: {\"kind\":\"job\",\"id\":\"j1\"}\n\n",
            &mut last,
        );
        assert_eq!(last.as_deref(), Some("7"));
        assert!(matches!(&feeds[..], [Feed::Change(Change::Job { id })] if id == "j1"));
        assert!(matches!(
            &parse("event: resync\ndata: {}\n\n", &mut last)[..],
            [Feed::Resync]
        ));
        assert!(parse(": keep-alive\n\n", &mut last).is_empty());
    }
}
