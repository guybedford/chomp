// Chomp Task Runner
// Copyright (C) 2022  Guy Bedford

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::http_client::hash;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

// In-memory content-hash cache for watched files, shared between the task runner's watch
// invalidation and the dev server. It is only populated while watching or serving and is
// never persisted: its sole job is to let both systems agree, from a single hash, whether
// a watched file's bytes actually changed. A watch event that leaves the contents
// unchanged (a read that bumped mtime, or a rewrite with identical bytes) then triggers
// neither a rebuild nor an HMR reload.
//
// Files are keyed by their absolute, forward-slashed path so the runner (keying off notify
// events) and the server (keying off the served path) resolve to the same entry. A key
// mismatch only ever costs a cache miss, which is treated as "changed" — never a skipped
// change — so divergent path normalisation degrades safely to the previous behaviour.
#[derive(Clone, Default)]
pub struct HashCache {
    inner: Arc<RwLock<HashMap<String, String>>>,
}

impl HashCache {
    pub fn new() -> HashCache {
        HashCache::default()
    }

    // Hash the file's current contents and reconcile against the cached value, updating the
    // cache to the new hash. Returns `(hash, changed)`, where `changed` is true when the
    // file was not previously cached or its contents differ. Returns `None` when the file
    // cannot be read (e.g. it was removed) — callers treat that as a change.
    pub fn revalidate(&self, path: &Path) -> Option<(String, bool)> {
        let source = std::fs::read(path).ok()?;
        let hash = hash(&source);
        let key = path.to_string_lossy().replace('\\', "/");
        let mut cache = self.inner.write().unwrap();
        let changed = match cache.get(&key) {
            Some(existing) => existing != &hash,
            None => true,
        };
        if changed {
            cache.insert(key, hash.clone());
        }
        Some((hash, changed))
    }
}

#[cfg(test)]
mod tests {
    use super::HashCache;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("chomp-hashcache-test-{}", name));
        p
    }

    #[test]
    fn detects_content_changes() {
        let path = temp_path("content");
        std::fs::write(&path, b"one").unwrap();
        let cache = HashCache::new();

        // First sighting of a file is always reported as a change.
        let (h1, changed) = cache.revalidate(&path).unwrap();
        assert!(changed);
        // Re-hashing identical bytes is not a change, and yields the same hash.
        let (h2, changed) = cache.revalidate(&path).unwrap();
        assert!(!changed);
        assert_eq!(h1, h2);
        // Different bytes are a change with a new hash.
        std::fs::write(&path, b"two").unwrap();
        let (h3, changed) = cache.revalidate(&path).unwrap();
        assert!(changed);
        assert_ne!(h2, h3);

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_is_treated_as_changed() {
        // An unreadable/removed file yields None so callers fall through to invalidation.
        let cache = HashCache::new();
        assert!(cache.revalidate(&temp_path("does-not-exist")).is_none());
    }
}
