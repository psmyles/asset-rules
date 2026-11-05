//! `rules.json` version 2: `check` and `skip`, which say which files are looked at at all.

mod common;

use std::path::{Path, PathBuf};

use asset_rules::{Context, Level, ProblemKind, Resolution, Rules, SkippedBy, explain, resolve};
use common::{TempDir, sketch, temp_dir, write_rules};
use serde_json::{Value, json};

/// The sketch as version 2, checking only `Assets/`, and skipping editor folders and baked lighting.
fn v2() -> Value {
    let mut rules = sketch();
    rules["version"] = json!(2);
    rules["check"] = json!(["Assets/**"]);
    rules["skip"] = json!([
        { "label": "Editor-only", "paths": ["Assets/**/Editor/**"] },
        { "label": "Baked lighting", "names": ["Lightmap-*", "ReflectionProbe-*"] },
        { "names": ["T_*_N"], "paths": ["Assets/Vendor/**"] }
    ]);
    rules
}

fn load(rules: &Value) -> (TempDir, Rules) {
    let dir = temp_dir();
    let path = write_rules(&dir, rules);
    let rules = Rules::load(&path).expect("load");
    (dir, rules)
}

fn ctx() -> Context {
    Context {
        repo_root: PathBuf::from("/repo"),
        branch: Some("main".to_owned()),
    }
}

fn resolve_path(rules: &Rules, path: &str) -> Resolution {
    resolve(rules, &ctx(), Path::new(path))
}

fn kinds(rules: &Rules) -> Vec<(ProblemKind, Level)> {
    rules
        .check()
        .into_iter()
        .map(|problem| (problem.kind, problem.kind.level()))
        .collect()
}

#[test]
fn version_2_loads_and_has_no_problems() {
    let (_dir, rules) = load(&v2());
    assert_eq!(rules.version(), 2);
    assert_eq!(
        rules
            .check_paths()
            .unwrap()
            .iter()
            .map(|p| p.as_str())
            .collect::<Vec<_>>(),
        ["Assets/**"]
    );
    assert_eq!(rules.skip().len(), 3);
    assert_eq!(rules.skip()[1].label.as_deref(), Some("Baked lighting"));
    assert_eq!(rules.check(), []);
}

#[test]
fn version_1_still_loads_and_checks_every_file() {
    let (_dir, rules) = load(&sketch());
    assert_eq!(rules.version(), 1);
    assert!(rules.check_paths().is_none());
    assert!(rules.skip().is_empty());
    assert!(matches!(
        resolve_path(&rules, "SourceArt/Lightmap-0_comp_light.png"),
        Resolution::Unmatched { .. }
    ));
}

#[test]
fn check_and_skip_need_version_2() {
    let mut rules = v2();
    rules["version"] = json!(1);
    let (_dir, rules) = load(&rules);
    let problems = rules.check();
    let needs: Vec<&str> = problems
        .iter()
        .filter(|p| p.kind == ProblemKind::NeedsNewerVersion)
        .map(|p| p.at.as_str())
        .collect();
    assert_eq!(needs, ["check", "skip"], "{problems:?}");
    assert!(problems.iter().all(|p| p.kind.level() == Level::Error));
    assert!(
        problems[0].message.contains("\"version\": 2"),
        "{}",
        problems[0]
    );
}

#[test]
fn a_file_outside_check_is_skipped() {
    let (_dir, rules) = load(&v2());
    assert_eq!(
        resolve_path(&rules, "SourceArt/Props/SM_Crate.fbx"),
        Resolution::Skipped {
            kind: "model".to_owned(),
            by: SkippedBy::Check
        }
    );
    // Inside `check`, the same name resolves.
    assert!(matches!(
        resolve_path(&rules, "Assets/Art/Environment/SM_Crate.fbx"),
        Resolution::Resolved(_)
    ));
}

#[test]
fn a_file_of_no_type_is_ignored_before_check_is_asked() {
    let (_dir, rules) = load(&v2());
    assert_eq!(
        resolve_path(&rules, "SourceArt/notes.txt"),
        Resolution::Ignored
    );
}

#[test]
fn skip_entries_match_by_name_by_path_or_both() {
    let (_dir, rules) = load(&v2());
    let skipped = |path: &str| match resolve_path(&rules, path) {
        Resolution::Skipped { by, .. } => Some(by),
        _ => None,
    };
    // By path.
    assert_eq!(
        skipped("Assets/Tools/Editor/icon.png"),
        Some(SkippedBy::Skip(0))
    );
    // By name, anywhere under `check`.
    assert_eq!(
        skipped("Assets/Scenes/Bank/Lightmap-0_comp_light.png"),
        Some(SkippedBy::Skip(1))
    );
    assert_eq!(
        skipped("Assets/Scenes/ReflectionProbe-3.exr"),
        Some(SkippedBy::Skip(1))
    );
    // Both: the name alone isn't enough, nor the path alone.
    assert_eq!(
        skipped("Assets/Vendor/Rocks/T_Rock_N.png"),
        Some(SkippedBy::Skip(2))
    );
    assert_eq!(skipped("Assets/Art/T_Rock_N.png"), None);
    assert_eq!(skipped("Assets/Vendor/Rocks/T_Rock_D.png"), None);
    // The first entry that matches is the one reported.
    assert_eq!(
        skipped("Assets/Tools/Editor/Lightmap-1.png"),
        Some(SkippedBy::Skip(0))
    );
}

#[test]
fn outside_the_repository_check_skips_nothing_but_names_still_do() {
    let (_dir, rules) = load(&v2());
    let outside = |name: &str| resolve(&rules, &ctx(), Path::new(&format!("/elsewhere/{name}")));
    assert!(matches!(outside("T_Wall_N.png"), Resolution::Resolved(_)));
    assert_eq!(
        outside("Lightmap-0.png"),
        Resolution::Skipped {
            kind: "texture".to_owned(),
            by: SkippedBy::Skip(1)
        }
    );
}

#[test]
fn problems_in_check_and_skip_are_reported() {
    let mut rules = v2();
    rules["check"] = json!([]);
    rules["skip"] = json!([
        { "label": "Nothing" },
        { "names": [] },
        { "names": ["T_[*"], "paths": ["Assets\\Plugins\\**"] },
        { "paths": ["Assets/Plugins/**"], "reason": "third party" }
    ]);
    let (_dir, rules) = load(&rules);
    let problems = rules.check();
    let found: Vec<(ProblemKind, &str)> =
        problems.iter().map(|p| (p.kind, p.at.as_str())).collect();
    assert_eq!(
        found,
        [
            (ProblemKind::UnknownField, "skip[3]"),
            (ProblemKind::NeverMatches, "check"),
            (ProblemKind::NeverMatches, "skip[0] \"Nothing\""),
            (ProblemKind::NeverMatches, "skip[1]"),
            (ProblemKind::InvalidGlob, "skip[2]"),
            (ProblemKind::Backslash, "skip[2]"),
        ],
        "{problems:#?}"
    );
    assert_eq!(
        kinds(&rules)
            .iter()
            .filter(|(_, level)| *level == Level::Error)
            .count(),
        1
    );
}

#[test]
fn an_empty_skip_entry_skips_nothing() {
    let mut rules = v2();
    rules["skip"] = json!([{ "label": "Nothing" }]);
    let (_dir, rules) = load(&rules);
    assert!(matches!(
        resolve_path(&rules, "Assets/Art/Environment/SM_Crate.fbx"),
        Resolution::Resolved(_)
    ));
}

#[test]
fn explain_says_why_a_file_is_skipped() {
    let (_dir, rules) = load(&v2());
    let text = explain(
        &rules,
        &ctx(),
        Path::new("Assets/Scenes/Lightmap-0_comp_light.png"),
    );
    assert!(text.contains("Type: texture"), "{text}");
    assert!(
        text.contains("Skipped: skip[1] \"Baked lighting\" matches, so it isn't checked."),
        "{text}"
    );
    assert!(
        text.contains(
            "The name \"Lightmap-0_comp_light\" matches one of: Lightmap-*, ReflectionProbe-*."
        ),
        "{text}"
    );

    let text = explain(&rules, &ctx(), Path::new("SourceArt/SM_Crate.fbx"));
    assert!(
        text.contains("Skipped: not in \"check\", so it isn't checked."),
        "{text}"
    );
    assert!(
        text.contains("\"SourceArt/SM_Crate.fbx\" matches none of \"check\": Assets/**."),
        "{text}"
    );
}
