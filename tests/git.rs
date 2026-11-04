//! Git helpers, against temporary repositories (plan item 5).
//!
//! Git LFS is simulated without git-lfs: a pointer blob is staged and its object written where
//! git-lfs would keep it.

mod common;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use asset_rules::git::{Repo, StagedFile};
use common::{TempDir, temp_dir};

/// A repository on branch `main` in a temporary folder, with git's user and global settings kept
/// out of the commands the tests make.
struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    fn new() -> TestRepo {
        let dir = temp_dir();
        git_in(dir.path(), &["init", "-q", "-b", "main"]);
        TestRepo { dir }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        git_in(self.path(), args)
    }

    fn write(&self, relative: &str, contents: &str) -> PathBuf {
        self.dir.write(relative, contents)
    }

    fn commit(&self) {
        self.git(&["commit", "-q", "--no-verify", "-m", "test"]);
    }

    fn repo(&self) -> Repo {
        Repo::discover(self.path()).expect("discover the repository")
    }

    fn staged(&self) -> Vec<StagedFile> {
        self.repo().staged().expect("list staged files")
    }

    /// The id git gives `relative`'s working content.
    fn hash(&self, relative: &str) -> String {
        self.git(&["hash-object", relative]).trim().to_owned()
    }
}

/// Runs git in `dir` and returns its output, failing the test if it fails.
fn git_in(dir: &Path, args: &[&str]) -> String {
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

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

/// A Git LFS pointer to an object with id `oid`.
fn pointer(oid: &str, size: usize) -> String {
    format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {size}\n")
}

const OID: &str = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";

// --- The repository --------------------------------------------------------------------------

#[test]
fn discovers_the_root_and_git_folder_from_anywhere_inside() {
    let test = TestRepo::new();
    test.write("Assets/Art/readme.txt", "");
    for dir in [test.path().to_owned(), test.path().join("Assets/Art")] {
        let repo = Repo::discover(&dir).unwrap();
        assert_eq!(canonical(repo.root()), canonical(test.path()));
        assert_eq!(
            canonical(repo.common_dir()),
            canonical(&test.path().join(".git"))
        );
    }
}

#[test]
fn a_worktree_shares_the_main_repositorys_git_folder() {
    // In a worktree `.git` is a file, not a folder.
    let test = TestRepo::new();
    test.write("a.txt", "a");
    test.git(&["add", "a.txt"]);
    test.commit();
    let worktree = test.path().join("worktree");
    test.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "side",
        worktree.to_str().unwrap(),
    ]);
    assert!(worktree.join(".git").is_file());

    let repo = Repo::discover(&worktree).unwrap();
    assert_eq!(canonical(repo.root()), canonical(&worktree));
    assert_eq!(
        canonical(repo.common_dir()),
        canonical(&test.path().join(".git"))
    );
    assert_eq!(repo.branch().unwrap().as_deref(), Some("side"));
}

#[test]
fn a_folder_outside_any_repository_is_an_error() {
    let dir = temp_dir();
    let error = Repo::discover(dir.path()).unwrap_err();
    assert!(error.to_string().contains("rev-parse"), "{error}");
}

// --- Branch and commit -----------------------------------------------------------------------

#[test]
fn the_branch_is_known_before_the_first_commit() {
    let test = TestRepo::new();
    assert_eq!(test.repo().branch().unwrap().as_deref(), Some("main"));
    assert_eq!(test.repo().head().unwrap(), None);
}

#[test]
fn branch_names_keep_their_slashes() {
    let test = TestRepo::new();
    test.git(&["checkout", "-q", "-b", "proto/crates"]);
    assert_eq!(
        test.repo().branch().unwrap().as_deref(),
        Some("proto/crates")
    );
}

#[test]
fn a_detached_head_has_no_branch() {
    let test = TestRepo::new();
    test.write("a.txt", "a");
    test.git(&["add", "a.txt"]);
    test.commit();
    test.git(&["checkout", "-q", "--detach"]);
    let repo = test.repo();
    assert_eq!(repo.branch().unwrap(), None);
    let context = repo.context().unwrap();
    assert_eq!(context.branch, None);
    assert_eq!(canonical(&context.repo_root), canonical(test.path()));
}

#[test]
fn head_is_the_abbreviated_commit() {
    let test = TestRepo::new();
    test.write("a.txt", "a");
    test.git(&["add", "a.txt"]);
    test.commit();
    let full = test.git(&["rev-parse", "HEAD"]);
    let head = test.repo().head().unwrap().expect("a commit");
    assert!(
        head.len() >= 7 && full.starts_with(&head),
        "{head} / {full}"
    );
}

// --- Staged files ----------------------------------------------------------------------------

#[test]
fn staged_files_are_the_added_and_changed_ones_with_their_blobs() {
    let test = TestRepo::new();
    test.write("Assets/Kept.png", "kept");
    test.write("Assets/Changed.png", "old");
    test.write("Assets/Deleted.png", "deleted");
    test.write("Assets/Moved.png", "moved");
    test.git(&["add", "."]);
    test.commit();

    test.write("Assets/Changed.png", "new");
    test.write("Assets/Art/T_Crème brûlée.png", "added");
    test.git(&["rm", "-q", "Assets/Deleted.png"]);
    test.git(&["mv", "Assets/Moved.png", "Assets/Art/Moved.png"]);
    test.git(&["add", "."]);

    let staged = test.staged();
    let paths: Vec<&str> = staged.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "Assets/Art/Moved.png",
            "Assets/Art/T_Crème brûlée.png",
            "Assets/Changed.png"
        ]
    );
    for file in &staged {
        assert_eq!(file.blob, test.hash(&file.path), "{}", file.path);
        assert!(!file.partially_staged, "{}", file.path);
    }
}

#[test]
fn staged_files_before_the_first_commit() {
    let test = TestRepo::new();
    test.write("Assets/T_Crate_D.png", "crate");
    test.git(&["add", "."]);
    let staged = test.staged();
    assert_eq!(
        staged,
        [StagedFile {
            path: "Assets/T_Crate_D.png".to_owned(),
            blob: test.hash("Assets/T_Crate_D.png"),
            partially_staged: false,
        }]
    );
}

#[test]
fn submodules_are_not_staged_files() {
    let test = TestRepo::new();
    test.write("a.txt", "a");
    test.git(&["add", "a.txt"]);
    test.commit();
    let commit = test.git(&["rev-parse", "HEAD"]);
    // A gitlink, as `git submodule add` would stage for the rules repo.
    test.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{},rules", commit.trim()),
    ]);
    test.write("Assets/T_Crate_D.png", "crate");
    test.git(&["add", "Assets"]);
    let paths: Vec<String> = test.staged().into_iter().map(|file| file.path).collect();
    assert_eq!(paths, ["Assets/T_Crate_D.png"]);
}

#[test]
fn nothing_staged_is_an_empty_list() {
    let test = TestRepo::new();
    test.write("a.txt", "a");
    assert_eq!(test.staged(), []);
}

// --- Staged content --------------------------------------------------------------------------

#[test]
fn a_file_with_no_unstaged_changes_is_read_in_place() {
    let test = TestRepo::new();
    test.write("Assets/SM_Rock.fbx", "rock");
    test.git(&["add", "."]);
    let repo = test.repo();
    let staged = repo.staged().unwrap();
    let content = repo.content(&staged[0]).unwrap();
    assert!(!content.is_copy());
    assert_eq!(content.path(), repo.root().join("Assets/SM_Rock.fbx"));
}

#[test]
fn a_partially_staged_file_is_read_from_a_copy_of_its_staged_blob() {
    let test = TestRepo::new();
    test.write("Assets/SM_Rock.fbx", "staged rock");
    test.git(&["add", "."]);
    test.write("Assets/SM_Rock.fbx", "edited after staging");

    let repo = test.repo();
    let staged = repo.staged().unwrap();
    assert!(staged[0].partially_staged);
    let content = repo.content(&staged[0]).unwrap();
    assert!(content.is_copy());
    assert_eq!(content.path().file_name().unwrap(), "SM_Rock.fbx");
    assert!(
        content
            .path()
            .starts_with(repo.common_dir().join("asset-check/tmp"))
    );
    assert_eq!(fs::read_to_string(content.path()).unwrap(), "staged rock");

    let copy = content.path().to_owned();
    drop(content);
    assert!(!copy.exists(), "the copy is removed when dropped");
    assert!(!copy.parent().unwrap().exists());
}

#[test]
fn a_staged_file_deleted_from_the_working_tree_is_read_from_its_blob() {
    let test = TestRepo::new();
    test.write("Assets/T_Crate_D.png", "crate");
    test.git(&["add", "."]);
    fs::remove_file(test.path().join("Assets/T_Crate_D.png")).unwrap();

    let repo = test.repo();
    let staged = repo.staged().unwrap();
    assert!(staged[0].partially_staged);
    let content = repo.content(&staged[0]).unwrap();
    assert_eq!(fs::read_to_string(content.path()).unwrap(), "crate");
}

#[test]
fn a_partially_staged_lfs_file_is_read_from_its_lfs_object() {
    let test = TestRepo::new();
    let object = "the real FBX bytes";
    test.write("Assets/Art/SM_Rock.fbx", &pointer(OID, object.len()));
    test.git(&["add", "."]);
    // What git-lfs's clean filter leaves when the file is staged.
    test.write(
        &format!(".git/lfs/objects/{}/{}/{OID}", &OID[0..2], &OID[2..4]),
        object,
    );
    test.write("Assets/Art/SM_Rock.fbx", "edited after staging");

    let repo = test.repo();
    let staged = repo.staged().unwrap();
    assert!(staged[0].partially_staged);
    let content = repo.content(&staged[0]).unwrap();
    assert_eq!(content.path().file_name().unwrap(), "SM_Rock.fbx");
    assert_eq!(fs::read_to_string(content.path()).unwrap(), object);
}

#[test]
fn a_missing_lfs_object_is_not_found() {
    let test = TestRepo::new();
    test.write("Assets/Art/SM_Rock.fbx", &pointer(OID, 10));
    test.git(&["add", "."]);
    test.write("Assets/Art/SM_Rock.fbx", "edited after staging");

    let repo = test.repo();
    let staged = repo.staged().unwrap();
    let error = repo.content(&staged[0]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error}");
    assert!(error.to_string().contains("Git LFS object"), "{error}");
    let tmp = repo.common_dir().join("asset-check/tmp");
    assert_eq!(fs::read_dir(tmp).unwrap().count(), 0, "nothing left behind");
}

#[test]
fn text_that_only_looks_like_a_pointer_is_copied_as_it_is() {
    let test = TestRepo::new();
    let not_a_pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\n",
        "z".repeat(64)
    );
    test.write("Assets/notes.txt", &not_a_pointer);
    test.git(&["add", "."]);
    test.write("Assets/notes.txt", "edited");

    let repo = test.repo();
    let staged = repo.staged().unwrap();
    let content = repo.content(&staged[0]).unwrap();
    assert_eq!(fs::read_to_string(content.path()).unwrap(), not_a_pointer);
}
