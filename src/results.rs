//! Results v1: what both CLIs write with `--results` (AC10).
//!
//! One shape for both checkers, so the Launcher window, the Unity tool and CI read one format.
//! Every field is always written, as `null` when it has no value. Within a version fields are
//! only ever added; a change to what one means is a new version.
//!
//! It follows the sketch in `asset-check-plan.md` §3, plus a top-level `findings` for what
//! concerns the run rather than one file, such as a tool older than the rules require.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

use crate::file::write_replacing;
use crate::version::Version;
use crate::{Context, Mode, Pattern, Resolved, Rules, StageFrom};

/// The `schema` every results file names.
pub const SCHEMA: &str = "asset-check.results";

/// The results version this crate writes and reads.
pub const VERSION: u64 = 1;

/// The rule id of the finding for a file no profile matches (AC6).
pub const NO_PROFILE_RULE: &str = "file.no_profile";

/// The rule id of the finding for a tool older than `rules.json` requires.
pub const TOO_OLD_RULE: &str = "tool.too_old";

/// One CLI run's results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Results {
    /// The CLI that wrote them.
    pub tool: ToolInfo,
    /// The rules they were checked against.
    pub rules: RulesInfo,
    pub context: ContextInfo,
    /// Findings about the run rather than any one file, e.g. [`TOO_OLD_RULE`].
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// The files this CLI looked at. Files of other tools' types, and ignored files, aren't listed.
    pub files: Vec<FileResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    /// `fire-cli` or `review3d-cli`.
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesInfo {
    /// The rules repo's commit, if known.
    pub commit: Option<String>,
    /// [`Rules::digest`]: `sha256:<hex>` of `rules.json`.
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextInfo {
    /// The branch the stage came from; `null` for a detached HEAD.
    pub branch: Option<String>,
    pub mode: Mode,
}

/// What happened to one file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileResult {
    /// Repository-relative, with `/` separators.
    pub path: String,
    /// The file's type; `null` only if it wasn't resolved.
    #[serde(rename = "type")]
    pub kind: Option<String>,
    /// The matching profile's id; `null` if none matched.
    pub profile: Option<String>,
    pub profile_label: Option<String>,
    pub stage: Option<String>,
    pub stage_from: Option<StageFrom>,
    pub status: Status,
    /// Why the file couldn't be checked or was skipped; `null` when it was checked.
    pub problem: Option<String>,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

/// Whether a file was checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Checked; its findings are complete.
    Checked,
    /// It couldn't be checked (`problem` says why). Counts as a failure in [`Results::exit_code`].
    Unreadable,
    /// Not checked, on purpose (`problem` says why).
    Skipped,
}

/// How serious a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// A note: never opens the window or fails the run.
    Info,
    Warning,
    /// Blocks the commit in enforce mode.
    Error,
}

/// One thing a rule found in a file, with plain-language text for the artist (AC11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// The rule's id, e.g. `texture.max_size`.
    pub rule: String,
    pub severity: Severity,
    /// A few words, e.g. "Too large".
    pub title: String,
    /// What is wrong, with the measured value and the limit.
    pub what: String,
    /// The consequence in the game or the pipeline.
    pub why: String,
    /// The steps to fix it, in the artist's own tool.
    pub how: String,
    /// The technical artist's note on the rule.
    pub note: Option<String>,
    /// The figures measured, e.g. `{"width": 4096, "height": 4096}`.
    pub measured: Option<Map<String, Value>>,
    /// The limits they were held to, e.g. `{"max": 2048}`.
    pub limit: Option<Map<String, Value>>,
    /// The affected objects inside a model.
    #[serde(default)]
    pub objects: Vec<String>,
    /// Reserved for automatic fixes (AC2): always written as `null`.
    #[serde(default)]
    fix: Reserved,
}

impl Finding {
    /// A finding with no note, figures or objects.
    pub fn new(
        rule: impl Into<String>,
        severity: Severity,
        title: impl Into<String>,
        what: impl Into<String>,
        why: impl Into<String>,
        how: impl Into<String>,
    ) -> Finding {
        Finding {
            rule: rule.into(),
            severity,
            title: title.into(),
            what: what.into(),
            why: why.into(),
            how: how.into(),
            note: None,
            measured: None,
            limit: None,
            objects: Vec::new(),
            fix: Reserved,
        }
    }
}

/// A field kept for a later use: written as `null`, and whatever a reader finds there is ignored,
/// so a later writer can fill it without breaking readers of this version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Reserved;

impl Serialize for Reserved {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_none()
    }
}

impl<'de> Deserialize<'de> for Reserved {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Reserved, D::Error> {
        IgnoredAny::deserialize(deserializer)?;
        Ok(Reserved)
    }
}

impl FileResult {
    /// A checked file that `resolved` applies to, with no findings yet.
    pub fn resolved(path: impl Into<String>, resolved: &Resolved) -> FileResult {
        FileResult {
            path: path.into(),
            kind: Some(resolved.kind.clone()),
            profile: Some(resolved.profile_id.clone()),
            profile_label: Some(resolved.profile_label.clone()),
            stage: Some(resolved.stage.clone()),
            stage_from: Some(resolved.stage_from),
            status: Status::Checked,
            problem: None,
            findings: Vec::new(),
        }
    }

    /// A file of type `kind` that no profile matches, with its [`NO_PROFILE_RULE`] finding: a
    /// warning, or an error in enforce mode, whose "how" lists the name patterns of `kind`.
    pub fn unmatched(path: impl Into<String>, kind: &str, rules: &Rules) -> FileResult {
        let path = path.into();
        let name = Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        FileResult {
            kind: Some(kind.to_owned()),
            profile: None,
            profile_label: None,
            stage: None,
            stage_from: None,
            status: Status::Checked,
            problem: None,
            findings: vec![no_profile(&name, kind, rules)],
            path,
        }
    }
}

/// The finding for a file named `name` (without its extension) that no profile of `kind` matches.
fn no_profile(name: &str, kind: &str, rules: &Rules) -> Finding {
    let severity = match rules.mode() {
        Mode::Advise => Severity::Warning,
        Mode::Enforce => Severity::Error,
    };
    let profiles: Vec<_> = rules
        .profiles()
        .iter()
        .filter(|profile| profile.kind == kind)
        .collect();
    let choices: Vec<String> = profiles
        .iter()
        .map(|profile| {
            let names = join(&profile.names, " or ");
            match &profile.paths {
                None => format!("{names} ({})", profile.label),
                Some(paths) => format!("{names} in {} ({})", join(paths, " or "), profile.label),
            }
        })
        .collect();
    let how = if choices.is_empty() {
        format!("No {kind} types are set up in the rules yet. Ask your technical artist.")
    } else {
        format!(
            "Rename the file so its name, without the extension, matches one of these \
             (* stands for any text): {}.",
            choices.join(", ")
        )
    };
    let patterns: Vec<&str> = profiles
        .iter()
        .flat_map(|profile| profile.names.iter().map(Pattern::as_str))
        .collect();
    let mut finding = Finding::new(
        NO_PROFILE_RULE,
        severity,
        format!("Name doesn't match any {kind} type"),
        format!("\"{name}\" doesn't match the name of any {kind} type."),
        "Unity's import presets choose a file's settings by its name, so a file named outside \
         these patterns gets the wrong import settings without anyone noticing.",
        how,
    );
    finding.measured = object(json!({ "name": name }));
    finding.limit = object(json!({ "names": patterns }));
    finding
}

/// The finding for `tool` when it is older than `required`, at `severity`. `None` when it isn't,
/// or when either isn't a version.
pub(crate) fn too_old(tool: &ToolInfo, required: &str, severity: Severity) -> Option<Finding> {
    let have = Version::parse(&tool.version)?;
    if have >= Version::parse(required)? {
        return None;
    }
    let product = product(&tool.name);
    let mut finding = Finding::new(
        TOO_OLD_RULE,
        severity,
        format!("{product} is too old"),
        format!(
            "This is {} {}, but the rules need {required} or later.",
            tool.name, tool.version
        ),
        "An older version may not know every rule the technical artist has set, so files can be \
         checked against the wrong rules, or not at all.",
        format!("Update {product} to version {required} or later, then check again."),
    );
    finding.measured = object(json!({ "version": tool.version }));
    finding.limit = object(json!({ "min_version": required }));
    Some(finding)
}

/// What the artist installs to get `tool`: each CLI ships with its app (AC12, AC13).
fn product(tool: &str) -> &str {
    match tool {
        "fire-cli" => "Fire",
        "review3d-cli" => "3D Review",
        "launcher" => "Launcher",
        other => other,
    }
}

fn join(patterns: &[Pattern], separator: &str) -> String {
    let texts: Vec<&str> = patterns.iter().map(Pattern::as_str).collect();
    texts.join(separator)
}

fn object(value: Value) -> Option<Map<String, Value>> {
    match value {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// How results are written: `schema` and `version` first, then the fields of [`Results`].
#[derive(Serialize)]
struct Document<'a> {
    schema: &'static str,
    version: u64,
    #[serde(flatten)]
    results: &'a Results,
}

impl Results {
    /// Results for a run of `tool` against `rules`, with no files yet. `rules_commit` is the rules
    /// repo's commit, if known.
    pub fn new(
        tool: ToolInfo,
        rules: &Rules,
        rules_commit: Option<String>,
        ctx: &Context,
    ) -> Results {
        Results {
            tool,
            rules: RulesInfo {
                commit: rules_commit,
                digest: rules.digest().to_owned(),
            },
            context: ContextInfo {
                branch: ctx.branch.clone(),
                mode: rules.mode(),
            },
            findings: Vec::new(),
            files: Vec::new(),
        }
    }

    fn serialize(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(&Document {
            schema: SCHEMA,
            version: VERSION,
            results: self,
        })
    }

    /// The results as pretty-printed JSON.
    pub fn to_json(&self) -> String {
        // Every key is a string and every value plain data, so serializing can't fail.
        self.serialize().expect("results serialize to JSON")
    }

    /// Writes the results to `path`, creating its folder if needed. The file is replaced, never
    /// overwritten in place, so a reader sees either the old results or the new ones, never half.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        let json = self.serialize().map_err(io::Error::other)?;
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        write_replacing(path, json.as_bytes())
    }

    /// Reads results written by [`Results::write`]. Fields this version doesn't know are ignored.
    pub fn read(path: &Path) -> Result<Results, ResultsError> {
        let bytes = fs::read(path).map_err(|source| ResultsError::Read {
            path: path.to_owned(),
            source,
        })?;
        let json = |source| ResultsError::Json {
            path: path.to_owned(),
            source,
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(json)?;
        let schema = value.get("schema").and_then(Value::as_str);
        let version = value.get("version").and_then(Value::as_u64);
        if schema != Some(SCHEMA) || version != Some(VERSION) {
            return Err(ResultsError::Schema {
                path: path.to_owned(),
                schema: schema.map(str::to_owned),
                version,
            });
        }
        serde_json::from_slice(&bytes).map_err(json)
    }

    /// The process exit code for these results: 0 when no finding (the run's or a file's) is at
    /// warning level or worse and no file was unreadable, else 1. (2, a command line that
    /// couldn't be followed, is each CLI's own.)
    pub fn exit_code(&self) -> u8 {
        let serious = |findings: &[Finding]| {
            findings
                .iter()
                .any(|finding| finding.severity >= Severity::Warning)
        };
        let failed = serious(&self.findings)
            || self
                .files
                .iter()
                .any(|file| file.status == Status::Unreadable || serious(&file.findings));
        u8::from(failed)
    }
}

/// Why results couldn't be read.
#[derive(Debug)]
pub enum ResultsError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    /// Not JSON, or not shaped like results.
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// Not [`SCHEMA`] version [`VERSION`]: what the file says, if anything.
    Schema {
        path: PathBuf,
        schema: Option<String>,
        version: Option<u64>,
    },
}

impl fmt::Display for ResultsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResultsError::Read { path, source } => {
                write!(f, "can't read {}: {source}", path.display())
            }
            ResultsError::Json { path, source } => {
                write!(f, "{} isn't valid results: {source}", path.display())
            }
            ResultsError::Schema {
                path,
                schema,
                version,
            } => {
                let schema = schema.as_deref().unwrap_or("no schema");
                let version = version.map_or("no version".to_owned(), |v| format!("version {v}"));
                write!(
                    f,
                    "{} is {schema}, {version}; expected {SCHEMA}, version {VERSION}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ResultsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ResultsError::Read { source, .. } => Some(source),
            ResultsError::Json { source, .. } => Some(source),
            ResultsError::Schema { .. } => None,
        }
    }
}
