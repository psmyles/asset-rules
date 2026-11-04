//! `rules.json` v1: loading it, and every problem `rules check` reports (plan item 2).

mod common;

use std::fs;

use asset_rules::{Level, Mode, Problem, ProblemKind, Rules, RulesError};
use common::{TempDir, sketch, temp_dir, write_rules};
use serde_json::{Value, json};

/// Writes `rules` (and the rules files it names) into a new folder and loads it.
fn setup(rules: &Value) -> (TempDir, Rules) {
    let dir = temp_dir();
    let path = write_rules(&dir, rules);
    let loaded = Rules::load(&path).expect("load rules.json");
    (dir, loaded)
}

fn check(rules: &Value) -> Vec<Problem> {
    setup(rules).1.check()
}

/// Each problem's kind and location, for comparing against exactly what is expected.
fn found(problems: &[Problem]) -> Vec<(ProblemKind, &str)> {
    problems.iter().map(|p| (p.kind, p.at.as_str())).collect()
}

/// Writes `text` as `rules.json` and tries to load it.
fn load_text(text: &str) -> Result<Rules, RulesError> {
    let dir = temp_dir();
    Rules::load(&dir.write("rules.json", text))
}

/// `rules` with `profiles` replaced.
fn with_profiles(profiles: Value) -> Value {
    let mut rules = sketch();
    rules["profiles"] = profiles;
    rules
}

/// A texture profile with one rules file for every stage.
fn texture(id: &str, names: &[&str]) -> Value {
    json!({
        "id": id, "type": "texture", "label": id, "names": names,
        "rules": { "*": format!("textures/{id}.json") }
    })
}

/// [`texture`] narrowed to `paths`.
fn texture_in(id: &str, names: &[&str], paths: &[&str]) -> Value {
    let mut profile = texture(id, names);
    profile["paths"] = json!(paths);
    profile
}

// --- Loading ---------------------------------------------------------------------------------

#[test]
fn the_sketch_loads_and_has_no_problems() {
    let (dir, rules) = setup(&sketch());
    assert_eq!(rules.check(), []);
    assert_eq!(rules.mode(), Mode::Advise);
    assert_eq!(rules.stages(), ["prototype", "production"]);
    assert_eq!(rules.stage(), "prototype");
    assert_eq!(rules.branches()[0].pattern.as_str(), "proto/*");
    assert_eq!(rules.requires()["3d-review-cli"], "0.9.0");
    assert_eq!(rules.types()["model"].tool, "3d-review-cli");
    assert_eq!(rules.profiles()[1].id, "prop-model");
    assert_eq!(rules.dir(), dir.path());
    assert_eq!(
        rules.file_path("models/prop.json"),
        dir.path().join("models/prop.json")
    );
}

#[test]
fn a_profile_falls_back_to_its_catch_all_file() {
    let (_dir, rules) = setup(&sketch());
    let character = &rules.profiles()[0];
    assert_eq!(character.rules_file("prototype"), Some("models/character-proto.json"));
    assert_eq!(character.rules_file("production"), Some("models/character.json"));
    let prop = &rules.profiles()[1];
    assert_eq!(prop.rules_file("review"), None);
}

#[test]
fn extensions_are_lowercased() {
    let mut rules = sketch();
    rules["types"]["model"]["extensions"] = json!(["FBX", "Fbx"]);
    let (_dir, rules) = setup(&rules);
    assert_eq!(rules.types()["model"].extensions, ["fbx", "fbx"]);
}

#[test]
fn the_digest_is_the_sha256_of_the_file() {
    let dir = temp_dir();
    let text = serde_json::to_string(&sketch()).unwrap();
    let a = Rules::load(&dir.write("a.json", &text)).unwrap();
    let b = Rules::load(&dir.write("b.json", &text)).unwrap();
    let c = Rules::load(&dir.write("c.json", &format!("{text}\n"))).unwrap();
    assert_eq!(a.digest().len(), "sha256:".len() + 64);
    assert!(a.digest().starts_with("sha256:"));
    assert_eq!(a.digest(), b.digest());
    assert_ne!(a.digest(), c.digest());
}

#[test]
fn a_missing_file_is_a_read_error() {
    let dir = temp_dir();
    let error = Rules::load(&dir.path().join("rules.json")).unwrap_err();
    assert!(matches!(error, RulesError::Read { .. }), "{error}");
}

#[test]
fn text_that_isnt_json_is_a_json_error() {
    let error = load_text("{ \"version\": 1,").unwrap_err();
    assert!(matches!(error, RulesError::Json { .. }), "{error}");
}

#[test]
fn a_missing_field_is_a_json_error_that_names_it() {
    let mut rules = sketch();
    rules.as_object_mut().unwrap().remove("stage");
    let error = load_text(&rules.to_string()).unwrap_err();
    assert!(matches!(error, RulesError::Json { .. }), "{error}");
    assert!(error.to_string().contains("missing field `stage`"), "{error}");
}

#[test]
fn an_unknown_mode_is_a_json_error() {
    let mut rules = sketch();
    rules["mode"] = json!("block");
    let error = load_text(&rules.to_string()).unwrap_err();
    assert!(matches!(error, RulesError::Json { .. }), "{error}");
}

#[test]
fn another_version_is_refused_before_its_shape_is_read() {
    let error = load_text(r#"{ "version": 2, "something": "new" }"#).unwrap_err();
    assert!(
        matches!(&error, RulesError::Version { found: Some(v), .. } if v == "2"),
        "{error}"
    );
    assert!(error.to_string().contains("may need updating"), "{error}");

    let mut rules = sketch();
    rules.as_object_mut().unwrap().remove("version");
    let error = load_text(&rules.to_string()).unwrap_err();
    assert!(matches!(error, RulesError::Version { found: None, .. }), "{error}");
}

#[test]
fn branches_and_requires_may_be_left_out() {
    let mut rules = sketch();
    let object = rules.as_object_mut().unwrap();
    object.remove("branches");
    object.remove("requires");
    let (_dir, rules) = setup(&rules);
    assert!(rules.branches().is_empty());
    assert!(rules.requires().is_empty());
    assert_eq!(rules.check(), []);
}

// --- rules check: the problems in the plan ---------------------------------------------------

#[test]
fn unknown_type() {
    let mut rules = sketch();
    rules["profiles"][2]["type"] = json!("textures");
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [(ProblemKind::UnknownType, "profiles[2] \"normal-map\"")]
    );
    assert_eq!(problems[0].level(), Level::Error);
    assert_eq!(
        problems[0].to_string(),
        "error: profiles[2] \"normal-map\": type \"textures\" is not one of the types in \
         \"types\" (model, texture)"
    );
}

#[test]
fn unknown_stage_wherever_a_stage_is_named() {
    let mut rules = sketch();
    rules["stage"] = json!("prod");
    rules["branches"][1]["stage"] = json!("release");
    rules["profiles"][0]["stage"] = json!("final");
    rules["profiles"][1]["rules"]["beta"] = json!("models/prop-beta.json");
    assert_eq!(
        found(&check(&rules)),
        [
            (ProblemKind::UnknownStage, "stage"),
            (ProblemKind::UnknownStage, "branches[1]"),
            (ProblemKind::UnknownStage, "profiles[0] \"character-model\""),
            (ProblemKind::UnknownStage, "profiles[1] \"prop-model\""),
        ]
    );
}

#[test]
fn unknown_tool_in_a_type_or_in_requires() {
    let mut rules = sketch();
    rules["types"]["texture"]["tool"] = json!("fire");
    rules["requires"]["3d-reveiw-cli"] = json!("0.9.0");
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [
            (ProblemKind::UnknownTool, "requires \"3d-reveiw-cli\""),
            (ProblemKind::UnknownTool, "types \"texture\""),
        ]
    );
}

#[test]
fn requires_can_name_the_launcher() {
    let mut rules = sketch();
    rules["requires"]["launcher"] = json!("0.2.0");
    assert_eq!(check(&rules), []);
}

#[test]
fn a_stage_with_no_rules_file_and_no_catch_all() {
    let mut rules = sketch();
    rules["profiles"][2]["rules"] = json!({ "production": "textures/normal.json" });
    rules["profiles"][1]["rules"] = json!({});
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [
            (ProblemKind::MissingStageFile, "profiles[1] \"prop-model\""),
            (ProblemKind::MissingStageFile, "profiles[2] \"normal-map\""),
        ]
    );
    assert!(problems[0].message.contains("stages \"prototype\", \"production\""));
    assert!(problems[1].message.contains("stage \"prototype\","));
}

#[test]
fn a_rules_file_that_doesnt_exist() {
    let (dir, rules) = setup(&sketch());
    fs::remove_file(dir.path().join("models/prop-proto.json")).unwrap();
    let problems = rules.check();
    assert_eq!(
        found(&problems),
        [(ProblemKind::RulesFileNotFound, "profiles[1] \"prop-model\"")]
    );
    assert!(problems[0].message.contains("\"models/prop-proto.json\""));
}

#[test]
fn an_invalid_glob_in_names_paths_or_branches() {
    let mut rules = sketch();
    rules["branches"][0]["pattern"] = json!("proto/[");
    rules["profiles"][1]["paths"] = json!(["Assets/{Art"]);
    rules["profiles"][2]["names"] = json!(["T_[_N"]);
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [
            (ProblemKind::InvalidGlob, "branches[0]"),
            (ProblemKind::InvalidGlob, "profiles[1] \"prop-model\""),
            (ProblemKind::InvalidGlob, "profiles[2] \"normal-map\""),
        ]
    );
    assert!(problems[0].message.contains("\"proto/[\""), "{}", problems[0]);
}

#[test]
fn duplicate_profile_ids() {
    let mut rules = sketch();
    rules["profiles"][2]["id"] = json!("character-model");
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [(ProblemKind::DuplicateId, "profiles[2] \"character-model\"")]
    );
    assert!(problems[0].message.contains("profiles[0]"));
}

#[test]
fn duplicate_stages() {
    let mut rules = sketch();
    rules["stages"] = json!(["prototype", "production", "prototype"]);
    assert_eq!(found(&check(&rules)), [(ProblemKind::DuplicateStage, "stages")]);
}

// --- rules check: profiles that can never match ----------------------------------------------

#[test]
fn a_wider_profile_first_hides_a_narrower_one() {
    let rules = with_profiles(json!([
        texture("prop-texture", &["T_*"]),
        texture("normal-map", &["T_*_N"]),
    ]));
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [(ProblemKind::NeverMatches, "profiles[1] \"normal-map\"")]
    );
    assert_eq!(problems[0].level(), Level::Warning);
    assert!(problems[0].message.contains("profiles[0] \"prop-texture\""));
}

#[test]
fn a_narrower_profile_first_is_fine() {
    let rules = with_profiles(json!([
        texture("normal-map", &["T_*_N"]),
        texture("prop-texture", &["T_*"]),
    ]));
    assert_eq!(check(&rules), []);
}

#[test]
fn a_catch_all_hides_everything_after_it() {
    let rules = with_profiles(json!([
        texture("any-texture", &["*"]),
        texture("normal-map", &["T_*_N"]),
        texture("mask", &["M_[A-Z]*"]),
    ]));
    assert_eq!(
        found(&check(&rules)),
        [
            (ProblemKind::NeverMatches, "profiles[1] \"normal-map\""),
            (ProblemKind::NeverMatches, "profiles[2] \"mask\""),
        ]
    );
}

#[test]
fn a_copied_profile_is_hidden_by_the_original() {
    let rules = with_profiles(json!([
        texture("normal-map", &["T_*_N"]),
        texture("normal-map-copy", &["T_*_N"]),
    ]));
    assert_eq!(
        found(&check(&rules)),
        [(ProblemKind::NeverMatches, "profiles[1] \"normal-map-copy\"")]
    );
}

#[test]
fn every_name_must_be_covered() {
    let partly = with_profiles(json!([
        texture("skin", &["SK_*"]),
        texture("character", &["SK_*", "CH_*"]),
    ]));
    assert_eq!(check(&partly), []);

    let fully = with_profiles(json!([
        texture("people", &["SK_*", "CH_*", "NPC_*"]),
        texture("character", &["SK_*", "CH_*"]),
    ]));
    assert_eq!(
        found(&check(&fully)),
        [(ProblemKind::NeverMatches, "profiles[1] \"character\"")]
    );
}

#[test]
fn single_character_wildcards_cover_less_than_stars() {
    let rules = with_profiles(json!([
        texture("short", &["T_?"]),
        texture("any", &["T_*"]),
    ]));
    assert_eq!(check(&rules), []);
}

#[test]
fn disjoint_names_and_other_types_dont_hide() {
    let mut rules = with_profiles(json!([
        texture("diffuse", &["T_*_D"]),
        texture("normal", &["T_*_N"]),
    ]));
    // A model catch-all doesn't hide texture profiles.
    let mut models = json!([{
        "id": "any-model", "type": "model", "label": "Model", "names": ["*"],
        "rules": { "*": "models/any.json" }
    }]);
    models
        .as_array_mut()
        .unwrap()
        .extend(rules["profiles"].as_array().unwrap().iter().cloned());
    rules["profiles"] = models;
    assert_eq!(check(&rules), []);
}

#[test]
fn paths_narrow_what_an_earlier_profile_hides() {
    // The earlier profile is narrowed to UI; the later one matches anywhere.
    let narrower_first = with_profiles(json!([
        texture_in("ui", &["T_*"], &["Assets/UI/**"]),
        texture("normal-map", &["T_*_N"]),
    ]));
    assert_eq!(check(&narrower_first), []);

    // The earlier profile matches anywhere, so narrowing the later one doesn't save it.
    let wider_first = with_profiles(json!([
        texture("any", &["T_*"]),
        texture_in("env-normal", &["T_*_N"], &["Assets/Art/Environment/**"]),
    ]));
    assert_eq!(
        found(&check(&wider_first)),
        [(ProblemKind::NeverMatches, "profiles[1] \"env-normal\"")]
    );

    // A folder covers the folders inside it, not the other way round.
    let outer_first = with_profiles(json!([
        texture_in("art", &["T_*"], &["Assets/Art/**"]),
        texture_in("env", &["T_*"], &["Assets/Art/Environment/**"]),
    ]));
    assert_eq!(
        found(&check(&outer_first)),
        [(ProblemKind::NeverMatches, "profiles[1] \"env\"")]
    );
    let inner_first = with_profiles(json!([
        texture_in("env", &["T_*"], &["Assets/Art/Environment/**"]),
        texture_in("art", &["T_*"], &["Assets/Art/**"]),
    ]));
    assert_eq!(check(&inner_first), []);
}

#[test]
fn a_profile_with_nothing_to_match() {
    let rules = with_profiles(json!([
        texture("no-names", &[]),
        texture_in("no-paths", &["T_*"], &[]),
    ]));
    assert_eq!(
        found(&check(&rules)),
        [
            (ProblemKind::NeverMatches, "profiles[0] \"no-names\""),
            (ProblemKind::NeverMatches, "profiles[1] \"no-paths\""),
        ]
    );
}

// --- rules check: further mistakes it catches ------------------------------------------------

#[test]
fn an_extension_in_two_types() {
    let mut rules = sketch();
    rules["types"]["texture"]["extensions"] = json!(["png", "FBX"]);
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [(ProblemKind::DuplicateExtension, "types \"texture\"")]
    );
    assert!(problems[0].message.contains("\"fbx\" is also in type \"model\""));
}

#[test]
fn an_extension_with_a_dot_or_none() {
    let mut rules = sketch();
    rules["types"]["texture"]["extensions"] = json!([".png", "tar.gz", ""]);
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [
            (ProblemKind::InvalidExtension, "types \"texture\""),
            (ProblemKind::InvalidExtension, "types \"texture\""),
            (ProblemKind::InvalidExtension, "types \"texture\""),
        ]
    );
    assert!(problems[0].message.contains("write \"png\""));
}

#[test]
fn unknown_fields_load_and_are_warned_about() {
    let mut rules = sketch();
    rules["comment"] = json!("Quest budgets");
    rules["profiles"][1]["path"] = json!(["Assets/Art/**"]);
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [
            (ProblemKind::UnknownField, "top level"),
            (ProblemKind::UnknownField, "profiles[1] \"prop-model\""),
        ]
    );
    assert!(problems.iter().all(|p| p.level() == Level::Warning));
    assert!(problems[1].message.contains("\"path\""));
    assert!(problems[1].message.contains("paths"));
}

#[test]
fn a_backslash_in_a_path_pattern() {
    let mut rules = sketch();
    rules["profiles"][1]["paths"] = json!(["Assets\\Art\\Environment\\**"]);
    let problems = check(&rules);
    assert_eq!(
        found(&problems),
        [(ProblemKind::Backslash, "profiles[1] \"prop-model\"")]
    );
    assert!(problems[0].message.contains("write \"Assets/Art/Environment/**\""));
}
