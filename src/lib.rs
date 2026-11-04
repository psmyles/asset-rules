//! The shared core of Asset Check's two checkers, `fire-cli` (images) and `3d-review-cli`
//! (models).
//!
//! It answers one question the same way for every caller: *which rules apply to this file?*
//! It also defines the results format both CLIs write.
//!
//! It does not contain the rules (they live in the rules repo, a separate git submodule) and
//! does not know what a rule means: it hands each CLI the path of the rules file that applies,
//! and the CLI parses that file and runs the checks.
