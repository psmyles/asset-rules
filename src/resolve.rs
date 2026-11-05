//! Which rules apply to a file: its type, profile, stage and rules file (AC6-AC7).

use std::borrow::Cow;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{BranchStage, Profile, Rules, Skip};

/// What resolution needs to know besides the file's path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// The repository's top folder (`git rev-parse --show-toplevel`). Absolute paths given to
    /// [`resolve`] are made relative to it.
    pub repo_root: PathBuf,
    /// The branch the file is being committed to, or a pull request's target branch (AC18).
    /// `None` for a detached HEAD, which matches no branch rule.
    pub branch: Option<String>,
}

impl Context {
    /// `path` relative to the repository's top folder with `/` separators, as results and
    /// `paths` patterns use it, or `None` if it lies outside. Paths are taken as [`resolve`]
    /// takes them.
    pub fn repo_path(&self, path: &Path) -> Option<String> {
        repo_relative(&self.repo_root, path)
    }
}

/// Where a file's stage came from (AC7), in order of precedence. Written to results in
/// lowercase: `branch`, `profile`, `project`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageFrom {
    /// The asset itself. Reserved: the marking mechanism isn't decided, so v1 never returns it.
    Asset,
    /// The first of `branches[]` whose pattern matches [`Context::branch`].
    Branch,
    /// The profile's own `stage`.
    Profile,
    /// The project's `stage`.
    Project,
}

impl StageFrom {
    /// As results write it: `asset`, `branch`, `profile` or `project`.
    pub fn as_str(self) -> &'static str {
        match self {
            StageFrom::Asset => "asset",
            StageFrom::Branch => "branch",
            StageFrom::Profile => "profile",
            StageFrom::Project => "project",
        }
    }
}

/// Why a file of a known type isn't looked at (`rules.json` version 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkippedBy {
    /// Its repository path matches none of `check`.
    Check,
    /// It matches `skip[i]`.
    Skip(usize),
}

/// The answer to "which rules apply to this file?".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// No type has the file's extension, so Asset Check doesn't look at it.
    Ignored,
    /// The file has a type, but `check` or `skip` leaves it out: Asset Check doesn't look at it,
    /// and the CLIs don't report it.
    Skipped {
        /// The type, a key of [`Rules::types`].
        kind: String,
        by: SkippedBy,
    },
    /// The file has a type, but no profile of that type matches it. The CLI reports this as rule
    /// `file.no_profile`.
    Unmatched {
        /// The type, a key of [`Rules::types`].
        kind: String,
    },
    /// A profile matched.
    Resolved(Resolved),
}

/// The type, profile, stage and rules file that apply to a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The type, a key of [`Rules::types`].
    pub kind: String,
    pub profile_id: String,
    pub profile_label: String,
    pub stage: String,
    pub stage_from: StageFrom,
    /// The profile's rules file for `stage`, else its `"*"` file, joined to [`Rules::dir`].
    /// `None` only if it has neither, which `rules check` reports as
    /// [`MissingStageFile`](crate::ProblemKind::MissingStageFile).
    pub rules_file: Option<PathBuf>,
}

/// Which rules apply to the file at `path`.
///
/// `path` is relative to the repository's top folder, as git prints it, or absolute. A CLI given
/// a path relative to its current folder should pass [`std::path::absolute`] of it. A file
/// outside [`Context::repo_root`] has no repository path, so only profiles without `paths` can
/// match it.
///
/// 1. **Type:** the one whose `extensions` include the file's, compared case-insensitively.
/// 2. **Skipped?** Not if `check` is absent or one of its patterns matches the repository path,
///    and no `skip` entry matches ([`skipped_by`]).
/// 3. **Profile:** the first of that type, in file order, with a `names` pattern matching the
///    file name without its extension and, if it has `paths`, a pattern matching the
///    repository-relative path. Matching is case-sensitive.
/// 4. **Stage:** from the branch, else the profile, else the project ([`StageFrom`]).
/// 5. **Rules file:** the profile's file for that stage, else its `"*"` file.
pub fn resolve(rules: &Rules, ctx: &Context, path: &Path) -> Resolution {
    let file = FilePath::new(&ctx.repo_root, path);
    let Some(kind) = file_type(rules, &file) else {
        return Resolution::Ignored;
    };
    if let Some(by) = skipped_by(rules, &file) {
        return Resolution::Skipped {
            kind: kind.to_owned(),
            by,
        };
    }
    let Some(profile) = rules
        .profiles()
        .iter()
        .find(|profile| profile.kind == kind && profile_matches(profile, &file))
    else {
        return Resolution::Unmatched {
            kind: kind.to_owned(),
        };
    };
    let (stage, stage_from) = stage_of(rules, ctx, profile);
    Resolution::Resolved(Resolved {
        kind: kind.to_owned(),
        profile_id: profile.id.clone(),
        profile_label: profile.label.clone(),
        stage: stage.to_owned(),
        stage_from,
        rules_file: profile.rules_file(stage).map(|file| rules.file_path(file)),
    })
}

/// The parts of a file's path that resolution matches against.
#[derive(Clone, Debug)]
pub(crate) struct FilePath {
    /// Relative to the repository's top folder, with `/` separators. `None` outside it.
    pub(crate) relative: Option<String>,
    /// The file name without its extension.
    pub(crate) stem: String,
    /// Lowercased, without the dot.
    pub(crate) extension: Option<String>,
}

impl FilePath {
    pub(crate) fn new(repo_root: &Path, path: &Path) -> FilePath {
        FilePath {
            relative: repo_relative(repo_root, path),
            stem: path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            extension: path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase()),
        }
    }
}

/// `path` relative to `root` with `/` separators, or `None` if it lies outside `root`.
///
/// A relative `path` is taken as relative to `root` already. An absolute one is compared with
/// `root` as written first and, failing that, with both resolved through the file system, so
/// a symlinked or differently spelled root (macOS's `/var` and `/private/var`) still matches.
fn repo_relative(root: &Path, path: &Path) -> Option<String> {
    let relative: Cow<'_, Path> = if path.is_absolute() {
        match path.strip_prefix(root) {
            Ok(rest) => Cow::Borrowed(rest),
            Err(_) => {
                let root = fs::canonicalize(root).ok()?;
                let path = fs::canonicalize(path).ok()?;
                Cow::Owned(path.strip_prefix(&root).ok()?.to_path_buf())
            }
        }
    } else {
        Cow::Borrowed(path)
    };
    let mut parts: Vec<Cow<'_, str>> = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy()),
            Component::CurDir => {}
            // `..` past the top folder leaves the repository.
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.join("/"))
}

/// The type whose `extensions` include the file's. If two types claim it (`rules check` reports
/// that), the first by name wins.
pub(crate) fn file_type<'a>(rules: &'a Rules, file: &FilePath) -> Option<&'a str> {
    let extension = file.extension.as_deref()?;
    rules
        .types()
        .iter()
        .find(|(_, file_type)| file_type.extensions.iter().any(|e| e == extension))
        .map(|(name, _)| name.as_str())
}

/// Why `check` or `skip` leaves the file out, if they do.
///
/// `check` is matched against the repository path; a file outside the repository has none, so
/// `check` doesn't leave it out. A `skip` entry's `paths` can't match such a file either.
pub(crate) fn skipped_by(rules: &Rules, file: &FilePath) -> Option<SkippedBy> {
    if let (Some(check), Some(relative)) = (rules.check_paths(), &file.relative)
        && !check.iter().any(|pattern| pattern.is_match(relative))
    {
        return Some(SkippedBy::Check);
    }
    rules
        .skip()
        .iter()
        .position(|skip| skip_matches(skip, file))
        .map(SkippedBy::Skip)
}

/// Whether a `skip` entry matches: its `names` (if any) and its `paths` (if any). An entry with
/// neither matches nothing.
pub(crate) fn skip_matches(skip: &Skip, file: &FilePath) -> bool {
    if skip.names.is_none() && skip.paths.is_none() {
        return false;
    }
    let names = skip
        .names
        .as_ref()
        .is_none_or(|names| names.iter().any(|pattern| pattern.is_match(&file.stem)));
    let paths = match (&skip.paths, &file.relative) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(paths), Some(relative)) => paths.iter().any(|pattern| pattern.is_match(relative)),
    };
    names && paths
}

pub(crate) fn names_match(profile: &Profile, file: &FilePath) -> bool {
    profile
        .names
        .iter()
        .any(|pattern| pattern.is_match(&file.stem))
}

pub(crate) fn paths_match(profile: &Profile, file: &FilePath) -> bool {
    match (&profile.paths, &file.relative) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(paths), Some(relative)) => paths.iter().any(|pattern| pattern.is_match(relative)),
    }
}

fn profile_matches(profile: &Profile, file: &FilePath) -> bool {
    names_match(profile, file) && paths_match(profile, file)
}

/// The stage for a file of `profile`, and where it came from.
pub(crate) fn stage_of<'a>(
    rules: &'a Rules,
    ctx: &Context,
    profile: &'a Profile,
) -> (&'a str, StageFrom) {
    // The asset's own stage comes first (AC7), but is reserved: v1 never has one.
    if let Some((_, rule)) = branch_rule(rules, ctx) {
        return (&rule.stage, StageFrom::Branch);
    }
    if let Some(stage) = &profile.stage {
        return (stage, StageFrom::Profile);
    }
    (rules.stage(), StageFrom::Project)
}

/// The first of `branches[]` matching the context's branch, with its index. `None` for a detached
/// HEAD.
pub(crate) fn branch_rule<'a>(rules: &'a Rules, ctx: &Context) -> Option<(usize, &'a BranchStage)> {
    let branch = ctx.branch.as_deref()?;
    rules
        .branches()
        .iter()
        .enumerate()
        .find(|(_, rule)| rule.pattern.is_match(branch))
}
