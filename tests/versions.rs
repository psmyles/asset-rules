//! Tool versions: `requires` against a CLI's own version (plan item 7).

mod common;

use asset_rules::results::{FileResult, Results, Severity, TOO_OLD_RULE, ToolInfo};
use asset_rules::{Context, ProblemKind, Rules, RulesError};
use common::{sketch, temp_dir, write_rules};
use serde_json::{Value, json};

/// Loads `rules` from a folder that is removed straight away: `too_old` reads no rules file.
/// The sketch requires fire-cli 0.1.0 and review3d-cli 0.9.0.
fn load(rules: &Value) -> Rules {
    let dir = temp_dir();
    Rules::load(&write_rules(&dir, rules)).unwrap()
}

fn review(version: &str) -> ToolInfo {
    ToolInfo {
        name: "review3d-cli".to_owned(),
        version: version.to_owned(),
    }
}

#[test]
fn a_tool_at_or_above_the_required_version_is_fine() {
    let rules = load(&sketch());
    for version in ["0.9.0", "0.9.1", "0.10.0", "1.0.0", "0.9.0+build.7"] {
        assert_eq!(rules.too_old(&review(version)), None, "{version}");
    }
}

#[test]
fn a_tool_that_is_too_old_gets_one_finding_saying_what_to_update() {
    let rules = load(&sketch());
    let finding = rules.too_old(&review("0.8.2")).expect("too old");
    assert_eq!(finding.rule, TOO_OLD_RULE);
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(finding.title, "3D Review is too old");
    assert_eq!(
        finding.what,
        "This is review3d-cli 0.8.2, but the rules need 0.9.0 or later."
    );
    assert_eq!(
        finding.how,
        "Update 3D Review to version 0.9.0 or later, then check again."
    );
    assert_eq!(
        Value::Object(finding.measured.clone().unwrap()),
        json!({ "version": "0.8.2" })
    );
    assert_eq!(
        Value::Object(finding.limit.clone().unwrap()),
        json!({ "min_version": "0.9.0" })
    );

    let fire = ToolInfo {
        name: "fire-cli".to_owned(),
        version: "0.0.9".to_owned(),
    };
    assert_eq!(
        rules.too_old(&fire).unwrap().how,
        "Update Fire to version 0.1.0 or later, then check again."
    );
}

#[test]
fn a_pre_release_of_the_required_version_is_too_old() {
    let rules = load(&sketch());
    assert!(rules.too_old(&review("0.9.0-beta.3")).is_some());
}

#[test]
fn versions_compare_as_numbers_not_text() {
    let mut rules = sketch();
    rules["requires"]["review3d-cli"] = json!("0.10.0");
    let rules = load(&rules);
    assert!(rules.too_old(&review("0.9.0")).is_some());
    assert!(rules.too_old(&review("0.10.0")).is_none());
}

#[test]
fn a_short_version_means_zeros() {
    let mut rules = sketch();
    rules["requires"]["review3d-cli"] = json!("0.9");
    let rules = load(&rules);
    assert!(rules.too_old(&review("0.9.0")).is_none());
    assert!(rules.too_old(&review("0.8.9")).is_some());
}

#[test]
fn a_tool_not_in_requires_is_never_too_old() {
    let mut rules = sketch();
    rules.as_object_mut().unwrap().remove("requires");
    assert_eq!(load(&rules).too_old(&review("0.0.1")), None);
}

#[test]
fn too_old_is_an_error_in_enforce_mode() {
    let mut rules = sketch();
    rules["mode"] = json!("enforce");
    let finding = load(&rules).too_old(&review("0.8.0")).unwrap();
    assert_eq!(finding.severity, Severity::Error);
}

#[test]
fn rules_check_reports_a_requirement_that_isnt_a_version() {
    let mut rules = sketch();
    rules["requires"]["review3d-cli"] = json!("latest");
    let dir = temp_dir();
    let rules = Rules::load(&write_rules(&dir, &rules)).unwrap();
    let problems = rules.check();
    let found: Vec<(ProblemKind, &str)> =
        problems.iter().map(|p| (p.kind, p.at.as_str())).collect();
    assert_eq!(
        found,
        [(ProblemKind::InvalidVersion, "requires \"review3d-cli\"")]
    );
    // And it never makes a tool "too old".
    assert_eq!(rules.too_old(&review("0.0.1")), None);
}

#[test]
fn a_newer_rules_json_still_says_what_to_update() {
    // The technical artist moved to a rules.json this tool can't read, and raised `requires`.
    let dir = temp_dir();
    let path = dir.write(
        "rules.json",
        &json!({ "version": 3, "requires": { "review3d-cli": "1.2.0" }, "profiles": "new shape" })
            .to_string(),
    );
    let error = Rules::load(&path).unwrap_err();
    assert!(matches!(error, RulesError::Version { .. }), "{error}");
    let finding = error.too_old(&review("0.9.0")).expect("too old");
    assert_eq!(finding.severity, Severity::Error);
    assert_eq!(
        finding.how,
        "Update 3D Review to version 1.2.0 or later, then check again."
    );
    // A tool that is new enough by `requires` but still can't read the file gets no such finding:
    // the load error itself is what to report.
    assert_eq!(error.too_old(&review("1.2.0")), None);
}

#[test]
fn other_load_errors_have_no_too_old_finding() {
    let dir = temp_dir();
    let error = Rules::load(&dir.path().join("rules.json")).unwrap_err();
    assert_eq!(error.too_old(&review("0.0.1")), None);
}

#[test]
fn a_too_old_finding_belongs_to_the_run_and_fails_it() {
    let rules = load(&sketch());
    let ctx = Context::default();
    let mut results = Results::new(review("0.8.0"), &rules, None, &ctx);
    assert_eq!(results.exit_code(), 0);
    results.findings.extend(rules.too_old(&results.tool));
    assert_eq!(results.findings.len(), 1);
    assert_eq!(results.exit_code(), 1);

    let written: Value = serde_json::from_str(&results.to_json()).unwrap();
    assert_eq!(written["findings"][0]["rule"], TOO_OLD_RULE);

    let dir = temp_dir();
    let path = dir.path().join("results.json");
    results
        .files
        .push(FileResult::unmatched("Assets/Crate.fbx", "model", &rules));
    results.write(&path).unwrap();
    assert_eq!(Results::read(&path).unwrap(), results);
}
