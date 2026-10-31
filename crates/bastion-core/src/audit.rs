//! The audit chain (PLAN.md D21). Each entry carries the hash of the one
//! before it, so a change to any past entry breaks every hash after it.
//! The hash is an HMAC under a key kept outside the record, so whoever can
//! edit the record cannot also recompute the chain without that key.

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
pub fn append(last: Option<&Entry>, at: u64, record: Record, key: &[u8]) -> Entry {
    let (seq, prev) = last.map_or((1, GENESIS.to_string()), |e| (e.seq + 1, e.hash.clone()));
    let mut entry = Entry {
        seq,
        at,
        record,
        prev,
        hash: String::new(),
    };
    entry.hash = hash(&entry, key);
    entry
}

/// Walk a whole chain from its first entry. On a break, give the `seq` of
/// the first entry that does not follow from the one before it.
pub fn verify(chain: &[Entry], key: &[u8]) -> Result<(), u64> {
    let mut last: Option<&Entry> = None;
    for entry in chain {
        if !follows(entry, last, key) {
            return Err(entry.seq);
        }
        last = Some(entry);
    }
    Ok(())
}

/// The entry is the one `append` would have written after `last`.
fn follows(entry: &Entry, last: Option<&Entry>, key: &[u8]) -> bool {
    let (seq, prev) = last.map_or((1, GENESIS), |e| (e.seq + 1, e.hash.as_str()));
    let linked = entry.seq == seq && entry.prev == prev;
    linked && entry.hash == hash(entry, key)
}

/// Each field goes in with its length first, so no two entries hash the same
/// bytes.
fn hash(e: &Entry, key: &[u8]) -> String {
    let mut message = Vec::new();
    message.extend(e.seq.to_le_bytes());
    message.extend(e.at.to_le_bytes());
    for field in [&e.record.kind, &e.record.subject, &e.record.detail, &e.prev] {
        message.extend((field.len() as u64).to_le_bytes());
        message.extend(field.as_bytes());
    }
    hmac(key, &message)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// HMAC-SHA256 (RFC 2104), so the core needs no crate beyond `sha2`.
fn hmac(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"a key kept outside the record";

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
                KEY,
            );
            chain.push(next);
        }
        chain
    }

    #[test]
    fn hmac_matches_rfc_4231_case_2() {
        let mac: String = hmac(b"Jefe", b"what do ya want for nothing?")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(
            mac,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn a_chain_as_written_verifies() {
        let chain = chain();
        assert_eq!(chain[0].prev, GENESIS);
        assert_eq!(chain[2].seq, 3);
        assert_eq!(verify(&chain, KEY), Ok(()));
        assert_eq!(verify(&[], KEY), Ok(()));
    }

    #[test]
    fn one_changed_byte_breaks_the_chain() {
        let mut chain = chain();
        chain[1].record.detail = "continue sandbox::fs::write".into();
        assert_eq!(verify(&chain, KEY), Err(2));
    }

    #[test]
    fn a_rehashed_entry_still_breaks_the_next_one() {
        let mut chain = chain();
        chain[1].record.detail = "continue sandbox::fs::write".into();
        chain[1].hash = hash(&chain[1], KEY);
        assert_eq!(verify(&chain, KEY), Err(3));
    }

    #[test]
    fn a_chain_rewritten_without_the_key_does_not_verify() {
        let mut forged: Vec<Entry> = Vec::new();
        for e in chain() {
            let mut r = e.record.clone();
            if e.seq == 2 {
                r.detail = "continue sandbox::fs::write".into();
            }
            let next = append(forged.last(), e.at, r, b"a guessed key");
            forged.push(next);
        }
        assert_eq!(verify(&forged, KEY), Err(1));
    }

    #[test]
    fn a_dropped_or_reordered_entry_breaks_the_chain() {
        let mut chain = chain();
        chain.remove(1);
        assert_eq!(verify(&chain, KEY), Err(3));
        let mut chain = self::tests::chain();
        chain.swap(0, 1);
        assert_eq!(verify(&chain, KEY), Err(2));
    }

    #[test]
    fn field_boundaries_change_the_hash() {
        let a = append(None, 1, record("gate", "j1x", "y"), KEY);
        let b = append(None, 1, record("gate", "j1", "xy"), KEY);
        assert_ne!(a.hash, b.hash);
    }
}
