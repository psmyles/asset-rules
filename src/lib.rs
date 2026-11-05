//! The shared core of Asset Check's two checkers, `fire-cli` (images) and `3d-review-cli`
//! (models).
//!
//! It answers one question the same way for every caller: *which rules apply to this file?*
//! It also defines the results format both CLIs write.
//!
//! It does not contain the rules (they live in the rules repo, a separate git submodule) and
//! does not know what a rule means: it hands each CLI the path of the rules file that applies,
//! and the CLI parses that file and runs the checks.
//!
//! - [`Rules::load`] reads `rules.json`; [`Rules::check`] lists the problems in it, and
//!   [`Rules::check_rules_files`] those in a checker's rules files, read against its catalog, for
//!   each CLI's `rules check` command.
//! - [`resolve`] says which type, profile, stage and rules file apply to a file; [`explain`]
//!   says why, for each CLI's `rules explain` command.
//! - [`results`] is the format both CLIs write with `--results`.
//! - [`git`] lists the staged files, the branch, and where to read each file's staged content.
//! - [`cache`] keeps each file's results entry until something that decides it changes.
//! - [`catalog`] reads each checker's rule catalog, and checks rules files against it;
//!   [`codegen`] generates the code a checker's `build.rs` makes from its catalog.
//! - [`json`] writes `rules.json` and rules files in the one style every tool uses.

pub mod cache;
pub mod catalog;
mod check;
pub mod codegen;
mod digest;
mod explain;
mod file;
pub mod git;
pub mod json;
mod pattern;
mod resolve;
pub mod results;
mod rules;
mod version;

pub use check::{Level, Problem, ProblemKind};
pub use explain::explain;
pub use pattern::Pattern;
pub use resolve::{Context, Resolution, Resolved, SkippedBy, StageFrom, resolve};
pub use rules::{
    ANY_STAGE, BranchStage, CHECKERS, FileType, Mode, Profile, RULES_VERSION, Rules, RulesError,
    SKIP_VERSION, Skip, TOOLS,
};
