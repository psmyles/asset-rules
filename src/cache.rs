//! The results cache (AC15): a file already checked isn't checked again until something that
//! decides its results changes.
//!
//! The Fork command "Check staged files" fills it, so the hook at commit time is instant for the
//! files already checked. Every entry is one file, `<cache>/<key[0..2]>/<key>.json`, holding the
//! file's results entry; its modification time is when it was last used.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::Resolved;
use crate::digest::{sha256, sha256_hex};
use crate::file::write_replacing;
use crate::git::Repo;
use crate::results::{FileResult, Status, ToolInfo};

/// How long an entry is kept without being used: 30 days.
pub const MAX_UNUSED: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Part of every key, so a change to how entries are stored misses every old one.
const FORMAT: &str = "asset-check.cache 1";

/// The cached results entries of one repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cache {
    dir: PathBuf,
}

/// What an entry is stored under: a hash of everything that decides a file's results entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    /// The key for checking the staged `blob` at repository path `path`, which resolved to
    /// `resolved`, with `tool` and the rules file whose [`digest_file`] is `rules_digest`.
    ///
    /// Made of the blob id, the rules file's digest, the tool's name and version, and the stage
    /// (AC15); and also of the path and the rest of the resolution, which the entry records. A
    /// copy of a file, or a profile relabelled in `rules.json`, then never gets an entry that
    /// names another path or the old label.
    pub fn new(
        path: &str,
        blob: &str,
        resolved: &Resolved,
        rules_digest: &str,
        tool: &ToolInfo,
    ) -> CacheKey {
        let parts = [
            FORMAT,
            &tool.name,
            &tool.version,
            blob,
            rules_digest,
            &resolved.stage,
            path,
            &resolved.kind,
            &resolved.profile_id,
            &resolved.profile_label,
            resolved.stage_from.as_str(),
        ];
        // NUL never appears in any part, so no two lists of parts join to the same text.
        CacheKey(sha256_hex(parts.join("\0").as_bytes()))
    }

    /// 64 hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `sha256:<hex>` of the file at `path`: what [`CacheKey::new`] takes for the rules file.
pub fn digest_file(path: &Path) -> io::Result<String> {
    Ok(sha256(&fs::read(path)?))
}

impl Cache {
    /// The cache in `<common dir>/asset-check/cache/`, shared by every worktree.
    pub fn open(repo: &Repo) -> Cache {
        Cache::at(repo.common_dir().join("asset-check").join("cache"))
    }

    /// A cache in `dir`. Nothing is created until the first [`Cache::put`].
    pub fn at(dir: impl Into<PathBuf>) -> Cache {
        Cache { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the entry for `key` is stored.
    pub fn path(&self, key: &CacheKey) -> PathBuf {
        self.dir
            .join(&key.as_str()[..2])
            .join(format!("{}.json", key.as_str()))
    }

    /// The entry stored under `key`, if there is one, marking it used. An entry that can't be
    /// read is a miss, and is removed.
    pub fn get(&self, key: &CacheKey) -> Option<FileResult> {
        let path = self.path(key);
        let bytes = fs::read(&path).ok()?;
        let Ok(entry) = serde_json::from_slice(&bytes) else {
            let _ = fs::remove_file(&path);
            return None;
        };
        if let Ok(file) = File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(entry)
    }

    /// Stores `entry` under `key`. Only a [`Status::Checked`] entry is stored: whether a file can
    /// be read can change without its content changing (an LFS object fetched later, say).
    pub fn put(&self, key: &CacheKey, entry: &FileResult) -> io::Result<()> {
        if entry.status != Status::Checked {
            return Ok(());
        }
        let json = serde_json::to_vec(entry).map_err(io::Error::other)?;
        let path = self.path(key);
        if let Some(bucket) = path.parent() {
            fs::create_dir_all(bucket)?;
        }
        write_replacing(&path, &json)
    }

    /// Removes the entries not used for [`MAX_UNUSED`], returning how many.
    pub fn prune(&self) -> io::Result<usize> {
        self.prune_unused_for(MAX_UNUSED)
    }

    /// Removes the entries (and any temporary file a killed write left) not used for `age`,
    /// returning how many. Another process pruning at the same time is harmless.
    pub fn prune_unused_for(&self, age: Duration) -> io::Result<usize> {
        let Some(cutoff) = SystemTime::now().checked_sub(age) else {
            return Ok(0);
        };
        let buckets = match fs::read_dir(&self.dir) {
            Ok(buckets) => buckets,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let mut removed = 0;
        for bucket in buckets.flatten() {
            let bucket = bucket.path();
            let Ok(entries) = fs::read_dir(&bucket) else {
                continue;
            };
            for entry in entries.flatten() {
                let unused = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .is_ok_and(|modified| modified < cutoff);
                if unused && fs::remove_file(entry.path()).is_ok() {
                    removed += 1;
                }
            }
            // Only succeeds once the bucket is empty.
            let _ = fs::remove_dir(&bucket);
        }
        Ok(removed)
    }
}
