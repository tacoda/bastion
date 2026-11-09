//! The audit writer (PLAN.md D21): the one writer of the chain. Records
//! queue on a channel and one task appends them in order. A record is kept
//! on disk until it is written, and a failed write is tried again with
//! backoff, so a store that is down delays records and loses none; the
//! status shows them pending with the last error. The newest entry is also
//! kept outside the database, so deleting entries from the end of the chain
//! breaks it. The chain is keyed with a secret kept in the Keychain.

use std::io::Write;
use std::path::{Path, PathBuf};
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
/// The longest wait between two tries of one write.
const MOST_WAIT: Duration = Duration::from_secs(30);
const SERVICE: &str = "bastion-audit";

/// Records not yet written, one JSON line each, oldest first.
#[derive(Clone)]
struct Spool {
    file: Option<PathBuf>,
    lock: Arc<Mutex<()>>,
}

impl Spool {
    fn load(&self) -> Vec<Record> {
        let Some(file) = &self.file else {
            return vec![];
        };
        let _held = self.lock.lock().unwrap();
        let text = std::fs::read_to_string(file).unwrap_or_default();
        text.lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    fn push(&self, record: &Record) {
        let Some(file) = &self.file else { return };
        let _held = self.lock.lock().unwrap();
        let line = serde_json::to_string(record).unwrap_or_default();
        let opened = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file);
        if let Err(e) = opened.and_then(|mut f| writeln!(f, "{line}")) {
            eprintln!("the audit spool {}: {e}", file.display());
        }
    }

    /// Drop the oldest record, once it is written.
    fn pop(&self) {
        let Some(file) = &self.file else { return };
        let _held = self.lock.lock().unwrap();
        let text = std::fs::read_to_string(file).unwrap_or_default();
        let rest: String = text.lines().skip(1).map(|l| format!("{l}\n")).collect();
        if let Err(e) = std::fs::write(file, rest) {
            eprintln!("the audit spool {}: {e}", file.display());
        }
    }
}

/// The newest entry's sequence and hash, kept outside the database.
#[derive(Clone)]
struct Anchor(Option<PathBuf>);

impl Anchor {
    fn save(&self, e: &Entry) {
        if let Some(file) = &self.0
            && let Err(err) = std::fs::write(file, format!("{} {}", e.seq, e.hash))
        {
            eprintln!("the audit anchor {}: {err}", file.display());
        }
    }

    fn load(&self) -> Option<(u64, String)> {
        let text = std::fs::read_to_string(self.0.as_ref()?).ok()?;
        let (seq, hash) = text.trim().split_once(' ')?;
        Some((seq.parse().ok()?, hash.into()))
    }

    /// The anchored entry's `seq` when the chain no longer holds it as kept.
    fn missing(&self, chain: &[Entry]) -> Option<u64> {
        let (seq, hash) = self.load()?;
        let held = chain.iter().any(|e| e.seq == seq && e.hash == hash);
        (!held).then_some(seq)
    }
}

/// What the writer needs: where records go, the key, how long one try may
/// take, and the directory that keeps records on disk (none keeps them in
/// memory only).
pub struct Writer {
    pub store: Arc<dyn Store>,
    pub key: Vec<u8>,
    pub limit: Duration,
    pub dir: Option<PathBuf>,
}

pub struct AuditLog {
    store: Arc<dyn Store>,
    key: Vec<u8>,
    records: mpsc::UnboundedSender<Record>,
    pending: Arc<AtomicU64>,
    last_error: Arc<Mutex<Option<String>>>,
    spool: Spool,
    anchor: Anchor,
}

/// The chain as read, and the first entry that breaks it, if one does.
pub struct Read {
    pub chain: Vec<Entry>,
    pub broken_at: Option<u64>,
}

/// The writing task's view of the log.
struct Task {
    store: Arc<dyn Store>,
    key: Vec<u8>,
    limit: Duration,
    pending: Arc<AtomicU64>,
    last_error: Arc<Mutex<Option<String>>>,
    spool: Spool,
    anchor: Anchor,
}

impl AuditLog {
    pub fn start(w: Writer) -> Arc<Self> {
        let file = |name: &str| w.dir.as_ref().map(|d| d.join(name));
        let spool = Spool {
            file: file("audit-spool.jsonl"),
            lock: Arc::default(),
        };
        let anchor = Anchor(file("audit-anchor"));
        // ponytail: an unbounded queue, kept on disk too; it grows only while the store is down.
        let (records, queue) = mpsc::unbounded_channel::<Record>();
        let earlier = spool.load();
        let pending = Arc::new(AtomicU64::new(earlier.len() as u64));
        for record in earlier {
            let _ = records.send(record);
        }
        let log = Arc::new(Self {
            store: w.store.clone(),
            key: w.key.clone(),
            records,
            pending: pending.clone(),
            last_error: Arc::default(),
            spool: spool.clone(),
            anchor: anchor.clone(),
        });
        let task = Task {
            store: w.store,
            key: w.key,
            limit: w.limit,
            pending,
            last_error: log.last_error.clone(),
            spool,
            anchor,
        };
        crate::rt::spawn(task.run(queue));
        log
    }

    /// Queue a record and keep it on disk until it is written. It never
    /// waits on the store.
    pub fn note(&self, record: Record) {
        self.spool.push(&record);
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

    /// The whole chain, verified under the key and against its anchor.
    pub async fn read(&self) -> Result<Read, String> {
        let chain = self.store.audit().await?;
        let broken_at = audit::verify(&chain, &self.key)
            .err()
            .or_else(|| self.anchor.missing(&chain));
        Ok(Read { chain, broken_at })
    }
}

impl Task {
    async fn run(self, mut queue: mpsc::UnboundedReceiver<Record>) {
        // `None` until the chain's tail is read, then the tail.
        let mut tail: Option<Option<Entry>> = None;
        while let Some(record) = queue.recv().await {
            self.write(&mut tail, record).await;
            self.spool.pop();
            self.pending.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// Write one record, trying again with backoff until it is written.
    async fn write(&self, tail: &mut Option<Option<Entry>>, record: Record) {
        let mut wait = self.limit.min(Duration::from_secs(1));
        loop {
            let tried = tokio::time::timeout(self.limit, self.attempt(tail, record.clone())).await;
            let error = match tried {
                Ok(Ok(entry)) => {
                    self.anchor.save(&entry);
                    *tail = Some(Some(entry));
                    *self.last_error.lock().unwrap() = None;
                    return;
                }
                Ok(Err(e)) => format!("the audit record was not written: {e}"),
                Err(_) => format!("an audit write took longer than {}s", self.limit.as_secs()),
            };
            eprintln!("{error}; trying again in {wait:?}");
            *self.last_error.lock().unwrap() = Some(error);
            // The tail is read again, in case the failed write landed.
            *tail = None;
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(MOST_WAIT);
        }
    }

    async fn attempt(
        &self,
        tail: &mut Option<Option<Entry>>,
        record: Record,
    ) -> Result<Entry, String> {
        if tail.is_none() {
            *tail = Some(self.store.audit_tail().await?);
        }
        let last = tail.as_ref().and_then(Option::as_ref);
        let entry = audit::append(last, now(), record, &self.key);
        self.store.append_audit(&entry).await?;
        Ok(entry)
    }
}

/// The spool and anchor directory's files, for tests and tools.
pub fn files(dir: &Path) -> [PathBuf; 2] {
    [dir.join("audit-spool.jsonl"), dir.join("audit-anchor")]
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

    fn writer(store: Arc<FakeStore>, limit: Duration, dir: Option<PathBuf>) -> Arc<AuditLog> {
        AuditLog::start(Writer {
            store,
            key: b"k".to_vec(),
            limit,
            dir,
        })
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bastion-audit-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn records_chain_in_order_under_the_key() {
        let store = Arc::new(FakeStore::default());
        let log = writer(store.clone(), WRITE_LIMIT, None);
        log.note(record("one"));
        log.note(record("two"));
        settle();
        let read = crate::rt::block_on(log.read()).unwrap();
        assert_eq!(read.chain.len(), 2);
        assert_eq!(read.broken_at, None);
        assert_eq!(audit::verify(&read.chain, b"another key"), Err(1));
        assert_eq!(log.status(), (0, None));
    }

    #[test]
    fn a_store_that_is_down_delays_records_and_loses_none() {
        let store = Arc::new(FakeStore::default());
        *store.stall_audit.lock().unwrap() = true;
        let log = writer(store.clone(), Duration::from_millis(50), None);
        let started = std::time::Instant::now();
        log.note(record("one"));
        log.note(record("two"));
        assert!(
            started.elapsed() < Duration::from_millis(10),
            "noting must not wait"
        );
        std::thread::sleep(Duration::from_millis(250));
        let (pending, error) = log.status();
        assert_eq!(pending, 2);
        assert!(error.unwrap().contains("longer than"));
        *store.stall_audit.lock().unwrap() = false;
        std::thread::sleep(Duration::from_millis(800));
        assert_eq!(log.status(), (0, None));
        assert_eq!(store.audit.lock().unwrap().len(), 2);
    }

    #[test]
    fn records_kept_on_disk_are_written_after_a_restart() {
        let dir = scratch("spool");
        let store = Arc::new(FakeStore::default());
        *store.stall_audit.lock().unwrap() = true;
        let before = writer(store.clone(), Duration::from_millis(50), Some(dir.clone()));
        before.note(record("noted before the restart"));
        drop(before);
        let fresh = Arc::new(FakeStore::default());
        let after = writer(fresh.clone(), WRITE_LIMIT, Some(dir.clone()));
        settle();
        assert_eq!(after.status(), (0, None));
        assert_eq!(
            fresh.audit.lock().unwrap()[0].record.detail,
            "noted before the restart"
        );
        let [spool, _] = files(&dir);
        assert_eq!(std::fs::read_to_string(spool).unwrap(), "");
    }

    #[test]
    fn deleting_the_newest_entries_breaks_the_chain() {
        let dir = scratch("anchor");
        let store = Arc::new(FakeStore::default());
        let log = writer(store.clone(), WRITE_LIMIT, Some(dir));
        log.note(record("one"));
        log.note(record("two"));
        settle();
        assert_eq!(crate::rt::block_on(log.read()).unwrap().broken_at, None);
        store.audit.lock().unwrap().pop();
        assert_eq!(crate::rt::block_on(log.read()).unwrap().broken_at, Some(2));
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
