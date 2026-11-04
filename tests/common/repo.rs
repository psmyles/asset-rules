//! Temporary git repositories for the tests.

use std::path::{Path, PathBuf};
use std::process::Command;

use asset_rules::git::{Repo, StagedFile};

use super::{TempDir, temp_dir};

/// A repository on branch `main` in a temporary folder, with git's user and global settings kept
/// out of the commands the tests make.
pub struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    pub fn new() -> TestRepo {
        let dir = temp_dir();
        git_in(dir.path(), &["init", "-q", "-b", "main"]);
        TestRepo { dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        git_in(self.path(), args)
    }

    pub fn write(&self, relative: &str, contents: &str) -> PathBuf {
        self.dir.write(relative, contents)
    }

    pub fn commit(&self) {
        self.git(&["commit", "-q", "--no-verify", "-m", "test"]);
    }

    pub fn repo(&self) -> Repo {
        Repo::discover(self.path()).expect("discover the repository")
    }

    pub fn staged(&self) -> Vec<StagedFile> {
        self.repo().staged().expect("list staged files")
    }

    /// The id git gives `relative`'s working content.
    pub fn hash(&self, relative: &str) -> String {
        self.git(&["hash-object", relative]).trim().to_owned()
    }
}

/// Runs git in `dir` and returns its output, failing the test if it fails.
pub fn git_in(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join("no-global-config"))
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
