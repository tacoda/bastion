//! The server's changes, numbered and pushed to every client (D10, D29).
//! A client that reconnects names the last number it saw and gets what it
//! missed, as long as the server still holds it.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use bastion_protocol::Change;
use tokio::sync::broadcast;

/// How many recent changes a reconnect can replay.
const KEPT: usize = 256;

pub struct Bus {
    tx: broadcast::Sender<(u64, Change)>,
    /// The latest changes, oldest first, and the next number to give.
    kept: Mutex<(VecDeque<(u64, Change)>, u64)>,
}

impl Default for Bus {
    fn default() -> Self {
        // Numbers start from the boot time, so a number from before a
        // restart is always older than any this server gives.
        let boot = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        Bus {
            tx: broadcast::channel(KEPT).0,
            kept: Mutex::new((VecDeque::new(), boot * 1000)),
        }
    }
}

impl Bus {
    pub fn publish(&self, change: Change) {
        let mut kept = self.kept.lock().unwrap();
        let n = kept.1;
        kept.1 += 1;
        kept.0.push_back((n, change.clone()));
        if kept.0.len() > KEPT {
            kept.0.pop_front();
        }
        // No subscriber is not an error.
        let _ = self.tx.send((n, change));
    }

    pub fn subscribe(&self) -> broadcast::Receiver<(u64, Change)> {
        self.tx.subscribe()
    }

    /// The changes after `last`. `None` when `last` is not one this server
    /// still holds, so the client must read everything again.
    pub fn since(&self, last: u64) -> Option<Vec<(u64, Change)>> {
        let kept = self.kept.lock().unwrap();
        let held = kept.0.front().map_or(kept.1, |(n, _)| *n);
        (held <= last + 1 && last < kept.1)
            .then(|| kept.0.iter().filter(|(n, _)| *n > last).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str) -> Change {
        Change::Job { id: id.into() }
    }

    #[test]
    fn a_reconnect_gets_what_it_missed_or_is_told_to_read_again() {
        let bus = Bus::default();
        let mut rx = bus.subscribe();
        bus.publish(job("a"));
        bus.publish(job("b"));
        let (first, _) = rx.try_recv().unwrap();
        assert_eq!(bus.since(first), Some(vec![(first + 1, job("b"))]));
        assert_eq!(bus.since(first + 1), Some(vec![]));
        assert_eq!(bus.since(first - 5), None, "older than the server holds");
        assert_eq!(bus.since(1), None, "a number from before a restart");
    }

    #[test]
    fn only_the_latest_changes_are_kept() {
        let bus = Bus::default();
        let mut rx = bus.subscribe();
        bus.publish(job("first"));
        let (first, _) = rx.try_recv().unwrap();
        // One more than the bus keeps, so the change after `first` is gone.
        for _ in 0..=KEPT {
            bus.publish(job("x"));
        }
        assert_eq!(bus.since(first), None);
        assert_eq!(bus.since(first + 1).map(|v| v.len()), Some(KEPT));
    }
}
