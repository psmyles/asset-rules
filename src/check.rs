//! `rules check`: the problems in a `rules.json` that loaded (AC8).

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{Map, Value};

use crate::rules::{ANY_STAGE, CHECKERS, Profile, Rules, TOOLS};
use crate::version::Version;

/// How serious a [`Problem`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Probably a mistake, but every file still gets an answer.
    Warning,
    /// Some file gets the wrong answer, or none.
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Level::Warning => "warning",
            Level::Error => "error",
        })
    }
}

/// What kind of problem `rules check` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProblemKind {
    /// A field this crate doesn't know. It is ignored.
    UnknownField,
    /// A profile's `type` that isn't a key of `types`.
    UnknownType,
    /// A stage that isn't in `stages`: the project's, a branch rule's, a profile's, or a key of a
    /// profile's `rules`.
    UnknownStage,
    /// A type's `tool` that isn't one of [`CHECKERS`], or a key of `requires` that isn't one of
    /// [`TOOLS`].
    UnknownTool,
    /// A `requires` value that isn't a version.
    InvalidVersion,
    /// A stage listed more than once in `stages`.
    DuplicateStage,
    /// An extension that is empty or contains a dot.
    InvalidExtension,
    /// An extension listed in more than one type.
    DuplicateExtension,
    /// Two profiles with the same id.
    DuplicateId,
    /// A pattern that isn't a valid glob.
    InvalidGlob,
    /// A path pattern containing `\`, which never matches a `/` separator.
    Backslash,
    /// A profile with no rules file for a stage, and no [`ANY_STAGE`] file.
    MissingStageFile,
    /// A rules file that doesn't exist.
    RulesFileNotFound,
    /// A profile that can never match: it has nothing to match with, or an earlier profile of the
    /// same type matches every file it would.
    NeverMatches,
}

impl ProblemKind {
    pub fn level(self) -> Level {
        match self {
            ProblemKind::UnknownField | ProblemKind::Backslash | ProblemKind::NeverMatches => {
                Level::Warning
            }
            _ => Level::Error,
        }
    }
}

/// One problem `rules check` found.
///
/// Displays as `<level>: <at>: <message>`, e.g.
/// `error: profiles[2] "normal-map": type "textures" is not one of the types in "types" (model, texture)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub kind: ProblemKind,
    /// Where in `rules.json`: `stage`, `branches[1]`, `types "texture"`, `profiles[2] "normal-map"`, ...
    pub at: String,
    /// What is wrong, for the technical artist.
    pub message: String,
}

impl Problem {
    pub fn level(&self) -> Level {
        self.kind.level()
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.level(), self.at, self.message)
    }
}

impl Rules {
    /// Every problem in the configuration, in file order. Rules files are looked for on disk now,
    /// relative to [`Rules::dir`].
    pub fn check(&self) -> Vec<Problem> {
        let mut checker = Checker {
            rules: self,
            out: Vec::new(),
        };
        checker.unknown_fields();
        checker.stages();
        checker.branches();
        checker.requires();
        checker.types();
        checker.profiles();
        checker.out
    }
}

struct Checker<'a> {
    rules: &'a Rules,
    out: Vec<Problem>,
}

impl Checker<'_> {
    fn push(&mut self, kind: ProblemKind, at: &str, message: String) {
        self.out.push(Problem {
            kind,
            at: at.to_owned(),
            message,
        });
    }

    /// Reports `stage` if it isn't in `stages`. `lead` says where it was named.
    fn stage_ref(&mut self, at: &str, lead: &str, stage: &str) {
        if !self.rules.stages().iter().any(|known| known == stage) {
            let known = list(self.rules.stages().iter().map(String::as_str));
            self.push(
                ProblemKind::UnknownStage,
                at,
                format!(
                    "{lead}\"{stage}\", which is not one of the stages in \"stages\" ({known})"
                ),
            );
        }
    }

    fn unknown_fields(&mut self) {
        for field in self.rules.unknown_fields() {
            let message = format!(
                "unknown field \"{}\" is ignored; the fields here are {}",
                field.key,
                list(field.expected.iter().copied())
            );
            self.push(ProblemKind::UnknownField, &field.at, message);
        }
    }

    fn stages(&mut self) {
        let stages = self.rules.stages();
        for (i, stage) in stages.iter().enumerate() {
            if stages[..i].contains(stage) {
                self.push(
                    ProblemKind::DuplicateStage,
                    "stages",
                    format!("stage \"{stage}\" is listed more than once"),
                );
            }
        }
        self.stage_ref("stage", "the project's stage is ", self.rules.stage());
    }

    fn branches(&mut self) {
        for (i, branch) in self.rules.branches().iter().enumerate() {
            let at = format!("branches[{i}]");
            if let Some(error) = branch.pattern.error() {
                let message = format!(
                    "pattern \"{}\" is not a valid glob: {error}",
                    branch.pattern.as_str()
                );
                self.push(ProblemKind::InvalidGlob, &at, message);
            }
            self.stage_ref(&at, "the stage is ", &branch.stage);
        }
    }

    fn requires(&mut self) {
        for (tool, version) in self.rules.requires() {
            let at = format!("requires \"{tool}\"");
            if !TOOLS.contains(&tool.as_str()) {
                let message = format!(
                    "\"{tool}\" is not a known tool ({})",
                    list(TOOLS.iter().copied())
                );
                self.push(ProblemKind::UnknownTool, &at, message);
            }
            if Version::parse(version).is_none() {
                let message = format!("\"{version}\" isn't a version like 0.9.0");
                self.push(ProblemKind::InvalidVersion, &at, message);
            }
        }
    }

    fn types(&mut self) {
        let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, file_type) in self.rules.types() {
            let at = format!("types \"{name}\"");
            if !CHECKERS.contains(&file_type.tool.as_str()) {
                let message = format!(
                    "tool \"{}\" is not a known checker ({})",
                    file_type.tool,
                    list(CHECKERS.iter().copied())
                );
                self.push(ProblemKind::UnknownTool, &at, message);
            }
            for extension in &file_type.extensions {
                if extension.is_empty() {
                    let message = "an extension is empty".to_owned();
                    self.push(ProblemKind::InvalidExtension, &at, message);
                } else if let Some(bare) = extension.strip_prefix('.')
                    && !bare.contains('.')
                {
                    let message =
                        format!("extension \"{extension}\" starts with a dot; write \"{bare}\"");
                    self.push(ProblemKind::InvalidExtension, &at, message);
                } else if extension.contains('.') {
                    let message = format!(
                        "extension \"{extension}\" contains a dot, but a file's extension is only \
                         the part after its last dot"
                    );
                    self.push(ProblemKind::InvalidExtension, &at, message);
                } else {
                    match owners.get(extension.as_str()) {
                        Some(&owner) if owner != name => {
                            let message = format!(
                                "extension \"{extension}\" is also in type \"{owner}\"; each \
                                 extension can belong to only one type"
                            );
                            self.push(ProblemKind::DuplicateExtension, &at, message);
                        }
                        Some(_) => {}
                        None => {
                            owners.insert(extension, name);
                        }
                    }
                }
            }
        }
    }

    fn profiles(&mut self) {
        let rules = self.rules;
        let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
        for (index, profile) in rules.profiles().iter().enumerate() {
            let at = profile_at(index, Some(&profile.id));

            if let Some(&first) = ids.get(profile.id.as_str()) {
                let message = format!(
                    "id \"{}\" is also used by profiles[{first}]; each profile needs its own id",
                    profile.id
                );
                self.push(ProblemKind::DuplicateId, &at, message);
            } else {
                ids.insert(&profile.id, index);
            }

            if !rules.types().contains_key(&profile.kind) {
                let message = format!(
                    "type \"{}\" is not one of the types in \"types\" ({})",
                    profile.kind,
                    list(rules.types().keys().map(String::as_str))
                );
                self.push(ProblemKind::UnknownType, &at, message);
            }

            if let Some(stage) = &profile.stage {
                self.stage_ref(&at, "the profile's stage is ", stage);
            }
            for stage in profile.rules.keys().filter(|key| *key != ANY_STAGE) {
                self.stage_ref(&at, "\"rules\" has a file for stage ", stage);
            }
            self.stage_files(&at, profile);

            for pattern in &profile.names {
                if let Some(error) = pattern.error() {
                    let message = format!(
                        "name pattern \"{}\" is not a valid glob: {error}",
                        pattern.as_str()
                    );
                    self.push(ProblemKind::InvalidGlob, &at, message);
                }
            }
            for pattern in profile.paths.iter().flatten() {
                if let Some(error) = pattern.error() {
                    let message = format!(
                        "path pattern \"{}\" is not a valid glob: {error}",
                        pattern.as_str()
                    );
                    self.push(ProblemKind::InvalidGlob, &at, message);
                }
                if pattern.as_str().contains('\\') {
                    let message = format!(
                        "path pattern \"{}\" contains \"\\\", but paths are matched with \"/\" \
                         separators; write \"{}\"",
                        pattern.as_str(),
                        pattern.as_str().replace('\\', "/")
                    );
                    self.push(ProblemKind::Backslash, &at, message);
                }
            }

            self.never_matches(&at, index, profile);
        }
    }

    /// Every stage needs a rules file, and every rules file named must exist.
    fn stage_files(&mut self, at: &str, profile: &Profile) {
        let rules = self.rules;
        if !profile.rules.contains_key(ANY_STAGE) {
            let mut missing: Vec<&str> = Vec::new();
            for stage in rules.stages() {
                if !profile.rules.contains_key(stage) && !missing.contains(&stage.as_str()) {
                    missing.push(stage);
                }
            }
            if !missing.is_empty() {
                let which = match missing.as_slice() {
                    [one] => format!("stage \"{one}\""),
                    many => format!("stages {}", quoted_list(many)),
                };
                let message = format!("no rules file for {which}, and no \"{ANY_STAGE}\" file");
                self.push(ProblemKind::MissingStageFile, at, message);
            }
        }
        for (stage, file) in &profile.rules {
            let path = rules.file_path(file);
            if !path.is_file() {
                let message = format!(
                    "rules file \"{file}\" (for \"{stage}\") doesn't exist: looked for {}",
                    path.display()
                );
                self.push(ProblemKind::RulesFileNotFound, at, message);
            }
        }
    }

    fn never_matches(&mut self, at: &str, index: usize, profile: &Profile) {
        if profile.names.is_empty() {
            let message = "has no name patterns, so it can never match".to_owned();
            self.push(ProblemKind::NeverMatches, at, message);
            return;
        }
        if profile.paths.as_ref().is_some_and(Vec::is_empty) {
            let message = "has an empty \"paths\" list, so it can never match; leave \"paths\" \
                           out to match every path"
                .to_owned();
            self.push(ProblemKind::NeverMatches, at, message);
            return;
        }
        let earlier = self.rules.profiles()[..index]
            .iter()
            .enumerate()
            .find(|(_, earlier)| earlier.kind == profile.kind && covers(earlier, profile));
        if let Some((first, earlier)) = earlier {
            let message = format!(
                "can never match: {} comes first and matches every file this profile would. \
                 Move this profile above it if this one should win",
                profile_at(first, Some(&earlier.id))
            );
            self.push(ProblemKind::NeverMatches, at, message);
        }
    }
}

/// Whether profile `p` provably matches every file profile `q` matches.
fn covers(p: &Profile, q: &Profile) -> bool {
    let names = q
        .names
        .iter()
        .all(|qn| p.names.iter().any(|pn| pn.covers_name(qn)));
    let paths = match (&p.paths, &q.paths) {
        (None, _) => true,
        (Some(ps), None) => ps.iter().any(|pp| pp.as_str() == "**"),
        (Some(ps), Some(qs)) => qs.iter().all(|qp| ps.iter().any(|pp| pp.covers_path(qp))),
    };
    names && paths
}

/// `profiles[2] "normal-map"`, or `profiles[2]` without an id.
fn profile_at(index: usize, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("profiles[{index}] \"{id}\""),
        None => format!("profiles[{index}]"),
    }
}

/// `a, b, c`, or `none` for nothing.
fn list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let items: Vec<&str> = items.into_iter().collect();
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

/// `"a", "b", "c"`.
fn quoted_list(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|item| format!("\"{item}\"")).collect();
    quoted.join(", ")
}

/// A field of `rules.json` this crate doesn't read, found on load.
#[derive(Clone, Debug)]
pub(crate) struct UnknownField {
    pub(crate) at: String,
    pub(crate) key: String,
    /// The fields that object can have.
    pub(crate) expected: &'static [&'static str],
}

const TOP_FIELDS: &[&str] = &[
    "version", "mode", "stages", "stage", "branches", "requires", "types", "profiles",
];
const BRANCH_FIELDS: &[&str] = &["pattern", "stage"];
const TYPE_FIELDS: &[&str] = &["tool", "extensions"];
const PROFILE_FIELDS: &[&str] = &["id", "type", "label", "names", "paths", "stage", "rules"];

/// Every field in `root` (a parsed `rules.json`) that this crate doesn't read.
pub(crate) fn scan_unknown_fields(root: &Value) -> Vec<UnknownField> {
    let mut out = Vec::new();
    let Some(root) = root.as_object() else {
        return out;
    };
    scan(root, "top level", TOP_FIELDS, &mut out);
    for (i, branch) in objects(root, "branches") {
        scan(branch, &format!("branches[{i}]"), BRANCH_FIELDS, &mut out);
    }
    if let Some(types) = root.get("types").and_then(Value::as_object) {
        for (name, file_type) in types {
            if let Some(file_type) = file_type.as_object() {
                scan(
                    file_type,
                    &format!("types \"{name}\""),
                    TYPE_FIELDS,
                    &mut out,
                );
            }
        }
    }
    for (i, profile) in objects(root, "profiles") {
        let at = profile_at(i, profile.get("id").and_then(Value::as_str));
        scan(profile, &at, PROFILE_FIELDS, &mut out);
    }
    out
}

fn scan(
    object: &Map<String, Value>,
    at: &str,
    expected: &'static [&'static str],
    out: &mut Vec<UnknownField>,
) {
    for key in object.keys() {
        if !expected.contains(&key.as_str()) {
            out.push(UnknownField {
                at: at.to_owned(),
                key: key.clone(),
                expected,
            });
        }
    }
}

/// The objects in the array `root[key]`, with their indexes.
fn objects<'a>(
    root: &'a Map<String, Value>,
    key: &str,
) -> impl Iterator<Item = (usize, &'a Map<String, Value>)> {
    root.get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, value)| value.as_object().map(|object| (i, object)))
}
