//! Embedding engine and cosine-similarity search for Glitch vault notes.
//!
//! All public functions are **synchronous and blocking** — call them from a
//! `tokio::task::spawn_blocking` closure when invoking from async Dioxus code.

use anyhow::{anyhow, Context};
use camino::Utf8Path;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

// ── Constants ─────────────────────────────────────────────────────────────────

/// BGE-small-en-v1.5: 384-dim, ~130 MB on first download.
const MODEL: EmbeddingModel = EmbeddingModel::BGESmallENV15;

/// Vault-relative path to the embedding store.
const STORE_FILE: &str = ".glitch/embeddings.json";

/// Truncate note content to this many characters before embedding.
/// ~1 500 chars ≈ 300 BPE tokens, well inside BGE's 512-token limit.
const MAX_CHARS: usize = 1_500;

// ── Global engine (lazy-init) ─────────────────────────────────────────────────

static ENGINE: OnceLock<Mutex<TextEmbedding>> = OnceLock::new();

/// Return (or initialise) the global `TextEmbedding` model.
///
/// First call downloads the model to `cache_dir` if it is not already there.
fn get_engine(cache_dir: &Path) -> anyhow::Result<std::sync::MutexGuard<'static, TextEmbedding>> {
    if ENGINE.get().is_none() {
        let opts = InitOptions::new(MODEL)
            .with_cache_dir(cache_dir.to_path_buf())
            .with_show_download_progress(true);
        let model = TextEmbedding::try_new(opts)
            .context("failed to initialise fastembed model (BGE-small-en-v1.5)")?;
        // Ignore the error if another thread already stored the engine.
        let _ = ENGINE.set(Mutex::new(model));
    }
    ENGINE
        .get()
        .unwrap()
        .lock()
        .map_err(|_| anyhow!("embedding engine mutex was poisoned"))
}

// ── On-disk store ─────────────────────────────────────────────────────────────

/// Flat map: vault-relative path → f32 embedding vector (384 dims).
type Store = HashMap<String, Vec<f32>>;

fn store_path(vault_root: &Utf8Path) -> std::path::PathBuf {
    vault_root.join(STORE_FILE).into_std_path_buf()
}

fn load_store(vault_root: &Utf8Path) -> Store {
    let p = store_path(vault_root);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_store(vault_root: &Utf8Path, store: &Store) -> anyhow::Result<()> {
    let p = store_path(vault_root);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string(store)?)?;
    Ok(())
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A note returned by [`find_similar`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarNote {
    /// Vault-relative path, e.g. `"notes/foo.md"`.
    pub rel_path: String,
    /// Cosine similarity in [0, 1].
    pub score: f32,
}

// ── Public functions ──────────────────────────────────────────────────────────

/// Embed `content` (first [`MAX_CHARS`] chars) and persist the embedding in
/// `<vault_root>/.glitch/embeddings.json` under the key `rel_path`.
///
/// `cache_dir` — directory where fastembed stores the downloaded model files.
/// Recommended: `%LOCALAPPDATA%\Glitch\models` on Windows.
///
/// **Blocking.** Run inside `tokio::task::spawn_blocking`.
pub fn embed_note(
    vault_root: &Utf8Path,
    rel_path: &str,
    content: &str,
    cache_dir: &Path,
) -> anyhow::Result<()> {
    let text: String = content.chars().take(MAX_CHARS).collect();
    let embedding = {
        let engine = get_engine(cache_dir)?;
        let mut batch = engine.embed(vec![text], None)?;
        batch.pop().ok_or_else(|| anyhow!("fastembed returned empty batch"))?
    };
    let mut store = load_store(vault_root);
    store.insert(rel_path.to_string(), embedding);
    save_store(vault_root, &store)?;
    Ok(())
}

/// Remove a note's embedding (e.g. on delete / rename).
pub fn remove_note(vault_root: &Utf8Path, rel_path: &str) -> anyhow::Result<()> {
    let mut store = load_store(vault_root);
    if store.remove(rel_path).is_some() {
        save_store(vault_root, &store)?;
    }
    Ok(())
}

/// Return the `top_k` vault notes most similar to `rel_path` (excluding itself),
/// sorted by cosine similarity descending.
///
/// Returns an empty `Vec` if `rel_path` has no stored embedding yet.
pub fn find_similar(
    vault_root: &Utf8Path,
    rel_path: &str,
    top_k: usize,
) -> anyhow::Result<Vec<SimilarNote>> {
    let store = load_store(vault_root);
    let Some(query) = store.get(rel_path) else {
        return Ok(vec![]);
    };
    let query = query.clone();

    let mut scored: Vec<SimilarNote> = store
        .iter()
        .filter(|(k, _)| k.as_str() != rel_path)
        .map(|(k, emb)| SimilarNote {
            rel_path: k.clone(),
            score: cosine(&query, emb),
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    Ok(scored)
}

/// True if `rel_path` already has a stored embedding.
pub fn is_indexed(vault_root: &Utf8Path, rel_path: &str) -> bool {
    load_store(vault_root).contains_key(rel_path)
}

/// Embed an arbitrary text query and return the `top_k` vault notes most similar to it.
///
/// Requires the model to be downloaded (`cache_dir`) and at least some notes to be indexed.
/// Returns an empty `Vec` if the store has no embeddings yet.
///
/// **Blocking.** Run inside `tokio::task::spawn_blocking`.
pub fn search_by_text(
    vault_root: &Utf8Path,
    query_text: &str,
    top_k: usize,
    cache_dir: &Path,
) -> anyhow::Result<Vec<SimilarNote>> {
    let text: String = query_text.chars().take(MAX_CHARS).collect();
    if text.is_empty() {
        return Ok(vec![]);
    }
    let query_emb = {
        let engine = get_engine(cache_dir)?;
        let mut batch = engine.embed(vec![text], None)?;
        batch.pop().ok_or_else(|| anyhow!("fastembed returned empty batch"))?
    };
    let store = load_store(vault_root);
    if store.is_empty() {
        return Ok(vec![]);
    }
    let mut scored: Vec<SimilarNote> = store
        .iter()
        .map(|(k, emb)| SimilarNote {
            rel_path: k.clone(),
            score: cosine(&query_emb, emb),
        })
        .collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    Ok(scored)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}
