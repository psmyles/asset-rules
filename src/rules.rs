//! `rules.json` v1: the model and its loader (AC5-AC8).

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::Pattern;
use crate::check::{UnknownField, scan_unknown_fields};
use crate::digest::sha256;

/// The `rules.json` version this crate reads.
pub const RULES_VERSION: u64 = 1;

/// The key in a profile's `rules` for the file that applies to every stage without its own.
pub const ANY_STAGE: &str = "*";

/// The checkers a type can name as its `tool`.
pub const CHECKERS: &[&str] = &["fire-cli", "3d-review-cli"];

/// The tools `requires` can name: the checkers, and the Launcher, whose version setup checks
/// (AC16).
pub const TOOLS: &[&str] = &["fire-cli", "3d-review-cli", "launcher"];

/// Whether findings only advise the artist or can block a commit (AC2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Report only: the artist can always commit.
    Advise,
    /// Errors block the commit; warnings still allow it.
    Enforce,
}

/// One of `branches[]`: the stage for branches whose name matches `pattern` (AC7).
#[derive(Clone, Debug, Deserialize)]
pub struct BranchStage {
    /// Matched against the short branch name, e.g. `proto/crates`.
    pub pattern: Pattern,
    pub stage: String,
}

/// One of `types`: which checker handles files with these extensions.
#[derive(Clone, Debug, Deserialize)]
pub struct FileType {
    /// One of [`CHECKERS`].
    pub tool: String,
    /// Without the dot, lowercased on load: extensions are compared case-insensitively.
    pub extensions: Vec<String>,
}

/// One of `profiles[]`: a kind of asset, recognised by its name, with a rules file per stage.
#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    /// Unique among the profiles; results name the profile by it.
    pub id: String,
    /// The file type this profile is for: a key of `types`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The name the artist sees, e.g. "Prop texture".
    pub label: String,
    /// Matched against the file name without its extension. Any one matching is enough.
    pub names: Vec<Pattern>,
    /// Matched against the repository-relative path, with `/` separators, to narrow `names`. Any
    /// one matching is enough. `None` matches every path.
    pub paths: Option<Vec<Pattern>>,
    /// The profile's own stage, which outranks the project's (AC7).
    pub stage: Option<String>,
    /// Stage → rules file, relative to the folder `rules.json` is in. [`ANY_STAGE`] covers every
    /// stage without its own entry.
    pub rules: BTreeMap<String, String>,
}

impl Profile {
    /// The rules file for `stage`: its own entry, else the [`ANY_STAGE`] entry.
    pub fn rules_file(&self, stage: &str) -> Option<&str> {
        self.rules
            .get(stage)
            .or_else(|| self.rules.get(ANY_STAGE))
            .map(String::as_str)
    }
}

/// A loaded `rules.json`, with the digest of its bytes.
///
/// Loading checks only that the file can be used: that it is JSON of the right shape and version.
/// [`Rules::check`] finds the problems in what it says.
#[derive(Clone, Debug)]
pub struct Rules {
    path: PathBuf,
    digest: String,
    doc: Document,
    unknown_fields: Vec<UnknownField>,
}

/// The fields of `rules.json`, as written. `version` is checked before this is read.
#[derive(Clone, Debug, Deserialize)]
struct Document {
    mode: Mode,
    stages: Vec<String>,
    stage: String,
    #[serde(default)]
    branches: Vec<BranchStage>,
    #[serde(default)]
    requires: BTreeMap<String, String>,
    types: BTreeMap<String, FileType>,
    profiles: Vec<Profile>,
}

impl Rules {
    /// Reads and parses the `rules.json` at `path`.
    ///
    /// Fields this crate doesn't know are ignored, so a newer rules repo still loads; `check`
    /// warns about them.
    pub fn load(path: &Path) -> Result<Rules, RulesError> {
        let bytes = fs::read(path).map_err(|source| RulesError::Read {
            path: path.to_owned(),
            source,
        })?;
        let json = |source| RulesError::Json {
            path: path.to_owned(),
            source,
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(json)?;
        if value.is_object() {
            match value.get("version") {
                Some(version) if version.as_u64() == Some(RULES_VERSION) => {}
                found => {
                    return Err(RulesError::Version {
                        path: path.to_owned(),
                        found: found.map(Value::to_string),
                    });
                }
            }
        }
        let mut doc: Document = serde_json::from_slice(&bytes).map_err(json)?;
        for file_type in doc.types.values_mut() {
            for extension in &mut file_type.extensions {
                *extension = extension.to_lowercase();
            }
        }
        Ok(Rules {
            path: path.to_owned(),
            digest: sha256(&bytes),
            doc,
            unknown_fields: scan_unknown_fields(&value),
        })
    }

    /// The path it was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The folder `rules.json` is in, which profile rules files are relative to.
    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new(""))
    }

    /// Where a rules file named in a profile is: `file` joined to [`Rules::dir`].
    pub fn file_path(&self, file: &str) -> PathBuf {
        self.dir().join(file)
    }

    /// `sha256:<hex>` of the file's bytes.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn mode(&self) -> Mode {
        self.doc.mode
    }

    /// The stage names, in file order.
    pub fn stages(&self) -> &[String] {
        &self.doc.stages
    }

    /// The project's stage: the last source a file's stage comes from (AC7).
    pub fn stage(&self) -> &str {
        &self.doc.stage
    }

    /// The branch rules, in file order; the first match wins.
    pub fn branches(&self) -> &[BranchStage] {
        &self.doc.branches
    }

    /// Tool name → the minimum version `rules.json` asks for.
    pub fn requires(&self) -> &BTreeMap<String, String> {
        &self.doc.requires
    }

    /// Type name → its checker and extensions.
    pub fn types(&self) -> &BTreeMap<String, FileType> {
        &self.doc.types
    }

    /// The profiles, in file order; the first match wins.
    pub fn profiles(&self) -> &[Profile] {
        &self.doc.profiles
    }

    pub(crate) fn unknown_fields(&self) -> &[UnknownField] {
        &self.unknown_fields
    }
}

/// Why a `rules.json` couldn't be loaded.
#[derive(Debug)]
pub enum RulesError {
    /// The file couldn't be read.
    Read { path: PathBuf, source: io::Error },
    /// The file isn't JSON, or isn't shaped like `rules.json`: a required field is missing or a
    /// value has the wrong type.
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// The file's `version` is missing (`found` is `None`) or isn't [`RULES_VERSION`] (`found` is
    /// the value as written).
    Version {
        path: PathBuf,
        found: Option<String>,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::Read { path, source } => {
                write!(f, "can't read {}: {source}", path.display())
            }
            RulesError::Json { path, source } => {
                write!(f, "{} isn't valid: {source}", path.display())
            }
            RulesError::Version { path, found: None } => write!(
                f,
                "{} has no \"version\"; this tool reads version {RULES_VERSION}",
                path.display()
            ),
            RulesError::Version {
                path,
                found: Some(found),
            } => write!(
                f,
                "{} is version {found}, but this tool reads version {RULES_VERSION}; \
                 the tool may need updating",
                path.display()
            ),
        }
    }
}

impl Error for RulesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            RulesError::Read { source, .. } => Some(source),
            RulesError::Json { source, .. } => Some(source),
            RulesError::Version { .. } => None,
        }
    }
}
