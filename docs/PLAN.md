# asset-rules - Work Plan

**Status, 5 November 2026:** `v0.2.0` (items 11-15) is tagged and used by both CLIs: the rule
catalogs, for Rule Editor (the design is the rules repo's `docs/rule-editor-plan.md`). `v0.1.0` was
items 1-10.

0.1 was step 1 of the Asset Check rollout. Where the build departed
from this plan (a top-level `findings` in results, a wider cache key, how staged content is read),
the commit for that item says why.

## What this crate is

Asset Check checks art assets (textures, models) against a technical artist's rules when they are
committed to a Unity project, and tells the artist what is wrong, why, and how to fix it. The design is
`docs/asset-check-plan.md` in the rules repo,
[SandboxVR/exp-content-rules](https://github.com/SandboxVR/exp-content-rules) (private). Decision
numbers below (AC1-AC19) refer to it.

`asset-rules` is the **shared Rust crate** both checkers use (AC9): `fire-cli` (images, in fire's repo)
and `review3d-cli` (models, in 3D Review). It answers one question the same way for every caller (the
git hook, the Fork command, the Unity tool, CI): *which rules apply to this file?* It also defines the
results format both CLIs write.

**It is not the rules repo.** The rules repo is a separate git submodule that only the technical artist
writes. It holds `rules.json` and the per-profile rules files. This crate reads that data; it doesn't
contain it.

**It does not know what a rule means.** It hands each CLI the path of the rules file that applies, and
the CLI parses that file and runs the checks.

## Queue

Do these in order. Each item says when it's done.

1. **Crate skeleton.**
   - A `Cargo.toml` with lib name `asset-rules`.
   - Edition 2024 and the latest stable Rust (at least 1.88).
   - Lints at 3D Review's level: `clippy::all` and `undocumented_unsafe_blocks` denied. No `unsafe` is
     expected.
   - A `README.md` saying what the crate is and isn't (as above).
   - Licence: the repo's `LICENSE` is MIT.
   - Keep dependencies few: `serde`, `serde_json`, `globset`, `sha2`.
   - *Done when* `cargo clippy --all-targets -- -D warnings` and `cargo test` pass on an empty crate.

2. **`rules.json` v1: model, loader, validation** (AC5-AC8; sketch in `asset-check-plan.md` §3).
   - **Fields:** `version`, `mode` (`advise`/`enforce`), `stages`, the project `stage`, `branches[]`
     (`pattern` → `stage`), `requires` (minimum tool versions), `types` (`tool` and `extensions` per
     type), and `profiles[]`. Each profile has `id`, `type`, `label`, `names[]`, an optional `paths[]`, an
     optional `stage`, and `rules` (a stage → file map; `"*"` is a catch-all).
   - **Validation, behind a `rules check` command each CLI exposes:**
     - unknown type, stage or tool;
     - a stage with no rules file and no `"*"`;
     - a rules file that doesn't exist;
     - an invalid glob;
     - a profile that can never match because an earlier one always wins (warn);
     - duplicate ids.
   - *Done when* each of those problems has a test.

3. **Resolution** (AC6-AC7). For a repository-relative path:
   1. **Type:** from the extension, compared case-insensitively. A file whose extension belongs to no
      type → `Ignored`.
   2. **Profile:** the first profile of that type, in file order, whose `names` match. `names` match the
      file name without its extension; `paths` (optional) match the repository-relative path with `/`
      separators and support `**`. **Matching is case-sensitive.**
   3. **No profile matches** → `Unmatched { type }`. The CLI reports it as rule `file.no_profile`, a
      warning, or an error in enforce mode. Its "how" text lists the expected name patterns.
   4. **Stage:** the first that applies, in this order:
      1. asset - **reserved, always `None` in v1**;
      2. branch (`branches[]` patterns against the current branch; none when HEAD is detached);
      3. profile `stage`;
      4. project `stage`.

      Record which one applied (`stage_from`).
   5. **Rules file:** the profile's file for that stage, else its `"*"` file.

   Returns `Resolved { type, profile_id, profile_label, stage, stage_from, rules_file }`.

   *Done when* the fixtures in item 9 pass.

4. **Results v1** (AC10; sketch in `asset-check-plan.md` §3).
   - **Format:** schema `asset-check.results`, version 1.
   - **Per file:** `path`, `type`, `profile`, `profile_label`, `stage`, `stage_from`, `status`
     (`checked`/`unreadable`/`skipped`), `problem`, `findings[]`.
   - **Per finding:** `rule`, `severity` (`error`/`warning`/`info`), `title`, `what`, `why`, `how`,
     `note`, `measured`, `limit`, `objects[]`, and `fix`, which is always `null` until fixes ship.
   - **Writing:** atomic (temporary file, then rename), the way review3d-cli writes its `--report`.
   - **An exit-code helper:** 0 when nothing is at warning level or worse; 1 when something was found or
     a file couldn't be checked. 2 (usage error) stays each CLI's own.
   - *Done when* a round-trip test passes and the JSON matches the sketch.

5. **Git helpers** (AC15). Shell out to `git`, which is on PATH in hooks, Fork, Unity and CI. No libgit2.
   - **Repository root:** `git rev-parse --show-toplevel`.
   - **The git directory, for LFS objects and the cache:** `git rev-parse --git-common-dir`. `.git` can
     be a file in worktrees and submodules, so never assume a folder.
   - **Staged files:** `git diff --cached --name-only -z --diff-filter=ACMR`.
   - **Staged blob ids:** `git ls-files -s -z -- <paths>`.
   - **Partially staged files:** staged files that also appear in `git diff --name-only -z`.
   - **Branch:** `git symbolic-ref --short -q HEAD`. Empty means detached.
   - **Staged content of a partially staged file:**
     - if the blob is a Git LFS pointer (`version https://git-lfs.github.com/spec/v1`,
       `oid sha256:<hex>`), the object is at `<common-dir>/lfs/objects/<hex[0..2]>/<hex[2..4]>/<hex>`;
     - otherwise `git cat-file blob <id>`, written to a temporary file.

     A file with no unstaged changes is checked in place.
   - *Done when* tests pass against temporary repositories. The LFS case can be simulated with a pointer
     blob and an object file, without git-lfs installed.

6. **Cache** (AC15).
   - **Location:** `<common-dir>/asset-check/cache/`.
   - **Key:** a hash of the staged blob id, the digest of the rules file that applied, the tool's name and
     version, and the stage.
   - **Value:** that file's results entry.
   - **API:** `get` / `put`, plus pruning of entries not used for 30 days.
   - *Done when* a second check of an unchanged file reads from the cache, and changing any one key part
     misses.

7. **Tool versions.** `requires` against a CLI's own version. A tool that's too old produces one finding
   whose "how" text says what to update.

8. **`rules explain <file>`.** Human-readable output for the technical artist: which type, profile,
   stage (and where it came from) and rules file apply to the file, and why. If nothing matches, it says
   why: which patterns were tried.

9. **Fixtures** (the gate of Asset Check step 1):
   - first-match profiles;
   - path narrowing;
   - no match;
   - each stage source, with precedence branch > profile > project;
   - a missing stage file and the `"*"` fallback;
   - a detached HEAD;
   - an LFS file;
   - a partially staged file.

10. **Tag `v0.1.0`** once item 9 passes. review3d-cli and fire-cli then depend on it with
    `git = "https://github.com/psmyles/asset-rules", tag = "v0.1.0"`.

**0.2: rule catalogs and Rule Editor.**

11. **`rules.json` version 2: `check` and `skip`.** **Done.** Files a version-2 file leaves out
    resolve to `Resolution::Skipped`, which the CLIs don't report; `rules explain` says why.
    Version 1 still loads; `check` or `skip` in it is an error.
12. **The one JSON style** (`json::Node`, `Node::to_json`). **Done.** Every tool that writes
    `rules.json` or a rules file writes the same bytes; exp-content-rules' 15 files come back byte for
    byte.
13. **Rule catalogs** (`catalog`): reading `rules-catalog.toml` and what `rules catalog` prints,
    every check the format asks for, defaults and texts per base and case, placeholders, lenient
    settings for the checkers and a strict check of a rules file for `rules check` and Rule Editor.
    **Done.** Both CLIs' draft catalogs (copied into `tests/fixtures/catalogs/`) have no problems,
    and every rules file in exp-content-rules checks clean against them.
14. **What the ports need.**
    - `Rule::settings` reads an entry as both CLIs do: a listed rule is on unless it says
      `"enabled": false`, and an entry with a field of the wrong type takes the defaults whole.
    - `Rules::check_rules_files`: a CLI's rules files read against its catalog, for `rules check`.
    - `codegen`: what a CLI's `build.rs` generates from its catalog (`RuleId`, typed parameters,
      `Found` with each rule's measures and cases). The code for both draft catalogs is kept in
      `tests/fixtures/codegen/`, so the tests compile it.
    - A whole number written `6.0` is a count (3D Review's profile writer writes counts that way).
    **Done:** fire-cli and review3d-cli are both ported (their `catalog` branches) and build
    against this branch.
15. **Tag `v0.2.0`:** set `version = "0.2.0"` in `Cargo.toml`, push, tag; then both CLIs depend on
    the tag and commit their `Cargo.lock`. **Done** (5 November 2026).

## API sketch

Illustrative only; names settle while building.

```rust
pub struct Rules { /* parsed rules.json + its digest */ }
impl Rules {
    pub fn load(path: &Path) -> Result<Rules, RulesError>;
    pub fn check(&self) -> Vec<Problem>;                       // `rules check`
}

pub struct Context { pub repo_root: PathBuf, pub branch: Option<String> }

pub enum Resolution {
    Ignored,
    Unmatched { kind: String },
    Resolved(Resolved),  // type, profile_id, profile_label, stage, stage_from, rules_file
}
pub fn resolve(rules: &Rules, ctx: &Context, path: &Path) -> Resolution;
pub fn explain(rules: &Rules, ctx: &Context, path: &Path) -> String;

pub mod git {
    pub struct StagedFile { pub path: PathBuf, pub blob: String, pub partially_staged: bool }
    pub fn staged(repo: &Path) -> io::Result<Vec<StagedFile>>;
    pub fn content(repo: &Path, file: &StagedFile) -> io::Result<CheckedPath>;  // in place or temp copy
    pub fn branch(repo: &Path) -> Option<String>;
}

pub mod results { /* Results, FileResult, Finding, Severity, write_atomic, exit_code */ }
pub struct Cache { /* get / put / prune */ }
```

## Working on the Mac

- Pure Rust plus the `git` command, which comes with Xcode's command-line tools.
- To develop a CLI against unreleased changes, patch it from a `.cargo/config.toml` in the folder
  that holds the checkouts, not the CLI repository's own, which fire and 3D Review both commit. See
  the README.

  ```toml
  # <folder holding asset-rules, fire and 3d-review-rs>/.cargo/config.toml
  [patch."https://github.com/psmyles/asset-rules"]
  asset-rules = { path = "asset-rules" }
  ```

## References

- `docs/asset-check-plan.md` in SandboxVR/exp-content-rules: the whole design. AC5-AC10 and AC15 are
  this crate.
- 3D Review: `docs/cli.md`, the existing exit-code and `--report` conventions to match.
- `docs/PLAN.md` in 3D Review and fire: the CLI work that consumes this crate.
