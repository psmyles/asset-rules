//! The house JSON style: every tool that writes `rules.json` or a rules file writes the same bytes.

use std::fs;
use std::path::{Path, PathBuf};

use asset_rules::json::Node;
use serde_json::json;

fn style_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/style")
}

/// The file's text with LF line endings, however git checked it out.
fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .expect("read a fixture")
        .replace("\r\n", "\n")
}

#[test]
fn the_rules_repos_files_come_back_byte_for_byte() {
    let mut count = 0;
    for entry in fs::read_dir(style_dir()).expect("the style fixtures") {
        let path = entry.expect("an entry").path();
        let text = read(&path);
        let node = Node::parse(&text).expect("parse");
        assert_eq!(node.to_json(), text, "{}", path.display());
        count += 1;
    }
    assert!(count >= 6, "only {count} fixtures");
}

/// A rules repo's `rules.json` and every rules file it names, when `ASSET_RULES_STYLE_DIR` names
/// the repo (e.g. a checkout of exp-content-rules).
#[test]
fn a_whole_rules_repo_comes_back_byte_for_byte_when_asked() {
    let Some(dir) = std::env::var_os("ASSET_RULES_STYLE_DIR") else {
        return;
    };
    let rules_json = PathBuf::from(dir).join("rules.json");
    let rules = asset_rules::Rules::load(&rules_json).expect("load rules.json");
    let mut files: Vec<PathBuf> = vec![rules_json];
    for profile in rules.profiles() {
        for file in profile.rules.values() {
            let path = rules.file_path(file);
            if !files.contains(&path) {
                files.push(path);
            }
        }
    }
    for path in &files {
        let text = read(path);
        let node = Node::parse(&text).expect("parse");
        assert_eq!(node.to_json(), text, "{}", path.display());
    }
    println!("{} files round-trip", files.len());
}

#[test]
fn keys_keep_their_order() {
    let node =
        Node::parse(r#"{"version": 1, "generator": "x", "profile": {"name": "a", "rules": {}}}"#)
            .unwrap();
    assert_eq!(
        node.to_json(),
        "{\"version\": 1, \"generator\": \"x\", \"profile\": {\"name\": \"a\", \"rules\": {}}}\n"
    );
}

#[test]
fn insert_replaces_in_place_or_appends_and_remove_takes_out() {
    let mut node = Node::parse(r#"{"enabled": false, "severity": "warning"}"#).unwrap();
    assert_eq!(
        node.insert("enabled", Node::Bool(true)),
        Some(Node::Bool(false))
    );
    assert_eq!(node.insert("note", Node::String("Hi".into())), None);
    assert_eq!(
        node.to_json(),
        "{\"enabled\": true, \"severity\": \"warning\", \"note\": \"Hi\"}\n"
    );
    assert_eq!(
        node.remove("severity"),
        Some(Node::String("warning".into()))
    );
    assert_eq!(node.get("severity"), None);
    assert_eq!(node.get("note").and_then(Node::as_str), Some("Hi"));
}

#[test]
fn what_fits_in_100_columns_stays_on_one_line() {
    // `"a"` makes the object too wide for one line. Then `"k"`'s line: indent 2, key `"k": ` (5),
    // and `["x…x"]` 93 wide: exactly 100.
    let k_line = |n: usize| {
        let text = Node::from(json!({ "a": "a".repeat(60), "k": ["x".repeat(n)] })).to_json();
        let line = text
            .lines()
            .find(|line| line.starts_with("  \"k\""))
            .unwrap();
        (line.to_owned(), text.lines().count())
    };
    let (line, lines) = k_line(89);
    assert_eq!(line.chars().count(), 100, "{line}");
    assert_eq!(lines, 4);

    // One more and the list opens up.
    let (line, lines) = k_line(90);
    assert_eq!(line, "  \"k\": [");
    assert_eq!(lines, 6);
}

#[test]
fn a_long_list_of_strings_is_wrapped_several_to_a_line() {
    let words: Vec<String> = (0..40).map(|i| format!("word{i:02}")).collect();
    let text = Node::from(json!({ "names": words })).to_json();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "  \"names\": [");
    let words = &lines[2..lines.len() - 2];
    for (i, line) in words.iter().enumerate() {
        assert!(line.starts_with("    \"word"), "{line}");
        assert!(line.chars().count() <= 100, "{line}");
        assert_eq!(line.ends_with(','), i + 1 < words.len(), "{line}");
    }
    assert_eq!(lines[lines.len() - 2], "  ]");
    assert!(lines.len() < 10, "{text}");
}

#[test]
fn text_is_written_as_utf8_not_escaped() {
    let node = Node::from(json!({ "note": "Scale it to 2048×2048 — or less." }));
    assert_eq!(
        node.to_json(),
        "{\"note\": \"Scale it to 2048×2048 — or less.\"}\n"
    );
}

#[test]
fn converting_to_and_from_serde_json_keeps_the_values() {
    let value = json!({ "a": [1, 2.5, true, null, "x"], "b": { "c": -3 } });
    let node = Node::from(value.clone());
    assert_eq!(serde_json::Value::from(&node), value);
    assert_eq!(Node::parse(&node.to_json()).unwrap(), node);
}
