//! The code a checker's `build.rs` generates from its catalog. The generated code for both
//! checkers' catalogs is kept in `tests/fixtures/codegen/`, so this test crate compiles it, and a
//! change to what the generator writes shows in review. `ASSET_RULES_BLESS=1` rewrites it.

use std::fs;
use std::path::{Path, PathBuf};

use asset_rules::catalog::Catalog;
use asset_rules::codegen;
use serde_json::json;

mod fire {
    #![allow(dead_code)]
    include!("fixtures/codegen/fire-cli.rs");
}

mod review {
    #![allow(dead_code)]
    include!("fixtures/codegen/3d-review-cli.rs");
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn the_kept_code_is_what_the_generator_writes() {
    for tool in ["fire-cli", "3d-review-cli"] {
        let catalog_path = fixtures().join(format!("catalogs/{tool}.toml"));
        let catalog = Catalog::from_toml(&fs::read_to_string(&catalog_path).unwrap()).unwrap();
        let code = codegen::rust(&catalog, &format!("../catalogs/{tool}.toml"));
        let kept = fixtures().join(format!("codegen/{tool}.rs"));
        if std::env::var_os("ASSET_RULES_BLESS").is_some() {
            fs::write(&kept, &code).unwrap();
        }
        let text = fs::read_to_string(&kept).unwrap().replace("\r\n", "\n");
        assert!(
            text == code,
            "{} is out of date: run with ASSET_RULES_BLESS=1",
            kept.display()
        );
    }
}

#[test]
fn generate_checks_the_catalog_first() {
    let catalog_path = fixtures().join("catalogs/fire-cli.toml");
    let code = codegen::generate(&catalog_path, "0.7.0").expect("no problems");
    assert!(code.contains("include_str!("), "{code}");
    // The rules are newer than an app at 0.6.0.
    let error = codegen::generate(&catalog_path, "0.6.0").unwrap_err();
    assert!(error.contains("has 14 problems"), "{error}");
    assert!(
        error.contains("newer than the app's own version"),
        "{error}"
    );
}

#[test]
fn rule_ids_follow_the_catalog() {
    use fire::RuleId;
    assert_eq!(RuleId::ALL.len(), 14);
    assert_eq!(RuleId::ALL[0], RuleId::PsdComposite);
    assert_eq!(RuleId::MultipleOf4.id(), "texture.multiple_of_4");
    assert_eq!(RuleId::from_id("texture.max_size"), Some(RuleId::MaxSize));
    assert_eq!(RuleId::from_id("texture.max_sise"), None);
    for rule in RuleId::ALL {
        assert_eq!(rule.rule().id, rule.id());
        assert_eq!(RuleId::from_id(rule.id()), Some(rule));
    }
    assert_eq!(fire::catalog().tool, "fire-cli");
}

#[test]
fn parameters_are_typed() {
    use fire::{RuleId, params};
    let defaults = |rule: RuleId| rule.rule().defaults(None);
    assert_eq!(
        params::MaxSize::read(&defaults(RuleId::MaxSize)).max,
        4096_u32
    );
    let depth = params::BitDepth::read(&defaults(RuleId::BitDepth));
    assert_eq!(depth.allowed, vec![8_u32]);
    assert!(!depth.palette);
    assert_eq!(
        params::FileSize::read(&defaults(RuleId::FileSize)).max_mb,
        50.0
    );

    // What a rules file sets, aliases read as their option's value.
    let format = RuleId::Format.rule();
    let settings = format.settings(
        None,
        Some(&json!({ "params": { "allowed": ["PNG", "jpeg"] } })),
    );
    assert_eq!(params::Format::read(&settings).allowed, ["png", "jpg"]);

    // Per base, in 3D Review's.
    use review::RuleId as Review;
    let unit = |base| review::params::Unit::read(&Review::Unit.rule().defaults(Some(base))).target;
    assert_eq!(unit("unity"), 1.0);
    assert_eq!(unit("unreal"), 0.01);
}

#[test]
fn found_fills_the_texts() {
    use fire::{Found, RuleId, found};
    let settings = RuleId::MaxSize
        .rule()
        .settings(None, Some(&json!({ "params": { "max": 2048 } })));
    let size: Found = found::MaxSize {
        width: "4096".into(),
        height: "4096".into(),
    }
    .into();
    assert_eq!(size.rule(), RuleId::MaxSize);
    assert_eq!(size.case(), None);
    let texts = size.texts(None, &settings);
    assert_eq!(texts.title, "Too large");
    assert_eq!(
        texts.what,
        "This image is 4096×4096. The most allowed is 2048×2048."
    );

    // A case replaces the texts it gives.
    let webp: Found = found::Format {
        case: Some(found::FormatCase::UnityCantRead),
        a_label: "a WebP".into(),
        allowed_formats: "PNG, TGA".into(),
        target: "PNG".into(),
    }
    .into();
    assert_eq!(webp.case(), Some("unity_cant_read"));
    let texts = webp.texts(None, &RuleId::Format.rule().defaults(None));
    assert_eq!(
        texts.what,
        "This is a WebP file. Allowed formats: PNG, TGA."
    );
    assert!(
        texts.why.starts_with("Unity's texture importer can't read"),
        "{}",
        texts.why
    );

    // Nothing to name: a unit struct.
    let psd: Found = found::PsdComposite.into();
    assert!(psd.measures().is_empty());
}
