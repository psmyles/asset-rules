//! The results cache (plan item 6).

mod common;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use asset_rules::cache::{Cache, CacheKey, MAX_UNUSED, digest_file};
use asset_rules::git::{Repo, StagedFile};
use asset_rules::results::{FileResult, Finding, Severity, Status, ToolInfo};
use asset_rules::{Resolution, Resolved, Rules, StageFrom, resolve};
use common::repo::TestRepo;
use common::temp_dir;

fn fixture() -> Rules {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project/rules.json");
    Rules::load(&path).unwrap()
}

fn tool() -> ToolInfo {
    ToolInfo {
        name: "fire-cli".to_owned(),
        version: "0.1.0".to_owned(),
    }
}

fn too_large() -> Finding {
    Finding::new(
        "texture.max_size",
        Severity::Error,
        "Too large",
        "4096×4096. Textures may be at most 2048×2048.",
        "It takes 16 MB of memory.",
        "Scale it down.",
    )
}

/// What a CLI does for each staged file: use the cached entry if there is one, else check the
/// file and store the entry for next time. Returns the entry, and whether it was cached.
fn check(cache: &Cache, rules: &Rules, repo: &Repo, file: &StagedFile) -> (FileResult, bool) {
    let ctx = repo.context().unwrap();
    let Resolution::Resolved(resolved) = resolve(rules, &ctx, Path::new(&file.path)) else {
        panic!("{} resolves", file.path);
    };
    let digest = digest_file(resolved.rules_file.as_deref().unwrap()).unwrap();
    let key = CacheKey::new(&file.path, &file.blob, &resolved, &digest, &tool());
    if let Some(entry) = cache.get(&key) {
        return (entry, true);
    }
    let mut entry = FileResult::resolved(&file.path, &resolved);
    entry.findings.push(too_large());
    cache.put(&key, &entry).unwrap();
    (entry, false)
}

/// A resolution to build keys from by hand.
fn resolved() -> Resolved {
    Resolved {
        kind: "texture".to_owned(),
        profile_id: "texture".to_owned(),
        profile_label: "Texture".to_owned(),
        stage: "production".to_owned(),
        stage_from: StageFrom::Branch,
        rules_file: Some(PathBuf::from("textures/texture.json")),
    }
}

const PATH: &str = "Assets/Art/T_Crate_D.png";
const BLOB: &str = "8ab686eafeb1f44702738c8b0f24f2567c36da6d";
const DIGEST: &str = "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

fn key() -> CacheKey {
    CacheKey::new(PATH, BLOB, &resolved(), DIGEST, &tool())
}

fn entry() -> FileResult {
    let mut entry = FileResult::resolved(PATH, &resolved());
    entry.findings.push(too_large());
    entry
}

/// Marks the entry for `key` as last used `days` days ago.
fn age(cache: &Cache, key: &CacheKey, days: u64) {
    let then = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
    File::options()
        .write(true)
        .open(cache.path(key))
        .unwrap()
        .set_modified(then)
        .unwrap();
}

// --- The plan's done criterion ---------------------------------------------------------------

#[test]
fn a_second_check_of_an_unchanged_file_reads_from_the_cache() {
    let test = TestRepo::new();
    test.write(PATH, "pixels");
    test.git(&["add", "."]);
    let repo = test.repo();
    let cache = Cache::open(&repo);
    assert_eq!(cache.dir(), repo.common_dir().join("asset-check/cache"));
    let rules = fixture();

    let staged = repo.staged().unwrap();
    let (first, cached) = check(&cache, &rules, &repo, &staged[0]);
    assert!(!cached, "nothing is cached yet");
    let (second, cached) = check(&cache, &rules, &repo, &staged[0]);
    assert!(cached, "the second check reads from the cache");
    assert_eq!(second, first);

    // Staging new content gives a new blob, which isn't cached.
    test.write(PATH, "different pixels");
    test.git(&["add", "."]);
    let staged = repo.staged().unwrap();
    let (_, cached) = check(&cache, &rules, &repo, &staged[0]);
    assert!(!cached, "a changed file is checked again");
}

#[test]
fn changing_any_one_key_part_misses() {
    let dir = temp_dir();
    let cache = Cache::at(dir.path().join("cache"));
    cache.put(&key(), &entry()).unwrap();
    assert_eq!(cache.get(&key()), Some(entry()));

    let other_tool = |name: &str, version: &str| ToolInfo {
        name: name.to_owned(),
        version: version.to_owned(),
    };
    let with = |change: fn(&mut Resolved)| {
        let mut resolved = resolved();
        change(&mut resolved);
        CacheKey::new(PATH, BLOB, &resolved, DIGEST, &tool())
    };
    let other_digest = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
    let misses = [
        // The parts AC15 names:
        (
            "blob",
            CacheKey::new(PATH, &BLOB.replace('8', "9"), &resolved(), DIGEST, &tool()),
        ),
        (
            "rules file",
            CacheKey::new(PATH, BLOB, &resolved(), other_digest, &tool()),
        ),
        (
            "tool name",
            CacheKey::new(
                PATH,
                BLOB,
                &resolved(),
                DIGEST,
                &other_tool("review3d-cli", "0.1.0"),
            ),
        ),
        (
            "tool version",
            CacheKey::new(
                PATH,
                BLOB,
                &resolved(),
                DIGEST,
                &other_tool("fire-cli", "0.1.1"),
            ),
        ),
        ("stage", with(|r| r.stage = "prototype".to_owned())),
        // And what the entry records:
        (
            "path",
            CacheKey::new(
                "Assets/Art/T_Copy_D.png",
                BLOB,
                &resolved(),
                DIGEST,
                &tool(),
            ),
        ),
        ("type", with(|r| r.kind = "image".to_owned())),
        (
            "profile",
            with(|r| r.profile_id = "prop-texture".to_owned()),
        ),
        (
            "label",
            with(|r| r.profile_label = "Prop texture".to_owned()),
        ),
        ("stage source", with(|r| r.stage_from = StageFrom::Project)),
    ];
    for (part, key) in misses {
        assert_eq!(cache.get(&key), None, "a different {part} misses");
    }
}

// --- What is stored --------------------------------------------------------------------------

#[test]
fn only_checked_entries_are_stored() {
    let dir = temp_dir();
    let cache = Cache::at(dir.path().join("cache"));
    for status in [Status::Unreadable, Status::Skipped] {
        let entry = FileResult {
            status,
            problem: Some("The Git LFS object isn't on this machine.".to_owned()),
            ..entry()
        };
        cache.put(&key(), &entry).unwrap();
        assert_eq!(cache.get(&key()), None, "{status:?}");
    }
    assert!(!cache.dir().exists(), "nothing was written");
}

#[test]
fn an_entry_that_cant_be_read_is_a_miss_and_is_removed() {
    let dir = temp_dir();
    let cache = Cache::at(dir.path().join("cache"));
    cache.put(&key(), &entry()).unwrap();
    fs::write(cache.path(&key()), "{ half an entr").unwrap();
    assert_eq!(cache.get(&key()), None);
    assert!(!cache.path(&key()).exists());
}

#[test]
fn the_rules_file_digest_is_its_sha256() {
    let rules = fixture();
    assert_eq!(digest_file(rules.path()).unwrap(), rules.digest());
}

// --- Pruning ---------------------------------------------------------------------------------

#[test]
fn entries_unused_for_30_days_are_pruned() {
    assert_eq!(MAX_UNUSED, Duration::from_secs(30 * 24 * 60 * 60));
    let dir = temp_dir();
    let cache = Cache::at(dir.path().join("cache"));
    let recent = key();
    let old = CacheKey::new("Assets/Art/T_Old_D.png", BLOB, &resolved(), DIGEST, &tool());
    cache.put(&recent, &entry()).unwrap();
    cache.put(&old, &entry()).unwrap();
    age(&cache, &recent, 29);
    age(&cache, &old, 31);

    assert_eq!(cache.prune().unwrap(), 1);
    assert!(cache.get(&recent).is_some());
    assert!(cache.get(&old).is_none());
    // The two keys are in different buckets, so the old one's is left empty, and removed.
    let old_bucket = cache.path(&old).parent().unwrap().to_owned();
    assert_ne!(Some(old_bucket.as_path()), cache.path(&recent).parent());
    assert!(!old_bucket.exists(), "an emptied bucket is removed");
    assert!(cache.path(&recent).exists());
}

#[test]
fn using_an_entry_keeps_it() {
    let dir = temp_dir();
    let cache = Cache::at(dir.path().join("cache"));
    cache.put(&key(), &entry()).unwrap();
    age(&cache, &key(), 31);
    assert!(cache.get(&key()).is_some());
    assert_eq!(cache.prune().unwrap(), 0);
    assert!(cache.get(&key()).is_some());
}

#[test]
fn pruning_a_cache_that_doesnt_exist_yet() {
    let dir = temp_dir();
    assert_eq!(Cache::at(dir.path().join("cache")).prune().unwrap(), 0);
}
