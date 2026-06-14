//! Persistent vector memory backed by VecLite (RAG store).
//!
//! Vectors carry their source text in `metadata.text`, so recall returns the
//! original content, not just ids.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use veclite_db::VecLite;

use crate::error::AiError;

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct Memory {
    db: VecLite,
}

pub struct Recalled {
    pub text: String,
    pub source: String,
}

impl Memory {
    /// Open (or create) the memory store at `~/.config/bruno/memory.vlt`.
    pub fn open() -> Result<Self, AiError> {
        let path = store_path().ok_or_else(|| AiError::Config("no home dir".into()))?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let db = VecLite::open(&path)
            .map_err(|e| AiError::Config(format!("veclite open failed: {e:?}")))?;
        Ok(Self { db })
    }

    /// Store `text` (with its embedding) under a fresh id. Returns the id.
    pub fn remember(
        &mut self,
        vector: Vec<f32>,
        text: &str,
        source: &str,
    ) -> Result<String, AiError> {
        let id = new_id();
        let ts = now_secs();
        let meta = json!({ "text": text, "source": source, "ts": ts });
        self.db
            .insert(&id, vector, Some(meta))
            .map_err(|e| AiError::Config(format!("veclite insert failed: {e:?}")))?;
        Ok(id)
    }

    /// Semantic search; returns the top-k stored texts by similarity.
    pub fn recall(&self, vector: &[f32], k: usize) -> Result<Vec<Recalled>, AiError> {
        let results = self
            .db
            .search(vector, k)
            .map_err(|e| AiError::Config(format!("veclite search failed: {e:?}")))?;
        Ok(results
            .into_iter()
            .filter_map(|r| {
                let meta = r.metadata?;
                let text = meta.get("text")?.as_str()?.to_string();
                let source = meta
                    .get("source")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string();
                let _ = r.score;
                Some(Recalled { text, source })
            })
            .collect())
    }
}

fn store_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("bruno").join("memory.vlt"))
}

fn new_id() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mem_{}_{}", now_secs(), n)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
