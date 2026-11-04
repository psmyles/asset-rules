//! Resolution: which type, profile, stage and rules file apply to a file (plan item 3).
//!
//! Most cases run against `tests/fixtures/project`, a small rules repo.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use asset_rules::{Context, Resolution, Resolved, Rules, StageFrom, resolve};
use common::{sketch, temp_dir, write_rules};
use serde_json::json;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}

fn fixture() -> Rules {
    Rules::load(&fixture_dir().join("rules.json")).expect("load the fixture")
}

/// A context on `branch` (`None`: detached HEAD), in a repository whose root no relative path
/// needs.
fn on(branch: Option<&str>) -> Context {
    Context {
        repo_root: PathBuf::from("/repo"),
        branch: branch.map(str::to_owned),
    }
}

fn resolve_on(branch: Option<&str>, path: &str) -> Resolution {
    resolve(&fixture(), &on(branch), Path::new(path))
}

#[track_caller]
fn resolved(resolution: Resolution) -> Resolved {
    match resolution {
        Resolution::Resolved(resolved) => resolved,
        other => panic!("expected a profile to match, got {other:?}"),
    }
}

/// Asserts that `path`, committed to `branch`, gets `profile` at `stage` (from `from`) with the
/// fixture's rules file `file`.
#[track_caller]
fn assert_resolves(
    branch: Option<&str>,
    path: &str,
    profile: &str,
    stage: &str,
    from: StageFrom,
    file: &str,
) {
    let got = resolved(resolve_on(branch, path));
    let got_file = got
        .rules_file
        .as_deref()
        .map(|f| f.strip_prefix(fixture_dir()).unwrap());
    assert_eq!(
        (
            got.profile_id.as_str(),
            got.stage.as_str(),
            got.stage_from,
            got_file
        ),
        (profile, stage, from, Some(Path::new(file))),
        "{path} on {branch:?}"
    );
}

#[track_caller]
fn assert_profile(path: &str, profile: &str) {
    assert_eq!(
        resolved(resolve_on(None, path)).profile_id,
        profile,
        "{path}"
    );
}

#[track_caller]
fn assert_unmatched(path: &str, kind: &str) {
    assert_eq!(
        resolve_on(None, path),
        Resolution::Unmatched {
            kind: kind.to_owned()
        },
        "{path}"
    );
}

#[test]
fn the_fixture_has_no_problems() {
    assert_eq!(fixture().check(), []);
}

// --- Type ------------------------------------------------------------------------------------

#[test]
fn files_with_no_type_are_ignored() {
    for path in [
        "Assets/Scenes/Main.unity",
        "Assets/Art/T_Crate_D.png.meta",
        "Assets/Art/README",
        "Assets/Art/.png",
    ] {
        assert_eq!(resolve_on(None, path), Resolution::Ignored, "{path}");
    }
}

#[test]
fn extensions_are_compared_case_insensitively() {
    let got = resolved(resolve_on(None, "Assets/Art/T_Crate_D.PNG"));
    assert_eq!(
        (got.kind.as_str(), got.profile_id.as_str()),
        ("texture", "texture")
    );
    assert_profile("Assets/Art/Props/SM_Crate.Fbx", "prop");
}

// --- Profile ---------------------------------------------------------------------------------

#[test]
fn the_first_matching_profile_wins() {
    // Both `normal-map` (T_*_N) and `texture` (T_*) match; `normal-map` comes first.
    let normal = resolved(resolve_on(None, "Assets/Art/T_Crate_N.png"));
    assert_eq!(
        (
            normal.kind.as_str(),
            normal.profile_id.as_str(),
            normal.profile_label.as_str()
        ),
        ("texture", "normal-map", "Normal map")
    );
    assert_profile("Assets/Art/T_Crate_D.png", "texture");
    assert_profile("Assets/Characters/SK_Hero.fbx", "character-model");
    assert_profile("Assets/Characters/CH_Hero.fbx", "character-model");
}

#[test]
fn paths_narrow_a_profile() {
    assert_profile("Assets/Art/Environment/SM_Rock.fbx", "environment-prop");
    assert_profile(
        "Assets/Art/Environment/Rocks/Big/SM_Rock.fbx",
        "environment-prop",
    );
    assert_profile("Assets/Art/Props/SM_Crate.fbx", "prop");
    assert_profile("Assets/Art/EnvironmentOld/SM_Rock.fbx", "prop");
    // Paths are case-sensitive too.
    assert_profile("Assets/art/environment/SM_Rock.fbx", "prop");
}

#[test]
fn no_matching_profile_is_unmatched_with_its_type() {
    assert_unmatched("Assets/Art/ComfyUI_00042_.png", "texture");
    assert_unmatched("Assets/Art/Crate.fbx", "model");
    // Names are case-sensitive.
    assert_unmatched("Assets/Art/t_crate_d.png", "texture");
    assert_unmatched("Assets/Art/sm_rock.fbx", "model");
}

// --- Stage -----------------------------------------------------------------------------------

#[test]
fn the_project_stage_applies_when_nothing_else_does() {
    let env = "Assets/Art/Environment/SM_Rock.fbx";
    let proto = "models/environment-proto.json";
    assert_resolves(
        Some("feature/rocks"),
        env,
        "environment-prop",
        "prototype",
        StageFrom::Project,
        proto,
    );
}

#[test]
fn a_profile_stage_beats_the_project_stage() {
    let hero = "Assets/Characters/SK_Hero.fbx";
    let file = "models/character.json";
    assert_resolves(
        Some("feature/hero"),
        hero,
        "character-model",
        "production",
        StageFrom::Profile,
        file,
    );
}

#[test]
fn a_branch_stage_beats_the_profile_and_project_stages() {
    let hero = "Assets/Characters/SK_Hero.fbx";
    let env = "Assets/Art/Environment/SM_Rock.fbx";
    // Over the profile's `production`:
    let file = "models/character-proto.json";
    assert_resolves(
        Some("proto/hero"),
        hero,
        "character-model",
        "prototype",
        StageFrom::Branch,
        file,
    );
    // Over the project's `prototype`:
    let file = "models/environment.json";
    assert_resolves(
        Some("main"),
        env,
        "environment-prop",
        "production",
        StageFrom::Branch,
        file,
    );
    assert_resolves(
        Some("release/1.0"),
        env,
        "environment-prop",
        "production",
        StageFrom::Branch,
        file,
    );
}

#[test]
fn branch_patterns_follow_the_glob_rules() {
    let env = "Assets/Art/Environment/SM_Rock.fbx";
    // `proto/**` reaches nested branches; `release/*` doesn't.
    let file = "models/environment-proto.json";
    assert_resolves(
        Some("proto/rocks/v2"),
        env,
        "environment-prop",
        "prototype",
        StageFrom::Branch,
        file,
    );
    assert_resolves(
        Some("release/1.0/hotfix"),
        env,
        "environment-prop",
        "prototype",
        StageFrom::Project,
        file,
    );
    // Case-sensitive.
    assert_resolves(
        Some("Main"),
        env,
        "environment-prop",
        "prototype",
        StageFrom::Project,
        file,
    );
}

#[test]
fn a_detached_head_matches_no_branch_rule() {
    let hero = "Assets/Characters/SK_Hero.fbx";
    let env = "Assets/Art/Environment/SM_Rock.fbx";
    assert_resolves(
        None,
        hero,
        "character-model",
        "production",
        StageFrom::Profile,
        "models/character.json",
    );
    assert_resolves(
        None,
        env,
        "environment-prop",
        "prototype",
        StageFrom::Project,
        "models/environment-proto.json",
    );
}

#[test]
fn the_first_matching_branch_rule_wins() {
    let mut rules = sketch();
    rules["branches"] = json!([
        { "pattern": "proto/**", "stage": "prototype" },
        { "pattern": "**", "stage": "production" }
    ]);
    let dir = temp_dir();
    let rules = Rules::load(&write_rules(&dir, &rules)).unwrap();
    let stage = |branch: &str| {
        let ctx = Context {
            repo_root: PathBuf::from("/repo"),
            branch: Some(branch.to_owned()),
        };
        let got = resolved(resolve(&rules, &ctx, Path::new("Assets/T_Crate_N.png")));
        (got.stage, got.stage_from)
    };
    assert_eq!(
        stage("proto/crates"),
        ("prototype".to_owned(), StageFrom::Branch)
    );
    assert_eq!(
        stage("feature/crates"),
        ("production".to_owned(), StageFrom::Branch)
    );
}

// --- Rules file ------------------------------------------------------------------------------

#[test]
fn a_stage_without_its_own_file_falls_back_to_the_catch_all() {
    let texture = "Assets/Art/T_Crate_D.png";
    // `texture` has its own `production` file, and `"*"` for the rest.
    assert_resolves(
        Some("main"),
        texture,
        "texture",
        "production",
        StageFrom::Branch,
        "textures/texture.json",
    );
    assert_resolves(
        None,
        texture,
        "texture",
        "prototype",
        StageFrom::Project,
        "textures/texture-proto.json",
    );
    // `character-model` has its own `prototype` file, and `"*"` for the rest.
    let hero = "Assets/Characters/SK_Hero.fbx";
    assert_resolves(
        Some("main"),
        hero,
        "character-model",
        "production",
        StageFrom::Branch,
        "models/character.json",
    );
}

#[test]
fn a_stage_with_no_file_at_all_has_no_rules_file() {
    let mut rules = sketch();
    rules["profiles"][1]["rules"] = json!({ "production": "models/prop.json" });
    let dir = temp_dir();
    let rules = Rules::load(&write_rules(&dir, &rules)).unwrap();
    let got = resolved(resolve(
        &rules,
        &on(None),
        Path::new("Assets/Art/Environment/SM_Rock.fbx"),
    ));
    assert_eq!(
        (got.profile_id.as_str(), got.stage.as_str()),
        ("prop-model", "prototype")
    );
    assert_eq!(got.rules_file, None);
}

// --- Paths -----------------------------------------------------------------------------------

#[test]
fn relative_paths_are_normalised() {
    assert_profile("./Assets/Art/Environment/SM_Rock.fbx", "environment-prop");
    assert_profile(
        "Assets/Art/Props/../Environment/SM_Rock.fbx",
        "environment-prop",
    );
}

#[test]
fn a_path_outside_the_repository_matches_only_profiles_without_paths() {
    assert_profile("../Assets/Art/Environment/SM_Rock.fbx", "prop");
    let dir = temp_dir();
    let ctx = Context {
        repo_root: dir.path().join("repo"),
        branch: None,
    };
    let outside = dir
        .path()
        .join("elsewhere/Assets/Art/Environment/SM_Rock.fbx");
    assert_eq!(
        resolved(resolve(&fixture(), &ctx, &outside)).profile_id,
        "prop"
    );
}

#[test]
fn absolute_paths_are_made_relative_to_the_repository() {
    let dir = temp_dir();
    let ctx = Context {
        repo_root: dir.path().to_owned(),
        branch: None,
    };
    let inside = dir.path().join("Assets/Art/Environment/SM_Rock.fbx");
    assert_eq!(
        resolved(resolve(&fixture(), &ctx, &inside)).profile_id,
        "environment-prop"
    );
}

#[test]
fn an_absolute_path_through_a_differently_spelled_root_still_matches() {
    // On macOS the temporary folder is under `/var`, a symlink to `/private/var`, which is what
    // the canonical root (and git) say. The file must exist to be resolved through the links.
    let dir = temp_dir();
    let file = dir.write("Assets/Art/Environment/SM_Rock.fbx", "");
    let ctx = Context {
        repo_root: fs::canonicalize(dir.path()).unwrap(),
        branch: None,
    };
    assert_eq!(
        resolved(resolve(&fixture(), &ctx, &file)).profile_id,
        "environment-prop"
    );
}

#[cfg(windows)]
#[test]
fn windows_separators_are_matched_as_slashes() {
    assert_profile("Assets\\Art\\Environment\\SM_Rock.fbx", "environment-prop");
}
