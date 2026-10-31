//! The audit chain (PLAN.md D21). Each entry carries the hash of the one
//! before it, so a change to any past entry breaks every hash after it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The `prev` of the first entry.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// What one entry records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// What happened, such as `gate` or `promote`.
    pub kind: String,
    /// What it happened to: a job id, or a constraint or capability.
    pub subject: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// 1 for the first entry, then one more for each.
    pub seq: u64,
    pub at: u64,
    #[serde(flatten)]
    pub record: Record,
    pub prev: String,
    pub hash: String,
}

/// The entry after `last`. The one writer appends only through this.
pub fn append(last: Option<&Entry>, at: u64, record: Record) -> Entry {
    let (seq, prev) = last.map_or((1, GENESIS.to_string()), |e| (e.seq + 1, e.hash.clone()));
    let mut entry = Entry {
        seq,
        at,
        record,
        prev,
        hash: String::new(),
    };
    entry.hash = hash(&entry);
    entry
}

/// Walk a whole chain from its first entry. On a break, give the `seq` of
/// the first entry that does not follow from the one before it.
pub fn verify(chain: &[Entry]) -> Result<(), u64> {
    let mut last: Option<&Entry> = None;
    for entry in chain {
        if !follows(entry, last) {
            return Err(entry.seq);
        }
        last = Some(entry);
    }
    Ok(())
}

/// The entry is the one `append` would have written after `last`.
fn follows(entry: &Entry, last: Option<&Entry>) -> bool {
    let (seq, prev) = last.map_or((1, GENESIS), |e| (e.seq + 1, e.hash.as_str()));
    let linked = entry.seq == seq && entry.prev == prev;
    linked && entry.hash == hash(entry)
}

/// Each field goes in with its length first, so no two entries hash the same
/// bytes.
fn hash(e: &Entry) -> String {
    let mut h = Sha256::new();
    h.update(e.seq.to_le_bytes());
    h.update(e.at.to_le_bytes());
    for field in [&e.record.kind, &e.record.subject, &e.record.detail, &e.prev] {
        h.update((field.len() as u64).to_le_bytes());
        h.update(field.as_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: &str, subject: &str, detail: &str) -> Record {
        Record {
            kind: kind.into(),
            subject: subject.into(),
            detail: detail.into(),
        }
    }

    fn chain() -> Vec<Entry> {
        let mut chain: Vec<Entry> = Vec::new();
        for (kind, detail) in [
            ("gate", "continue sandbox::exec"),
            ("gate", "hold sandbox::fs::write"),
            ("promote", "keep-tests checked"),
        ] {
            let next = append(
                chain.last(),
                100 + chain.len() as u64,
                record(kind, "j1", detail),
            );
            chain.push(next);
        }
        chain
    }

    #[test]
    fn a_chain_as_written_verifies() {
        let chain = chain();
        assert_eq!(chain[0].prev, GENESIS);
        assert_eq!(chain[2].seq, 3);
        assert_eq!(verify(&chain), Ok(()));
        assert_eq!(verify(&[]), Ok(()));
    }

    #[test]
    fn one_changed_byte_breaks_the_chain() {
        let mut chain = chain();
        chain[1].record.detail = "continue sandbox::fs::write".into();
        assert_eq!(verify(&chain), Err(2));
    }

    #[test]
    fn a_rehashed_entry_still_breaks_the_next_one() {
        let mut chain = chain();
        chain[1].record.detail = "continue sandbox::fs::write".into();
        chain[1].hash = hash(&chain[1]);
        assert_eq!(verify(&chain), Err(3));
    }

    #[test]
    fn a_dropped_or_reordered_entry_breaks_the_chain() {
        let mut chain = chain();
        chain.remove(1);
        assert_eq!(verify(&chain), Err(3));
        let mut chain = self::tests::chain();
        chain.swap(0, 1);
        assert_eq!(verify(&chain), Err(2));
    }

    #[test]
    fn field_boundaries_change_the_hash() {
        let a = append(None, 1, record("gate", "j1x", "y"));
        let b = append(None, 1, record("gate", "j1", "xy"));
        assert_ne!(a.hash, b.hash);
    }
}
