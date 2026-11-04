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
//! - [`Rules::load`] reads `rules.json`; [`Rules::check`] lists the problems in it, for each CLI's
//!   `rules check` command.
//! - [`resolve`] says which type, profile, stage and rules file apply to a file.
//! - [`results`] is the format both CLIs write with `--results`.
//! - [`git`] lists the staged files, the branch, and where to read each file's staged content.

mod check;
mod digest;
mod file;
pub mod git;
mod pattern;
mod resolve;
pub mod results;
mod rules;

pub use check::{Level, Problem, ProblemKind};
pub use pattern::Pattern;
pub use resolve::{Context, Resolution, Resolved, StageFrom, resolve};
pub use rules::{
    ANY_STAGE, BranchStage, CHECKERS, FileType, Mode, Profile, RULES_VERSION, Rules, RulesError,
    TOOLS,
};
