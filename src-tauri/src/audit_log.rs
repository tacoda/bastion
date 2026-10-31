//! The audit writer (PLAN.md D21): the one writer of the chain. Records
//! queue on a channel and one task appends them in order, each under a time
//! limit, so a stalled store shows as pending records and an error instead
//! of a stalled turn. The chain is keyed with a secret kept in the Keychain,
//! not in the database.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bastion_core::audit::{self, Entry, Record};
use tokio::sync::mpsc;

use crate::factory::now;
use crate::machine::Machine;
use crate::store::Store;

/// How long one append may take before it counts as stalled.
pub const WRITE_LIMIT: Duration = Duration::from_secs(5);
const SERVICE: &str = "bastion-audit";

pub struct AuditLog {
    store: Arc<dyn Store>,
    key: Vec<u8>,
    records: mpsc::UnboundedSender<Record>,
    pending: Arc<AtomicU64>,
    last_error: Arc<Mutex<Option<String>>>,
}

/// The chain as read, and the first entry that breaks it, if one does.
pub struct Read {
    pub chain: Vec<Entry>,
    pub broken_at: Option<u64>,
}

impl AuditLog {
    pub fn start(store: Arc<dyn Store>, key: Vec<u8>, limit: Duration) -> Arc<Self> {
        let (records, mut queue) = mpsc::unbounded_channel::<Record>();
        let log = Arc::new(Self {
            store: store.clone(),
            key: key.clone(),
            records,
            pending: Arc::new(AtomicU64::new(0)),
            last_error: Arc::new(Mutex::new(None)),
        });
        let (pending, last_error) = (log.pending.clone(), log.last_error.clone());
        tauri::async_runtime::spawn(async move {
            while let Some(what) = queue.recv().await {
                let written = tokio::time::timeout(limit, append(&*store, &key, what)).await;
                let error = match written {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(format!("the audit record was not written: {e}")),
                    Err(_) => Some(format!(
                        "an audit write took longer than {}s",
                        limit.as_secs()
                    )),
                };
                if let Some(e) = &error {
                    eprintln!("{e}");
                }
                *last_error.lock().unwrap() = error;
                pending.fetch_sub(1, Ordering::Relaxed);
            }
        });
        log
    }

    /// Queue a record. It never waits on the store.
    pub fn note(&self, record: Record) {
        self.pending.fetch_add(1, Ordering::Relaxed);
        if self.records.send(record).is_err() {
            self.pending.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Records queued and not yet written, and the last write's error.
    pub fn status(&self) -> (u64, Option<String>) {
        (
            self.pending.load(Ordering::Relaxed),
            self.last_error.lock().unwrap().clone(),
        )
    }

    /// The whole chain, verified under the key as it is read.
    pub async fn read(&self) -> Result<Read, String> {
        let chain = self.store.audit().await?;
        let broken_at = audit::verify(&chain, &self.key).err();
        Ok(Read { chain, broken_at })
    }
}

/// Append the entry after the last one in the store.
// ponytail: reads the whole chain to find its tail; read only the last row when the chain grows large.
async fn append(store: &dyn Store, key: &[u8], what: Record) -> Result<(), String> {
    let chain = store.audit().await?;
    let entry = audit::append(chain.last(), now(), what, key);
    store.append_audit(&entry).await
}

/// The chain's key from the macOS Keychain, made on first use.
// ponytail: `security -w` takes the new key as an argument, so it shows in the process list for a moment; use the Keychain API if that matters.
pub fn key(machine: &dyn Machine) -> Result<Vec<u8>, String> {
    let account = ["-s", SERVICE, "-a", "bastion"];
    let found = machine.run(
        "security",
        &[&["find-generic-password"][..], &account, &["-w"]].concat(),
        None,
    )?;
    if found.success {
        return unhex(found.text.trim());
    }
    let key: Vec<u8> = [uuid::Uuid::new_v4(), uuid::Uuid::new_v4()]
        .iter()
        .flat_map(|u| *u.as_bytes())
        .collect();
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let made = machine.run(
        "security",
        &[&["add-generic-password"][..], &account, &["-w", &hex]].concat(),
        None,
    )?;
    if !made.success {
        return Err(format!(
            "the Keychain refused the audit key: {}",
            made.text.trim()
        ));
    }
    Ok(key)
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let bad = || "the audit key in the Keychain is not hex".to_string();
    if text.len() != 64 {
        return Err(bad());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| bad()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::fake::FakeMachine;
    use crate::store::fake::FakeStore;

    fn record(detail: &str) -> Record {
        Record {
            kind: "gate".into(),
            subject: "j1".into(),
            detail: detail.into(),
        }
    }

    fn settle() {
        std::thread::sleep(Duration::from_millis(100));
    }

    #[test]
    fn records_chain_in_order_under_the_key() {
        let store = Arc::new(FakeStore::default());
        let log = AuditLog::start(store.clone(), b"k".to_vec(), WRITE_LIMIT);
        log.note(record("one"));
        log.note(record("two"));
        settle();
        let read = tauri::async_runtime::block_on(log.read()).unwrap();
        assert_eq!(read.chain.len(), 2);
        assert_eq!(read.broken_at, None);
        assert_eq!(audit::verify(&read.chain, b"another key"), Err(1));
        assert_eq!(log.status(), (0, None));
    }

    #[test]
    fn a_stalled_store_shows_as_pending_and_then_an_error() {
        let store = Arc::new(FakeStore::default());
        *store.stall_audit.lock().unwrap() = true;
        let log = AuditLog::start(store, b"k".to_vec(), Duration::from_millis(50));
        let started = std::time::Instant::now();
        log.note(record("one"));
        log.note(record("two"));
        assert!(
            started.elapsed() < Duration::from_millis(10),
            "noting must not wait"
        );
        assert_eq!(log.status().0, 2);
        std::thread::sleep(Duration::from_millis(250));
        let (pending, error) = log.status();
        assert_eq!(pending, 0);
        assert!(error.unwrap().contains("longer than"));
    }

    #[test]
    fn the_key_is_read_from_the_keychain_or_made() {
        let hex = "ab".repeat(32);
        let found = FakeMachine::new().run_ok("security", &hex);
        assert_eq!(key(&found).unwrap(), vec![0xab; 32]);
        let refused = FakeMachine::new().run_fails("security", "no keychain");
        assert!(key(&refused).is_err());
    }
}
