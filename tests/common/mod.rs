//! Helpers shared by the integration tests.

#![allow(dead_code)] // Each test crate uses only some of these.

pub mod repo;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

/// A folder under the system temporary directory, removed when dropped.
pub struct TempDir(PathBuf);

/// A new, empty [`TempDir`].
pub fn temp_dir() -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("asset-rules-test-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create a temporary folder");
    TempDir(path)
}

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes `contents` to `relative` inside the folder, creating folders on the way.
    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create a folder");
        }
        fs::write(&path, contents).expect("write a file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The `rules.json` sketched in `asset-check-plan.md` §3. It has no problems.
pub fn sketch() -> Value {
    json!({
        "version": 1,
        "mode": "advise",
        "stages": ["prototype", "production"],
        "stage": "prototype",
        "branches": [
            { "pattern": "proto/*", "stage": "prototype" },
            { "pattern": "main", "stage": "production" }
        ],
        "requires": { "fire-cli": "0.1.0", "review3d-cli": "0.9.0" },
        "types": {
            "texture": {
                "tool": "fire-cli",
                "extensions": ["png", "tga", "psd", "psb", "tif", "tiff", "exr", "jpg", "jpeg",
                               "webp", "avif", "heic", "heif"]
            },
            "model": { "tool": "review3d-cli", "extensions": ["fbx"] }
        },
        "profiles": [
            {
                "id": "character-model", "type": "model", "label": "Character model",
                "names": ["SK_*", "CH_*"], "stage": "production",
                "rules": { "*": "models/character.json", "prototype": "models/character-proto.json" }
            },
            {
                "id": "prop-model", "type": "model", "label": "Environment prop",
                "names": ["SM_*"], "paths": ["Assets/Art/Environment/**"],
                "rules": { "prototype": "models/prop-proto.json", "production": "models/prop.json" }
            },
            {
                "id": "normal-map", "type": "texture", "label": "Normal map",
                "names": ["T_*_N"],
                "rules": { "*": "textures/normal.json" }
            }
        ]
    })
}

/// Writes `rules` as `rules.json` into `dir`, with an empty rules file for every file its
/// profiles name, and returns the path of `rules.json`.
pub fn write_rules(dir: &TempDir, rules: &Value) -> PathBuf {
    let profiles = rules["profiles"].as_array().into_iter().flatten();
    for file in profiles.flat_map(|profile| profile["rules"].as_object().into_iter().flatten()) {
        if let Some(file) = file.1.as_str() {
            dir.write(file, "{}");
        }
    }
    dir.write(
        "rules.json",
        &serde_json::to_string_pretty(rules).expect("serialize"),
    )
}
