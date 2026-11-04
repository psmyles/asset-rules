//! Results v1: the format, writing and reading it, and the exit code (plan item 4).

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use asset_rules::results::{
    ContextInfo, FileResult, Finding, NO_PROFILE_RULE, Results, ResultsError, RulesInfo, Severity,
    Status, ToolInfo,
};
use asset_rules::{Context, Mode, Resolution, Rules, StageFrom, resolve};
use common::{sketch, temp_dir, write_rules};
use serde_json::{Map, Value, json};

fn object(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => panic!("not an object: {value}"),
    }
}

/// The results sketched in `asset-check-plan.md` §3, built through the API.
fn sketch_results() -> Results {
    let mut finding = Finding::new(
        "texture.max_size",
        Severity::Error,
        "Too large",
        "4096×4096. Prop textures may be at most 2048×2048.",
        "On Quest a texture this size takes 16 MB of memory…",
        "Scale the image down to 2048×2048 in Photoshop (Image > Image Size) and save it again.",
    );
    finding.note = Some("Quest budget for props.".to_owned());
    finding.measured = object(json!({ "width": 4096, "height": 4096 }));
    finding.limit = object(json!({ "max": 2048 }));
    Results {
        tool: ToolInfo {
            name: "fire-cli".to_owned(),
            version: "0.1.0".to_owned(),
        },
        rules: RulesInfo {
            commit: Some("1a2b3c4".to_owned()),
            digest: "sha256:…".to_owned(),
        },
        context: ContextInfo {
            branch: Some("main".to_owned()),
            mode: Mode::Advise,
        },
        findings: vec![],
        files: vec![FileResult {
            path: "Assets/Art/Props/T_Crate_D.png".to_owned(),
            kind: Some("texture".to_owned()),
            profile: Some("prop-texture".to_owned()),
            profile_label: Some("Prop texture".to_owned()),
            stage: Some("production".to_owned()),
            stage_from: Some(StageFrom::Branch),
            status: Status::Checked,
            problem: None,
            findings: vec![finding],
        }],
    }
}

/// The §3 sketch, verbatim, plus the run's `findings` (empty here), which the sketch didn't have.
fn sketch_json() -> Value {
    json!({
      "schema": "asset-check.results",
      "version": 1,
      "tool": { "name": "fire-cli", "version": "0.1.0" },
      "rules": { "commit": "1a2b3c4", "digest": "sha256:…" },
      "context": { "branch": "main", "mode": "advise" },
      "findings": [],
      "files": [
        {
          "path": "Assets/Art/Props/T_Crate_D.png",
          "type": "texture", "profile": "prop-texture", "profile_label": "Prop texture",
          "stage": "production", "stage_from": "branch",
          "status": "checked", "problem": null,
          "findings": [
            {
              "rule": "texture.max_size", "severity": "error", "title": "Too large",
              "what": "4096×4096. Prop textures may be at most 2048×2048.",
              "why": "On Quest a texture this size takes 16 MB of memory…",
              "how": "Scale the image down to 2048×2048 in Photoshop (Image > Image Size) and save it again.",
              "note": "Quest budget for props.",
              "measured": { "width": 4096, "height": 4096 }, "limit": { "max": 2048 },
              "objects": [], "fix": null
            }
          ]
        }
      ]
    })
}

/// The keys in pretty-printed JSON, in the order they're written.
fn keys_in_order(json: &str) -> Vec<&str> {
    json.lines()
        .filter_map(|line| line.trim_start().strip_prefix('"'))
        .filter_map(|rest| rest.split_once("\": ").map(|(key, _)| key))
        .collect()
}

fn fixture() -> Rules {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project/rules.json");
    Rules::load(&path).unwrap()
}

fn tool() -> ToolInfo {
    ToolInfo {
        name: "3d-review-cli".to_owned(),
        version: "0.9.0".to_owned(),
    }
}

fn context(branch: Option<&str>) -> Context {
    Context {
        repo_root: PathBuf::from("/repo"),
        branch: branch.map(str::to_owned),
    }
}

/// A finding of `severity` and nothing else.
fn finding(severity: Severity) -> Finding {
    Finding::new("test.rule", severity, "Title", "What.", "Why.", "How.")
}

/// A checked file with findings of these severities.
fn checked(severities: &[Severity]) -> FileResult {
    FileResult {
        path: "Assets/T_Crate_D.png".to_owned(),
        kind: Some("texture".to_owned()),
        profile: Some("texture".to_owned()),
        profile_label: Some("Texture".to_owned()),
        stage: Some("prototype".to_owned()),
        stage_from: Some(StageFrom::Project),
        status: Status::Checked,
        problem: None,
        findings: severities.iter().copied().map(finding).collect(),
    }
}

// --- The format ------------------------------------------------------------------------------

#[test]
fn the_json_matches_the_sketch() {
    let json = sketch_results().to_json();
    let written: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(written, sketch_json());
}

#[test]
fn keys_are_written_in_the_sketchs_order() {
    // Except inside `measured` and `limit`, which are JSON objects the CLI fills: serde_json
    // writes their keys sorted (keeping insertion order would need its `preserve_order` feature
    // and two more dependencies).
    let json = sketch_results().to_json();
    assert_eq!(
        keys_in_order(&json),
        [
            "schema",
            "version",
            "tool",
            "name",
            "version",
            "rules",
            "commit",
            "digest",
            "context",
            "branch",
            "mode",
            "findings",
            "files",
            "path",
            "type",
            "profile",
            "profile_label",
            "stage",
            "stage_from",
            "status",
            "problem",
            "findings",
            "rule",
            "severity",
            "title",
            "what",
            "why",
            "how",
            "note",
            "measured",
            "height",
            "width",
            "limit",
            "max",
            "objects",
            "fix",
        ]
    );
}

#[test]
fn empty_values_are_written_as_null_not_left_out() {
    let mut results = sketch_results();
    results.rules.commit = None;
    results.context.branch = None;
    results.files.push(FileResult {
        path: "Assets/Art/T_Broken.png".to_owned(),
        kind: Some("texture".to_owned()),
        profile: None,
        profile_label: None,
        stage: None,
        stage_from: None,
        status: Status::Unreadable,
        problem: Some("The file is a Git LFS pointer. Run git lfs pull.".to_owned()),
        findings: vec![finding(Severity::Info)],
    });
    let written: Value = serde_json::from_str(&results.to_json()).unwrap();
    assert_eq!(written["rules"]["commit"], Value::Null);
    assert_eq!(written["context"]["branch"], Value::Null);
    let file = &written["files"][1];
    for key in ["profile", "profile_label", "stage", "stage_from"] {
        assert_eq!(file.get(key), Some(&Value::Null), "{key}");
    }
    assert_eq!(file["status"], "unreadable");
    let finding = &file["findings"][0];
    for key in ["note", "measured", "limit", "fix"] {
        assert_eq!(finding.get(key), Some(&Value::Null), "{key}");
    }
    assert_eq!(finding["objects"], json!([]));
    assert_eq!(finding["severity"], "info");
}

// --- Writing and reading ---------------------------------------------------------------------

#[test]
fn results_round_trip_through_a_file() {
    let mut results = sketch_results();
    results.context.mode = Mode::Enforce;
    results.context.branch = None;
    let mut with_objects = finding(Severity::Warning);
    with_objects.objects = vec!["Body".to_owned(), "Head_LOD0".to_owned()];
    results.files.push(FileResult {
        findings: vec![with_objects, finding(Severity::Info)],
        ..checked(&[])
    });
    results.files.push(FileResult {
        status: Status::Skipped,
        problem: Some("Skipped.".to_owned()),
        ..checked(&[])
    });
    results.files.push(FileResult::unmatched(
        "Assets/Crate.fbx",
        "model",
        &fixture(),
    ));

    let dir = temp_dir();
    let path = dir.path().join("results.json");
    results.write(&path).unwrap();
    assert_eq!(Results::read(&path).unwrap(), results);
}

#[test]
fn writing_creates_the_folder_and_replaces_an_earlier_file() {
    let dir = temp_dir();
    let path = dir.path().join("out/asset-check/results.json");
    let mut results = sketch_results();
    results.write(&path).unwrap();
    results.files.clear();
    results.write(&path).unwrap();
    assert_eq!(Results::read(&path).unwrap().files, []);
    let names: Vec<String> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["results.json"], "no temporary file is left behind");
}

#[test]
fn a_failed_write_leaves_no_temporary_file() {
    // A folder where the file should go: the rename over it fails.
    let dir = temp_dir();
    let path = dir.path().join("results.json");
    fs::create_dir(&path).unwrap();
    assert!(sketch_results().write(&path).is_err());
    let names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["results.json"]);
    assert!(path.is_dir());
}

#[test]
fn reading_ignores_fields_it_doesnt_know_and_whatever_fix_holds() {
    let mut json = sketch_json();
    json["generator"] = json!("fire-cli 0.2.0");
    json["files"][0]["findings"][0]["fix"] = json!({ "action": "resize", "to": 2048 });
    json["files"][0]["findings"][0]["confidence"] = json!(0.9);
    let dir = temp_dir();
    let path = dir.write("results.json", &json.to_string());
    assert_eq!(Results::read(&path).unwrap(), sketch_results());
}

#[test]
fn reading_refuses_another_schema_or_version() {
    let dir = temp_dir();
    let other_version = dir.write(
        "v2.json",
        &json!({ "schema": "asset-check.results", "version": 2 }).to_string(),
    );
    let error = Results::read(&other_version).unwrap_err();
    assert!(
        matches!(
            &error,
            ResultsError::Schema {
                version: Some(2),
                ..
            }
        ),
        "{error}"
    );
    let report = dir.write(
        "report.json",
        &json!({ "schema": "3d-review.cli-report", "version": 1 }).to_string(),
    );
    let error = Results::read(&report).unwrap_err();
    assert!(matches!(&error, ResultsError::Schema { .. }), "{error}");
    assert!(
        error.to_string().contains("3d-review.cli-report"),
        "{error}"
    );
}

// --- Exit code -------------------------------------------------------------------------------

#[test]
fn exit_code_is_0_with_nothing_at_warning_or_worse() {
    let mut results = sketch_results();
    results.files = vec![];
    assert_eq!(results.exit_code(), 0);
    results.files = vec![checked(&[]), checked(&[Severity::Info, Severity::Info])];
    assert_eq!(results.exit_code(), 0);
    results.files.push(FileResult {
        status: Status::Skipped,
        problem: Some("Skipped.".to_owned()),
        ..checked(&[])
    });
    assert_eq!(results.exit_code(), 0);
}

#[test]
fn exit_code_is_1_for_a_warning_an_error_or_an_unreadable_file() {
    let mut results = sketch_results();
    results.files = vec![checked(&[Severity::Info, Severity::Warning])];
    assert_eq!(results.exit_code(), 1);
    results.files = vec![checked(&[]), checked(&[Severity::Error])];
    assert_eq!(results.exit_code(), 1);
    results.files = vec![FileResult {
        status: Status::Unreadable,
        problem: Some("Not an image.".to_owned()),
        ..checked(&[])
    }];
    assert_eq!(results.exit_code(), 1);
}

// --- From resolution -------------------------------------------------------------------------

#[test]
fn new_results_record_the_rules_and_context() {
    let rules = fixture();
    let results = Results::new(tool(), &rules, Some("1a2b3c4".to_owned()), &context(None));
    assert_eq!(results.tool, tool());
    assert_eq!(results.rules.digest, rules.digest());
    assert_eq!(results.rules.commit.as_deref(), Some("1a2b3c4"));
    assert_eq!(results.context.branch, None);
    assert_eq!(results.context.mode, Mode::Advise);
    assert_eq!(results.files, []);
}

#[test]
fn a_resolved_file_records_its_profile_and_stage() {
    let rules = fixture();
    let path = "Assets/Characters/SK_Hero.fbx";
    let Resolution::Resolved(resolved) = resolve(&rules, &context(Some("main")), Path::new(path))
    else {
        panic!("resolves");
    };
    let file = FileResult::resolved(path, &resolved);
    assert_eq!(
        file,
        FileResult {
            path: path.to_owned(),
            kind: Some("model".to_owned()),
            profile: Some("character-model".to_owned()),
            profile_label: Some("Character model".to_owned()),
            stage: Some("production".to_owned()),
            stage_from: Some(StageFrom::Branch),
            status: Status::Checked,
            problem: None,
            findings: vec![],
        }
    );
}

#[test]
fn an_unmatched_file_gets_a_no_profile_finding_listing_the_names() {
    let file = FileResult::unmatched("Assets/Art/ComfyUI_00042_.png", "texture", &fixture());
    assert_eq!(file.kind.as_deref(), Some("texture"));
    assert_eq!(
        (file.profile.as_deref(), file.stage.as_deref()),
        (None, None)
    );
    assert_eq!(file.status, Status::Checked);
    let [finding] = file.findings.as_slice() else {
        panic!("one finding: {:?}", file.findings);
    };
    assert_eq!(finding.rule, NO_PROFILE_RULE);
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.title, "Name doesn't match any texture type");
    assert_eq!(
        finding.what,
        "\"ComfyUI_00042_\" doesn't match the name of any texture type."
    );
    assert_eq!(
        finding.how,
        "Rename the file so its name, without the extension, matches one of these (* stands for \
         any text): T_*_N (Normal map), T_* (Texture)."
    );
    assert_eq!(
        finding.measured,
        object(json!({ "name": "ComfyUI_00042_" }))
    );
    assert_eq!(finding.limit, object(json!({ "names": ["T_*_N", "T_*"] })));
}

#[test]
fn the_no_profile_how_shows_paths_and_alternatives() {
    let file = FileResult::unmatched("Assets/Crate.fbx", "model", &fixture());
    assert_eq!(
        file.findings[0].how,
        "Rename the file so its name, without the extension, matches one of these (* stands for \
         any text): SK_* or CH_* (Character model), SM_* in Assets/Art/Environment/** \
         (Environment prop), SM_* (Prop)."
    );
}

#[test]
fn no_profile_is_an_error_in_enforce_mode() {
    let mut rules = sketch();
    rules["mode"] = json!("enforce");
    let dir = temp_dir();
    let rules = Rules::load(&write_rules(&dir, &rules)).unwrap();
    let file = FileResult::unmatched("Assets/Crate.png", "texture", &rules);
    assert_eq!(file.findings[0].severity, Severity::Error);
}

#[test]
fn the_repository_path_of_an_absolute_file() {
    let dir = temp_dir();
    let ctx = Context {
        repo_root: dir.path().to_owned(),
        branch: None,
    };
    let file = dir.path().join("Assets").join("Art").join("T_Crate_D.png");
    assert_eq!(
        ctx.repo_path(&file).as_deref(),
        Some("Assets/Art/T_Crate_D.png")
    );
    assert_eq!(
        ctx.repo_path(Path::new("./Assets/T.png")).as_deref(),
        Some("Assets/T.png")
    );
    assert_eq!(ctx.repo_path(Path::new("../T.png")), None);
}
