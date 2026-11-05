//! `rules explain <file>` (plan item 8).

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use asset_rules::{Context, Resolution, Rules, explain, resolve};
use common::{sketch, temp_dir, write_rules};
use serde_json::json;

fn fixture() -> Rules {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project/rules.json");
    Rules::load(&path).unwrap()
}

fn on(branch: Option<&str>) -> Context {
    Context {
        repo_root: PathBuf::from("/repo"),
        branch: branch.map(str::to_owned),
    }
}

fn explain_on(branch: Option<&str>, path: &str) -> String {
    explain(&fixture(), &on(branch), Path::new(path))
}

#[test]
fn a_file_resolved_by_its_path_and_branch() {
    assert_eq!(
        explain_on(Some("main"), "Assets/Art/Environment/SM_Rock.fbx"),
        r#"Assets/Art/Environment/SM_Rock.fbx

Type: model
    Files with the extension "fbx" are of type "model", which 3d-review-cli checks.

Profile: Environment prop (environment-prop)
    The model profiles are tried in file order, and the first that matches wins:
    - profiles[0] "character-model": no; the name "SM_Rock" matches none of SK_*, CH_*.
    - profiles[1] "environment-prop": yes; the name matches SM_*, and the path matches Assets/Art/Environment/**.

Stage: production, from the branch
    The branch "main" matches branches[1] (main). The branch outranks the profile and the project.

Rules file: models/environment.json
    It is the profile's file for "production".
"#
    );
}

#[test]
fn a_file_whose_path_rules_out_a_profile_falls_to_the_project_stage() {
    assert_eq!(
        explain_on(Some("feature/crates"), "Assets/Art/Props/SM_Crate.fbx"),
        r#"Assets/Art/Props/SM_Crate.fbx

Type: model
    Files with the extension "fbx" are of type "model", which 3d-review-cli checks.

Profile: Prop (prop)
    The model profiles are tried in file order, and the first that matches wins:
    - profiles[0] "character-model": no; the name "SM_Crate" matches none of SK_*, CH_*.
    - profiles[1] "environment-prop": no; the name matches SM_*, but the path matches none of Assets/Art/Environment/**.
    - profiles[2] "prop": yes; the name matches SM_*, and the profile has no paths.

Stage: prototype, from the project
    The branch "feature/crates" matches no branch rule (proto/**, main, release/*). The profile sets no stage, so the project's stage, prototype, applies.

Rules file: models/prop.json
    The profile has no file for "prototype", so its "*" file applies.
"#
    );
}

#[test]
fn a_detached_head_and_a_profile_stage() {
    assert_eq!(
        explain_on(None, "Assets/Characters/SK_Hero.fbx"),
        r#"Assets/Characters/SK_Hero.fbx

Type: model
    Files with the extension "fbx" are of type "model", which 3d-review-cli checks.

Profile: Character model (character-model)
    The model profiles are tried in file order, and the first that matches wins:
    - profiles[0] "character-model": yes; the name matches SK_*, and the profile has no paths.

Stage: production, from the profile
    HEAD is detached, so no branch rule applies. The profile sets its own stage, production.

Rules file: models/character.json
    The profile has no file for "production", so its "*" file applies.
"#
    );
}

#[test]
fn no_matching_profile_lists_the_patterns_tried() {
    assert_eq!(
        explain_on(Some("main"), "Assets/Art/ComfyUI_00042_.png"),
        r#"Assets/Art/ComfyUI_00042_.png

Type: texture
    Files with the extension "png" are of type "texture", which fire-cli checks.

Profile: none matches
    The texture profiles are tried in file order, and the first that matches wins:
    - profiles[3] "normal-map": no; the name "ComfyUI_00042_" matches none of T_*_N.
    - profiles[4] "texture": no; the name "ComfyUI_00042_" matches none of T_*.
    fire-cli reports this as file.no_profile: a warning, since the rules are in advise mode.
"#
    );
}

#[test]
fn an_ignored_file_lists_the_types() {
    assert_eq!(
        explain_on(None, "Assets/Scenes/Main.unity"),
        r#"Assets/Scenes/Main.unity

Ignored: no type has the extension "unity".
    Types: model (fbx); texture (png, tga, psd).
"#
    );
    assert!(explain_on(None, "Assets/README").contains("Ignored: the file has no extension."));
}

#[test]
fn a_file_outside_the_repository_cant_match_paths() {
    let text = explain_on(None, "../elsewhere/SM_Rock.fbx");
    assert!(
        text.starts_with("../elsewhere/SM_Rock.fbx (outside the repository)\n"),
        "{text}"
    );
    assert!(
        text.contains(
            "- profiles[1] \"environment-prop\": no; the name matches SM_*, but the file is \
             outside the repository, so it can't match the paths Assets/Art/Environment/**."
        ),
        "{text}"
    );
    assert!(text.contains("Profile: Prop (prop)"), "{text}");
}

#[test]
fn enforce_mode_no_rules_file_a_missing_file_and_an_invalid_pattern() {
    let mut rules = sketch();
    rules["mode"] = json!("enforce");
    rules["branches"] = json!([]);
    rules["profiles"][1]["rules"] = json!({ "production": "models/prop.json" });
    rules["profiles"][2]["names"] = json!(["T_[_N", "T_*_Normal"]);
    let dir = temp_dir();
    let rules = Rules::load(&write_rules(&dir, &rules)).unwrap();
    let ctx = Context {
        repo_root: PathBuf::from("/repo"),
        branch: Some("main".to_owned()),
    };

    let prop = explain(
        &rules,
        &ctx,
        Path::new("Assets/Art/Environment/SM_Rock.fbx"),
    );
    assert!(
        prop.contains(
            "The branch is \"main\", but there are no branch rules. The profile sets no stage, \
             so the project's stage, prototype, applies."
        ),
        "{prop}"
    );
    assert!(prop.contains("Rules file: none\n"), "{prop}");
    assert!(prop.contains("rules check reports this."), "{prop}");

    let texture = explain(&rules, &ctx, Path::new("Assets/T_Crate_N.png"));
    assert!(
        texture.contains("matches none of T_[_N (not a valid glob), T_*_Normal."),
        "{texture}"
    );
    assert!(
        texture.contains("an error, since the rules are in enforce mode."),
        "{texture}"
    );

    fs::remove_file(dir.path().join("models/character.json")).unwrap();
    let hero = explain(&rules, &ctx, Path::new("Assets/SK_Hero.fbx"));
    assert!(
        hero.contains("Rules file: models/character.json\n"),
        "{hero}"
    );
    assert!(hero.contains("It doesn't exist: looked for "), "{hero}");
}

#[test]
fn explain_always_agrees_with_resolve() {
    let rules = fixture();
    let paths = [
        "Assets/Art/Environment/SM_Rock.fbx",
        "Assets/Art/Environment/Deep/SM_Rock.fbx",
        "Assets/Art/Props/SM_Crate.fbx",
        "Assets/Characters/SK_Hero.fbx",
        "Assets/Characters/CH_Hero.FBX",
        "Assets/Art/T_Crate_N.png",
        "Assets/Art/T_Crate_D.tga",
        "Assets/Art/t_crate_d.png",
        "Assets/Art/ComfyUI_00042_.png",
        "Assets/Scenes/Main.unity",
        "../elsewhere/SM_Rock.fbx",
    ];
    let branches = [
        None,
        Some("main"),
        Some("proto/rocks/v2"),
        Some("release/1.0"),
        Some("feature/x"),
    ];
    for path in paths {
        for branch in branches {
            let ctx = on(branch);
            let text = explain(&rules, &ctx, Path::new(path));
            let line = |prefix: &str| {
                text.lines()
                    .find(|line| line.starts_with(prefix))
                    .map(str::to_owned)
            };
            match resolve(&rules, &ctx, Path::new(path)) {
                Resolution::Ignored => {
                    assert!(line("Ignored: ").is_some(), "{path} {branch:?}\n{text}")
                }
                Resolution::Skipped { kind, .. } => {
                    assert_eq!(line("Type: "), Some(format!("Type: {kind}")));
                    assert!(line("Skipped: ").is_some(), "{path} {branch:?}\n{text}");
                }
                Resolution::Unmatched { kind } => {
                    assert_eq!(line("Type: "), Some(format!("Type: {kind}")));
                    assert_eq!(line("Profile: "), Some("Profile: none matches".to_owned()));
                }
                Resolution::Resolved(resolved) => {
                    let context = format!("{path} {branch:?}\n{text}");
                    assert_eq!(
                        line("Type: "),
                        Some(format!("Type: {}", resolved.kind)),
                        "{context}"
                    );
                    assert_eq!(
                        line("Profile: "),
                        Some(format!(
                            "Profile: {} ({})",
                            resolved.profile_label, resolved.profile_id
                        )),
                        "{context}"
                    );
                    assert_eq!(
                        line("Stage: "),
                        Some(format!(
                            "Stage: {}, from the {}",
                            resolved.stage,
                            resolved.stage_from.as_str()
                        )),
                        "{context}"
                    );
                    let file = resolved.rules_file.unwrap();
                    let file = file.strip_prefix(rules.dir()).unwrap();
                    assert_eq!(
                        line("Rules file: "),
                        Some(format!("Rules file: {}", file.display())),
                        "{context}"
                    );
                }
            }
        }
    }
}
