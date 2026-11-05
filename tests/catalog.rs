//! Rule catalogs (format 1): reading, checking, defaults, texts, and rules files read against one.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use asset_rules::Rules;
use asset_rules::catalog::{Catalog, CatalogError, fill, placeholders};
use asset_rules::results::Severity;
use serde_json::{Value, json};

mod common;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load(name: &str) -> Catalog {
    let text = fs::read_to_string(fixture(&format!("catalogs/{name}.toml"))).expect("read");
    Catalog::from_toml(&text).expect("parse")
}

fn fire() -> Catalog {
    load("fire-cli")
}

fn review() -> Catalog {
    load("3d-review-cli")
}

/// The messages of every problem, as `at: message`.
fn problems(catalog: &Catalog, app_version: Option<&str>) -> Vec<String> {
    catalog
        .check(app_version)
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[track_caller]
fn assert_has(problems: &[String], expected: &str) {
    assert!(
        problems.iter().any(|p| p.contains(expected)),
        "no problem containing {expected:?} in:\n{}",
        problems.join("\n")
    );
}

/// A small catalog with every part, as TOML, for tests to change.
const SMALL: &str = r#"
catalog = 1
tool = "fire-cli"

[[category]]
id = "size"
title = "Size"

[[rule]]
id = "texture.max_size"
category = "size"
since = "0.7.0"
title = "Too large"
what = "This image is {width}×{height}. The most allowed is {max}×{max}."
why = "Big textures cost memory."
how = "Scale it down to {max} pixels."
measures = ["width", "height"]

[rule.default]
enabled = false
severity = "error"

[[rule.param]]
key = "max"
kind = "count"
unit = "px"
min = 1
max = 16384
default = 4096
label = "Largest side"

[[rule]]
id = "texture.format"
category = "size"
since = "0.7.0"
title = "Format not allowed"
what = "This is {a_label} file. Allowed: {allowed}."
why = "Agreed formats only."
how = "Save it as {target}."
measures = ["a_label", "target"]

[rule.default]
enabled = true
severity = "error"

[rule.case.unity_cant_read]
why = "Unity can't read it."

[[rule.param]]
key = "allowed"
kind = "choices"
default = ["png", "jpg"]
label = "Allowed formats"
options = [
  { value = "png", label = "PNG" },
  { value = "jpg", label = "JPEG", aliases = ["jpeg", "jfif"] },
  { value = "tga", label = "TGA" },
]
"#;

fn small() -> Catalog {
    Catalog::from_toml(SMALL).expect("parse SMALL")
}

/// SMALL with `from` replaced by `to` (exactly once).
fn small_with(from: &str, to: &str) -> Catalog {
    assert_eq!(SMALL.matches(from).count(), 1, "{from}");
    Catalog::from_toml(&SMALL.replace(from, to)).expect("parse")
}

#[test]
fn both_drafts_have_no_problems() {
    assert_eq!(problems(&fire(), Some("0.7.0")), Vec::<String>::new());
    assert_eq!(problems(&review(), Some("0.9.0")), Vec::<String>::new());
    assert_eq!(fire().rules.len(), 14);
    assert_eq!(review().rules.len(), 43);
}

#[test]
fn the_rules_repos_files_have_no_problems_against_their_catalogs() {
    let (fire, review) = (fire(), review());
    let mut count = 0;
    for entry in fs::read_dir(fixture("style")).expect("style fixtures") {
        let path = entry.expect("entry").path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let catalog = if name.starts_with("textures-") {
            &fire
        } else if name.starts_with("models-") {
            &review
        } else {
            continue;
        };
        let doc: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let found = catalog.check_rules_file(&doc);
        assert!(found.is_empty(), "{name}: {found:#?}");
        count += 1;
    }
    assert!(count >= 5);
}

#[test]
fn a_rules_repo_has_no_problems_against_the_catalogs_when_asked() {
    // ASSET_RULES_STYLE_DIR: a rules repo checkout, as in tests/json.rs.
    let Some(dir) = std::env::var_os("ASSET_RULES_STYLE_DIR") else {
        return;
    };
    let rules = Rules::load(&PathBuf::from(dir).join("rules.json")).unwrap();
    for catalog in [fire(), review()] {
        let found = rules.check_rules_files(&catalog);
        assert!(found.is_empty(), "{}: {found:#?}", catalog.tool);
    }
}

#[test]
fn json_round_trips_with_the_tools_version() {
    for catalog in [fire(), review()] {
        let json = catalog.to_json("1.2.3");
        let back = Catalog::from_json(&json).expect("read back");
        assert_eq!(back.version.as_deref(), Some("1.2.3"));
        let mut expected = catalog.clone();
        expected.version = Some("1.2.3".to_owned());
        assert_eq!(back, expected);
        assert_eq!(problems(&back, None), Vec::<String>::new());
    }
}

#[test]
fn another_format_is_refused() {
    let error = Catalog::from_toml("catalog = 2\ntool = \"x\"").unwrap_err();
    assert!(
        matches!(error, CatalogError::Version(Some(ref v)) if v == "2"),
        "{error}"
    );
    assert!(matches!(
        Catalog::from_toml("tool = \"x\"").unwrap_err(),
        CatalogError::Version(None)
    ));
    assert!(matches!(
        Catalog::from_toml("catalog = 1\n[[rule]]\nid = 3").unwrap_err(),
        CatalogError::Shape(_)
    ));
    assert!(matches!(
        Catalog::from_toml("catalog = = 1").unwrap_err(),
        CatalogError::Toml(_)
    ));
}

#[test]
fn defaults_differ_per_base_where_the_catalog_says() {
    let review = review();
    let unit = review.rule("scene.unit").expect("scene.unit");
    let unity = unit.defaults(Some("unity"));
    let unreal = unit.defaults(Some("unreal"));
    let generic = unit.defaults(Some("generic"));
    assert_eq!((unity.enabled, unity.severity), (true, Severity::Info));
    assert_eq!(unity.params["target"], json!(1.0));
    assert_eq!((unreal.enabled, unreal.severity), (true, Severity::Warning));
    assert_eq!(unreal.params["target"], json!(0.01));
    assert!(!generic.enabled);
    // A base the catalog doesn't have, or none, gives the shared defaults.
    assert_eq!(unit.defaults(None), unit.defaults(Some("godot")));
}

#[test]
fn texts_come_from_the_rule_then_its_base_then_the_case() {
    let review = review();
    let unit = review.rule("scene.unit").unwrap();
    assert!(
        unit.texts(Some("unity"), None)
            .why
            .starts_with("Unity works in meters.")
    );
    assert!(
        unit.texts(Some("unreal"), None)
            .why
            .starts_with("Unreal works in centimeters.")
    );
    assert_eq!(unit.texts(Some("generic"), None).why, unit.why);

    let small = small();
    let format = small.rule("texture.format").unwrap();
    let texts = format.texts(None, Some("unity_cant_read"));
    assert_eq!(texts.why, "Unity can't read it.");
    assert_eq!(texts.what, format.what);
    assert_eq!(format.texts(None, Some("no_such_case")).why, format.why);
}

#[test]
fn texts_are_filled_from_parameters_and_measures() {
    let small = small();
    let rule = small.rule("texture.max_size").unwrap();
    let settings = rule.settings(None, Some(&json!({ "params": { "max": 2048 } })));
    let measures = BTreeMap::from([("width", "4096".to_owned()), ("height", "4096".to_owned())]);
    let texts = rule.fill_texts(&rule.texts(None, None), &settings, &measures);
    assert_eq!(
        texts.what,
        "This image is 4096×4096. The most allowed is 2048×2048."
    );
    assert_eq!(texts.how, "Scale it down to 2048 pixels.");

    let format = small.rule("texture.format").unwrap();
    let settings = format.settings(
        None,
        Some(&json!({ "params": { "allowed": ["PNG", "jpeg"] } })),
    );
    assert_eq!(settings.params["allowed"], json!(["png", "jpg"]));
    let texts = format.fill_texts(&format.texts(None, None), &settings, &BTreeMap::new());
    assert_eq!(texts.what, "This is {a_label} file. Allowed: PNG, JPEG.");
}

#[test]
fn placeholders_and_fill_handle_braces() {
    assert_eq!(
        placeholders("Size {width}×{height}, {{literal}}").unwrap(),
        ["width", "height"]
    );
    assert!(placeholders("open {width").is_err());
    assert!(placeholders("stray } brace").is_err());
    assert!(placeholders("{Width}").is_err());
    let filled = fill("{a} and {b} and {{c}}", |name| {
        (name == "a").then(|| "A".to_owned())
    });
    assert_eq!(filled, "A and {b} and {c}");
}

#[test]
fn settings_are_read_leniently() {
    let small = small();
    let rule = small.rule("texture.max_size").unwrap();
    let settings = rule.settings(
        None,
        Some(&json!({
            "enabled": true,
            "severity": "warning",
            "params": { "max": "big", "unknown": 3 },
            "note": "Quest budget."
        })),
    );
    assert!(settings.enabled);
    assert_eq!(settings.severity, Severity::Warning);
    // The value of the wrong kind keeps the default; the unknown parameter is passed over.
    assert_eq!(
        settings.params,
        BTreeMap::from([("max".to_owned(), json!(4096))])
    );
    assert_eq!(settings.note.as_deref(), Some("Quest budget."));
    // Not listed: the defaults.
    assert_eq!(rule.settings(None, None), rule.defaults(None));
}

#[test]
fn a_listed_rule_is_on_unless_it_says_otherwise() {
    let small = small();
    let rule = small.rule("texture.max_size").unwrap();
    assert!(!rule.defaults(None).enabled, "the fixture's default is off");
    assert!(rule.settings(None, Some(&json!({}))).enabled);
    assert!(
        rule.settings(None, Some(&json!({ "severity": "warning" })))
            .enabled
    );
    assert!(
        !rule
            .settings(None, Some(&json!({ "enabled": false })))
            .enabled
    );
}

#[test]
fn an_entry_that_cant_be_read_takes_the_defaults_whole() {
    let small = small();
    let rule = small.rule("texture.max_size").unwrap();
    for entry in [
        json!("on"),
        json!({ "severity": "catastrophic", "params": { "max": 2048 } }),
        json!({ "enabled": "yes" }),
        json!({ "params": [2048] }),
        json!({ "note": 3 }),
    ] {
        assert_eq!(
            rule.settings(None, Some(&entry)),
            rule.defaults(None),
            "{entry}"
        );
    }
    // A value out of range keeps the default; the rest of the entry still applies.
    let settings = rule.settings(None, Some(&json!({ "params": { "max": 0 } })));
    assert!(settings.enabled);
    assert_eq!(settings.params["max"], json!(4096));
}

#[test]
fn a_rules_file_is_checked_strictly() {
    let small = small();
    let doc = json!({
        "version": 1,
        "generator": "hand-written",
        "profile": {
            "name": "Test",
            "base": "unity",
            "rules": {
                "texture.max_sise": { "enabled": false },
                "texture.max_size": {
                    "severity": "fatal",
                    "params": { "max": 0, "maximum": 2 },
                    "colour": "red"
                },
                "texture.format": {
                    "enabled": "yes",
                    "params": { "allowed": ["PNG", "jfif", "webp"] },
                    "note": 3
                }
            }
        }
    });
    let found: Vec<String> = small
        .check_rules_file(&doc)
        .iter()
        .map(ToString::to_string)
        .collect();
    for expected in [
        "profile.base: fire-cli has no default sets",
        "rules[\"texture.max_sise\"]: fire-cli has no rule \"texture.max_sise\"",
        "severity \"fatal\" isn't error, warning or info",
        "rules[\"texture.max_size\"].params.max: 0 is less than the least allowed, 1",
        "\"maximum\" isn't a parameter of this rule",
        "unknown field \"colour\"",
        "\"enabled\" isn't true or false",
        "\"note\" isn't text",
        "\"webp\" isn't one of",
    ] {
        assert_has(&found, expected);
    }
    // "PNG" and the alias "jfif" are accepted.
    assert!(
        !found
            .iter()
            .any(|p| p.contains("PNG") || p.contains("jfif")),
        "{found:#?}"
    );
    assert_eq!(found.len(), 9, "{found:#?}");
}

#[test]
fn every_kind_of_catalog_problem_is_reported() {
    let cases: &[(&str, &str, &str)] = &[
        ("tool = \"fire-cli\"", "tool = \"\"", "tool: is empty"),
        (
            "since = \"0.7.0\"\ntitle = \"Too large\"",
            "since = \"0.8.0\"\ntitle = \"Too large\"",
            "is newer than the app's own version",
        ),
        (
            "since = \"0.7.0\"\ntitle = \"Too large\"",
            "since = \"soon\"\ntitle = \"Too large\"",
            "isn't a version",
        ),
        (
            "id = \"texture.max_size\"",
            "id = \"texture.format\"",
            "the id is used more than once",
        ),
        (
            "id = \"texture.max_size\"",
            "id = \"MaxSize\"",
            "isn't lowercase words joined by",
        ),
        (
            "category = \"size\"\nsince = \"0.7.0\"\ntitle = \"Too large\"",
            "category = \"bulk\"\nsince = \"0.7.0\"\ntitle = \"Too large\"",
            "category \"bulk\" isn't one of the categories",
        ),
        (
            "title = \"Too large\"",
            "title = \"Too {max}\"",
            "\"title\" can't have placeholders",
        ),
        (
            "why = \"Big textures cost memory.\"",
            "why = \"Big {depth} textures.\"",
            "uses {depth}, which is neither a parameter",
        ),
        (
            "measures = [\"width\", \"height\"]",
            "measures = [\"width\", \"height\", \"max\"]",
            "measure \"max\" has the same name as a parameter",
        ),
        (
            "why = \"Unity can't read it.\"",
            "",
            "case \"unity_cant_read\": gives no text",
        ),
        (
            "default = 4096",
            "default = 0",
            "default: 0 is less than the least allowed",
        ),
        ("default = 4096", "", "has no default"),
        (
            "unit = \"px\"\nmin = 1",
            "unit = \"px\"\nmin = 20000",
            "min 20000 is more than max 16384",
        ),
        (
            "default = [\"png\", \"jpg\"]",
            "default = [\"png\", \"jpeg\"]",
            "default: \"jpeg\" isn't one of",
        ),
        (
            "aliases = [\"jpeg\", \"jfif\"]",
            "aliases = [\"jpeg\", \"PNG\"]",
            "\"PNG\" is listed more than once",
        ),
        (
            "[rule.case.unity_cant_read]",
            "[rule.base.unity]",
            "the catalog has no \"bases\"",
        ),
        (
            "[rule.default]\nenabled = false",
            "[rule.default]\nenabled = false\ncolour = 1",
            "unknown field \"colour\"",
        ),
    ];
    for (from, to, expected) in cases {
        let found = problems(&small_with(from, to), Some("0.7.0"));
        assert_has(&found, expected);
    }
    // And SMALL itself has none.
    assert_eq!(problems(&small(), Some("0.7.0")), Vec::<String>::new());
}

#[test]
fn bases_must_be_listed_and_give_every_parameter_a_value() {
    let catalog = Catalog::from_toml(
        &SMALL
            .replace("tool = \"fire-cli\"", "tool = \"fire-cli\"\nbases = [\"unity\", \"unreal\"]")
            .replace("default = 4096\n", "")
            .replace(
                "[[rule.param]]\nkey = \"max\"",
                "[rule.base.unity]\nparams = { max = 2048 }\n[rule.base.godot]\nseverity = \"info\"\n\n[[rule.param]]\nkey = \"max\"",
            ),
    )
    .expect("parse");
    let found = problems(&catalog, None);
    assert_has(
        &found,
        "base \"godot\": \"godot\" isn't one of the bases (unity, unreal)",
    );
    assert_has(
        &found,
        "has no default, and these sets give it none: unreal",
    );
}

#[test]
fn code_names_come_from_the_id_unless_named() {
    let review = review();
    let names: BTreeMap<&str, String> = review
        .rules
        .iter()
        .map(|rule| (rule.id.as_str(), rule.code_name()))
        .collect();
    assert_eq!(
        names["geometry.degenerate_triangles"],
        "DegenerateTriangles"
    );
    assert_eq!(names["uv.missing"], "UvMissing");
    assert_eq!(small().rules[0].code_name(), "MaxSize");

    let clash = small_with("id = \"texture.format\"", "id = \"model.max_size\"");
    assert_has(
        &problems(&clash, None),
        "its code name \"MaxSize\" is also rule",
    );
}

#[test]
fn rules_check_reads_a_checkers_own_rules_files_against_its_catalog() {
    let dir = common::temp_dir();
    let rules_json = common::write_rules(&dir, &common::sketch());
    // The texture profile's file: a misspelt rule, and a value out of range.
    dir.write(
        "textures/normal.json",
        r#"{"version": 1, "profile": {"name": "Normal map", "rules": {
            "texture.max_sise": {}, "texture.max_size": {"params": {"max": 0}}}}}"#,
    );
    // A model rules file is 3d-review-cli's, so fire-cli's catalog doesn't read it.
    dir.write(
        "models/character.json",
        r#"{"version": 1, "profile": {"rules": {"geometry.ngons": {}}}}"#,
    );
    let rules = Rules::load(&rules_json).expect("load");
    let found = |catalog: &Catalog| -> Vec<String> {
        rules
            .check_rules_files(catalog)
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    assert_eq!(
        found(&fire()),
        [
            r#"error: textures/normal.json rules["texture.max_sise"]: fire-cli has no rule "texture.max_sise""#,
            r#"error: textures/normal.json rules["texture.max_size"].params.max: 0 is less than the least allowed, 1"#,
        ]
    );

    // A file that isn't JSON says so; one that isn't there is left to `Rules::check`.
    dir.write("textures/normal.json", "{");
    let problems = found(&fire());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].starts_with("error: textures/normal.json: isn't valid JSON"),
        "{problems:?}"
    );
    fs::remove_file(dir.path().join("textures/normal.json")).expect("remove");
    assert!(found(&fire()).is_empty());
}

/// JSON doesn't tell `6` from `6.0`, and some writers (3D Review's own) write the second.
#[test]
fn a_whole_number_written_with_a_point_is_a_count() {
    let small = small();
    let rule = small.rule("texture.max_size").unwrap();
    let max = rule.param("max").unwrap();
    assert!(max.check_value(&json!(2048.0), true).is_ok());
    assert!(max.check_value(&json!(2048.5), false).is_err());
    let settings = rule.settings(None, Some(&json!({ "params": { "max": 2048.0 } })));
    assert_eq!(settings.params["max"], json!(2048));
    assert!(settings.params["max"].is_u64());
}
