//! The gate of Asset Check step 1 (plan item 9), end to end.
//!
//! A project repository with the fixture rules repo as a submodule and a file staged for each
//! case the gate lists, run through what each CLI will do: list the staged files, resolve each,
//! read its staged content, write results, and use the cache. The stand-in "check" records the
//! bytes it read, so the tests see which content was checked.
//!
//! | Gate case                              | Where                                          |
//! | -------------------------------------- | ---------------------------------------------- |
//! | first-match profiles                   | `T_Crate_N.png` is a normal map, not a texture |
//! | path narrowing                         | `SM_Rock` in Environment vs `SM_Crate` in Props |
//! | no match                               | `ComfyUI_00042_.png`                           |
//! | each stage source, in precedence order | `on_main`, `on_a_feature_branch`, `on_a_proto_branch` |
//! | a missing stage file and `"*"`         | `SK_Hero` and `SM_Crate` use their `"*"` file  |
//! | a detached HEAD                        | `on_a_detached_head`                           |
//! | an LFS file                            | `SM_Rock.fbx` is a partially staged LFS file   |
//! | a partially staged file                | `SM_Crate.fbx`                                 |

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use StageFrom::{Branch, Profile as FromProfile, Project as FromProject};
use asset_rules::cache::{Cache, CacheKey, digest_file};
use asset_rules::git::Repo;
use asset_rules::results::{
    FileResult, Finding, NO_PROFILE_RULE, Results, Severity, Status, TOO_OLD_RULE, ToolInfo,
};
use asset_rules::{Mode, Resolution, Rules, StageFrom, explain, resolve};
use common::repo::TestRepo;
use serde_json::json;

/// A checked file: its path, profile, stage, stage source, and what its check saw.
type Row = (
    String,
    Option<String>,
    Option<String>,
    Option<StageFrom>,
    String,
);

const OID: &str = "9f2b7e1c3a5d4f60718293a4b5c6d7e8f90123456789abcdef0123456789abcd";
const LFS_OBJECT: &str = "the rock's real FBX bytes";

/// The project: the fixture rules repo at `rules/`, a committed file, and the staged files.
struct Project {
    repo: TestRepo,
    /// Kept alive: the submodule was cloned from it.
    _rules_origin: TestRepo,
}

impl Project {
    fn new() -> Project {
        let rules_origin = TestRepo::new();
        copy_dir(&fixture_dir(), rules_origin.path());
        rules_origin.git(&["add", "."]);
        rules_origin.commit();

        let repo = TestRepo::new();
        repo.git(&[
            "submodule",
            "add",
            "-q",
            rules_origin.path().to_str().unwrap(),
            "rules",
        ]);
        repo.write("Assets/Art/T_Old_D.png", "committed before");
        repo.git(&["add", "."]);
        repo.commit();

        Project {
            repo,
            _rules_origin: rules_origin,
        }
    }

    /// Stages one file per gate case, as an artist would before committing.
    fn stage(&self) {
        let repo = &self.repo;
        repo.write("Assets/Art/T_Crate_N.png", "normal pixels");
        repo.write("Assets/Art/T_Crate_D.png", "diffuse pixels");
        repo.write("Assets/Art/ComfyUI_00042_.png", "generated pixels");
        repo.write("Assets/Characters/SK_Hero.fbx", "hero");
        repo.write("Assets/Scenes/Main.unity", "scene");
        repo.write("Assets/Art/Props/SM_Crate.fbx", "staged crate");
        // An LFS file: the index holds a pointer, the object is where git-lfs keeps it.
        let pointer = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{OID}\nsize {}\n",
            LFS_OBJECT.len()
        );
        repo.write("Assets/Art/Environment/SM_Rock.fbx", &pointer);
        repo.write(
            &format!(".git/lfs/objects/{}/{}/{OID}", &OID[0..2], &OID[2..4]),
            LFS_OBJECT,
        );
        repo.git(&["add", "Assets"]);
        // Both edited again after staging: the staged content is what must be checked.
        repo.write(
            "Assets/Art/Props/SM_Crate.fbx",
            "crate edited after staging",
        );
        repo.write(
            "Assets/Art/Environment/SM_Rock.fbx",
            "rock edited after staging",
        );
    }

    fn path(&self) -> &Path {
        self.repo.path()
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project")
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// One run of a CLI, and how many files came from the cache.
struct Run {
    results: Results,
    cached: usize,
}

/// What `fire-cli` or `3d-review-cli` will do for `--staged --rules rules/rules.json`.
fn run(dir: &Path, name: &str, version: &str) -> Run {
    let repo = Repo::discover(dir).unwrap();
    let rules = Rules::load(&repo.root().join("rules/rules.json")).unwrap();
    assert_eq!(rules.check(), [], "the fixture rules have no problems");
    let ctx = repo.context().unwrap();
    let rules_commit = Repo::discover(rules.dir()).unwrap().head().unwrap();
    let tool = ToolInfo {
        name: name.to_owned(),
        version: version.to_owned(),
    };
    let mut results = Results::new(tool.clone(), &rules, rules_commit, &ctx);
    results.findings.extend(rules.too_old(&tool));
    let cache = Cache::open(&repo);
    let mut cached = 0;

    for file in repo.staged().unwrap() {
        let ours = |kind: &str| rules.types()[kind].tool == name;
        match resolve(&rules, &ctx, Path::new(&file.path)) {
            Resolution::Ignored | Resolution::Skipped { .. } => {}
            Resolution::Unmatched { kind } if ours(&kind) => {
                results
                    .files
                    .push(FileResult::unmatched(&file.path, &kind, &rules));
            }
            Resolution::Resolved(resolved) if ours(&resolved.kind) => {
                let rules_file = resolved.rules_file.clone().unwrap();
                let key = CacheKey::new(
                    &file.path,
                    &file.blob,
                    &resolved,
                    &digest_file(&rules_file).unwrap(),
                    &tool,
                );
                if let Some(entry) = cache.get(&key) {
                    cached += 1;
                    results.files.push(entry);
                    continue;
                }
                let content = repo.content(&file).unwrap();
                let mut entry = FileResult::resolved(&file.path, &resolved);
                entry
                    .findings
                    .push(stand_in_check(content.path(), &rules_file));
                cache.put(&key, &entry).unwrap();
                results.files.push(entry);
            }
            // The other CLI's file.
            Resolution::Unmatched { .. } | Resolution::Resolved(_) => {}
        }
    }
    Run { results, cached }
}

/// Records what was read, and against which rules file.
fn stand_in_check(content: &Path, rules_file: &Path) -> Finding {
    let read = fs::read_to_string(content).unwrap();
    let mut finding = Finding::new(
        "test.read",
        Severity::Info,
        "Read",
        read.clone(),
        "So the tests see which content was checked.",
        "Nothing to do.",
    );
    let file_name = rules_file.file_name().unwrap().to_string_lossy();
    finding.measured = match json!({ "content": read, "rules_file": file_name }) {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    };
    finding
}

/// Each file's path, profile, stage, stage source, and the content and rules file its check saw.
fn summary(results: &Results) -> Vec<Row> {
    results
        .files
        .iter()
        .map(|file| {
            let seen = match file.findings.first() {
                Some(finding) if finding.rule == "test.read" => {
                    let measured = finding.measured.as_ref().unwrap();
                    format!(
                        "{} | {}",
                        measured["content"].as_str().unwrap(),
                        measured["rules_file"].as_str().unwrap()
                    )
                }
                Some(finding) => finding.rule.clone(),
                None => String::new(),
            };
            (
                file.path.clone(),
                file.profile.clone(),
                file.stage.clone(),
                file.stage_from,
                seen,
            )
        })
        .collect()
}

fn row(
    path: &str,
    profile: Option<&str>,
    stage: Option<&str>,
    from: Option<StageFrom>,
    seen: &str,
) -> Row {
    (
        path.to_owned(),
        profile.map(str::to_owned),
        stage.map(str::to_owned),
        from,
        seen.to_owned(),
    )
}

#[test]
fn on_main() {
    let project = Project::new();
    project.stage();

    let fire = run(project.path(), "fire-cli", "0.1.0");
    assert_eq!(
        summary(&fire.results),
        [
            row(
                "Assets/Art/ComfyUI_00042_.png",
                None,
                None,
                None,
                NO_PROFILE_RULE
            ),
            row(
                "Assets/Art/T_Crate_D.png",
                Some("texture"),
                Some("production"),
                Some(Branch),
                "diffuse pixels | texture.json"
            ),
            row(
                "Assets/Art/T_Crate_N.png",
                Some("normal-map"),
                Some("production"),
                Some(Branch),
                "normal pixels | normal.json"
            ),
        ]
    );
    assert_eq!(
        fire.results.exit_code(),
        1,
        "no profile matched ComfyUI_00042_"
    );

    let review = run(project.path(), "3d-review-cli", "0.9.0");
    assert_eq!(
        summary(&review.results),
        [
            row(
                "Assets/Art/Environment/SM_Rock.fbx",
                Some("environment-prop"),
                Some("production"),
                Some(Branch),
                &format!("{LFS_OBJECT} | environment.json")
            ),
            row(
                "Assets/Art/Props/SM_Crate.fbx",
                Some("prop"),
                Some("production"),
                Some(Branch),
                "staged crate | prop.json"
            ),
            row(
                "Assets/Characters/SK_Hero.fbx",
                Some("character-model"),
                Some("production"),
                Some(Branch),
                "hero | character.json"
            ),
        ]
    );
    assert_eq!(review.results.exit_code(), 0);
    assert!(review.results.findings.is_empty());

    // The run records the rules repo's commit and digest, the branch and the mode.
    let rules_head = project
        .repo
        .git(&["-C", "rules", "rev-parse", "--short", "HEAD"]);
    assert_eq!(
        review.results.rules.commit.as_deref(),
        Some(rules_head.trim())
    );
    let rules = Rules::load(&project.path().join("rules/rules.json")).unwrap();
    assert_eq!(review.results.rules.digest, rules.digest());
    assert_eq!(review.results.context.branch.as_deref(), Some("main"));
    assert_eq!(review.results.context.mode, Mode::Advise);

    // Written and read back by whatever shows the results.
    let path = project
        .path()
        .join(".git/asset-check/results-3d-review-cli.json");
    review.results.write(&path).unwrap();
    assert_eq!(Results::read(&path).unwrap(), review.results);
}

#[test]
fn a_second_run_reads_every_checked_file_from_the_cache() {
    let project = Project::new();
    project.stage();
    let first = run(project.path(), "3d-review-cli", "0.9.0");
    assert_eq!(first.cached, 0);
    let second = run(project.path(), "3d-review-cli", "0.9.0");
    assert_eq!(second.cached, 3);
    assert_eq!(second.results, first.results);
    // A new tool version checks everything again.
    let updated = run(project.path(), "3d-review-cli", "0.9.1");
    assert_eq!(updated.cached, 0);
}

#[test]
fn on_a_feature_branch_the_profile_then_the_project_stage_applies() {
    let project = Project::new();
    project.repo.git(&["checkout", "-q", "-b", "feature/rocks"]);
    project.stage();
    let review = run(project.path(), "3d-review-cli", "0.9.0");
    assert_eq!(
        summary(&review.results),
        [
            row(
                "Assets/Art/Environment/SM_Rock.fbx",
                Some("environment-prop"),
                Some("prototype"),
                Some(FromProject),
                &format!("{LFS_OBJECT} | environment-proto.json")
            ),
            row(
                "Assets/Art/Props/SM_Crate.fbx",
                Some("prop"),
                Some("prototype"),
                Some(FromProject),
                "staged crate | prop.json"
            ),
            row(
                "Assets/Characters/SK_Hero.fbx",
                Some("character-model"),
                Some("production"),
                Some(FromProfile),
                "hero | character.json"
            ),
        ]
    );
}

#[test]
fn on_a_proto_branch_the_branch_outranks_the_profile() {
    let project = Project::new();
    project.repo.git(&["checkout", "-q", "-b", "proto/hero"]);
    project.stage();
    let review = run(project.path(), "3d-review-cli", "0.9.0");
    let hero = &summary(&review.results)[2];
    assert_eq!(
        hero,
        &row(
            "Assets/Characters/SK_Hero.fbx",
            Some("character-model"),
            Some("prototype"),
            Some(Branch),
            "hero | character-proto.json"
        )
    );
}

#[test]
fn on_a_detached_head() {
    let project = Project::new();
    project.repo.git(&["checkout", "-q", "--detach"]);
    project.stage();
    let review = run(project.path(), "3d-review-cli", "0.9.0");
    assert_eq!(review.results.context.branch, None);
    let stages: Vec<(Option<String>, Option<StageFrom>)> = review
        .results
        .files
        .iter()
        .map(|file| (file.stage.clone(), file.stage_from))
        .collect();
    assert_eq!(
        stages,
        [
            (Some("prototype".to_owned()), Some(FromProject)),
            (Some("prototype".to_owned()), Some(FromProject)),
            (Some("production".to_owned()), Some(FromProfile)),
        ]
    );
    let ctx = Repo::discover(project.path()).unwrap().context().unwrap();
    let rules = Rules::load(&project.path().join("rules/rules.json")).unwrap();
    let text = explain(&rules, &ctx, Path::new("Assets/Characters/SK_Hero.fbx"));
    assert!(
        text.contains("HEAD is detached, so no branch rule applies."),
        "{text}"
    );
}

#[test]
fn only_added_and_changed_regular_files_are_checked() {
    let project = Project::new();
    project.stage();
    let staged: Vec<String> = Repo::discover(project.path())
        .unwrap()
        .staged()
        .unwrap()
        .into_iter()
        .map(|file| file.path)
        .collect();
    // Not the committed T_Old_D.png, nor the rules submodule.
    assert_eq!(
        staged,
        [
            "Assets/Art/ComfyUI_00042_.png",
            "Assets/Art/Environment/SM_Rock.fbx",
            "Assets/Art/Props/SM_Crate.fbx",
            "Assets/Art/T_Crate_D.png",
            "Assets/Art/T_Crate_N.png",
            "Assets/Characters/SK_Hero.fbx",
            "Assets/Scenes/Main.unity",
        ]
    );
}

#[test]
fn a_tool_too_old_for_the_rules_says_so_once() {
    let project = Project::new();
    project.stage();
    let review = run(project.path(), "3d-review-cli", "0.8.0");
    let rules: Vec<&str> = review
        .results
        .findings
        .iter()
        .map(|f| f.rule.as_str())
        .collect();
    assert_eq!(rules, [TOO_OLD_RULE]);
    assert_eq!(review.results.exit_code(), 1);
    assert!(
        review
            .results
            .files
            .iter()
            .all(|file| file.status == Status::Checked)
    );
}
