//! Screenshots and recordings: after a turn, each image or recording
//! the turn left under /work/artifacts in its VM is copied to the host and
//! recorded on the job, once.

use std::path::PathBuf;

use bastion_core::media;
use serde_json::{Value, json};

use super::{Factory, SLOW_MS, now};
use crate::engine::DEFAULT_NAMESPACE;
use crate::store::{Artifact, Record};

/// The largest file kept, so a stray recording cannot fill the disk.
const LARGEST: u64 = 50 * 1024 * 1024;

/// The kept-kind files a `sandbox::fs::ls` answer lists, small enough to keep.
fn listed(answer: &Value) -> Vec<String> {
    let entries = answer["entries"].as_array().cloned().unwrap_or_default();
    entries
        .iter()
        .filter(|e| e["is_dir"] != true && e["size"].as_u64().unwrap_or(0) <= LARGEST)
        .filter_map(|e| e["name"].as_str().map(String::from))
        .filter(|n| media::kept(n))
        .collect()
}

impl Factory {
    /// Where a job's kept files are on the host.
    fn media_dir(&self, job: &str) -> PathBuf {
        self.scratch.join("media").join(job)
    }

    /// The host path of `name` among `job`'s kept files, if it is one.
    pub fn media_path(&self, job: &str, name: &str) -> Option<PathBuf> {
        let path = self.media_dir(job).join(name);
        (media::kept(name) && self.machine.exists(&path)).then_some(path)
    }

    /// Copy out what the turn kept under /work/artifacts that the job does
    /// not hold yet. A file that cannot be read is left for the next turn.
    pub(crate) async fn keep_media(&self, record: &Record) {
        let Some(vm) = &record.runtime.sandbox else {
            return;
        };
        let ask = json!({ "sandbox_id": vm, "path": media::DIR });
        let Ok(answer) = self
            .engine
            .call(DEFAULT_NAMESPACE, "sandbox::fs::ls", ask)
            .await
        else {
            return;
        };
        let id = &record.job.id;
        let held = self.store.artifacts(id).await.unwrap_or_default();
        let have: Vec<&str> = held.iter().filter_map(|a| media::named(&a.body)).collect();
        for name in listed(&answer)
            .into_iter()
            .filter(|n| !have.contains(&n.as_str()))
        {
            if let Err(e) = self.keep_one(record, vm, &name).await {
                eprintln!("job {id}: {name} was not kept: {e}");
            }
        }
    }

    async fn keep_one(&self, record: &Record, vm: &str, name: &str) -> Result<(), String> {
        let path = format!("{}/{name}", media::DIR);
        let ask = json!({ "sandbox_id": vm, "path": path });
        let read = self
            .engine
            .call_for(DEFAULT_NAMESPACE, "sandbox::fs::read", ask, SLOW_MS);
        let bytes = self
            .engine
            .read_channel(read.await?["content"].clone())
            .await?;
        let dir = self.media_dir(&record.job.id);
        self.machine.make_dir(&dir)?;
        self.machine.write_bytes(&dir.join(name), &bytes)?;
        let kept = Artifact {
            stage: record.job.stage,
            step: record.job.node.clone().unwrap_or_default(),
            body: media::artifact(name),
            created_at: now(),
        };
        self.store.add_artifact(&record.job.id, &kept).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_keeps_images_and_recordings_under_the_size_limit() {
        let answer = json!({ "entries": [
            { "name": "home.png", "is_dir": false, "size": 2048 },
            { "name": "flow.webm", "is_dir": false, "size": 4096 },
            { "name": "notes.txt", "is_dir": false, "size": 10 },
            { "name": "shots", "is_dir": true, "size": 0 },
            { "name": "huge.mp4", "is_dir": false, "size": LARGEST + 1 },
        ]});
        assert_eq!(listed(&answer), ["home.png", "flow.webm"]);
        assert!(listed(&json!({})).is_empty());
    }
}
