//! Git helpers (AC15): the repository, the branch, which files are staged, and their staged
//! content.
//!
//! Everything shells out to `git`, which is on PATH wherever the checks run (the hook, Fork,
//! Unity, CI). No libgit2. The environment is inherited, so inside a `pre-commit` hook git's own
//! `GIT_INDEX_FILE` (set for `git commit <paths>`) is the index that is read.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Context;

/// A git repository (or worktree, or submodule) found from a folder inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    root: PathBuf,
    common_dir: PathBuf,
}

/// A file added or changed in the index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagedFile {
    /// Repository-relative, with `/` separators, as git prints it.
    pub path: String,
    /// The staged blob's id. For a Git LFS file, the pointer's.
    pub blob: String,
    /// The working file differs from what is staged, so the staged content must be read from
    /// git rather than from the working file.
    pub partially_staged: bool,
}

/// Where to read a staged file's content: the working file, or a temporary copy of the staged
/// content under the same file name, removed when this is dropped.
#[derive(Debug)]
pub struct CheckedPath {
    path: PathBuf,
    temp_dir: Option<PathBuf>,
}

impl CheckedPath {
    /// The file to read. Its file name is always the staged file's, so a loader that goes by the
    /// extension sees the right one.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether this is a temporary copy rather than the working file.
    pub fn is_copy(&self) -> bool {
        self.temp_dir.is_some()
    }
}

impl Drop for CheckedPath {
    fn drop(&mut self) {
        if let Some(dir) = &self.temp_dir {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

impl Repo {
    /// The repository containing `dir`: `git rev-parse --show-toplevel --git-common-dir`.
    pub fn discover(dir: &Path) -> io::Result<Repo> {
        let out = run(dir, &["rev-parse", "--show-toplevel", "--git-common-dir"])?;
        let text = String::from_utf8_lossy(&out);
        let mut lines = text.lines();
        let (Some(root), Some(common_dir)) = (lines.next(), lines.next()) else {
            return Err(io::Error::other(format!(
                "git rev-parse printed {text:?} for {}",
                dir.display()
            )));
        };
        // `--git-common-dir` may be relative to the folder git ran in.
        Ok(Repo {
            root: PathBuf::from(root),
            common_dir: dir.join(common_dir),
        })
    }

    /// The top folder of the working tree.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The git folder shared by every worktree: where Git LFS keeps its objects and Asset Check
    /// its cache. Never assumed to be `<root>/.git`, which is a file in worktrees and submodules.
    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    /// The current branch's short name (`git symbolic-ref --short -q HEAD`), or `None` for a
    /// detached HEAD. A new repository's branch counts before its first commit.
    pub fn branch(&self) -> io::Result<Option<String>> {
        let output = output(&self.root, &["symbolic-ref", "--short", "-q", "HEAD"])?;
        match output.status.code() {
            Some(0) => Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            )),
            Some(1) => Ok(None),
            _ => Err(failed(&["symbolic-ref", "--short", "-q", "HEAD"], &output)),
        }
    }

    /// The current commit's abbreviated id, or `None` before the first commit.
    pub fn head(&self) -> io::Result<Option<String>> {
        let output = output(
            &self.root,
            &["rev-parse", "-q", "--verify", "--short", "HEAD"],
        )?;
        Ok(output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
    }

    /// The context for [`resolve`](crate::resolve): this repository's root and branch.
    pub fn context(&self) -> io::Result<Context> {
        Ok(Context {
            repo_root: self.root.clone(),
            branch: self.branch()?,
        })
    }

    /// The files added or changed in the index, in path order.
    ///
    /// Only regular files: submodules (such as the rules repo itself) and symlinks are left out.
    /// A renamed file is listed under its new path, and deleted files not at all.
    pub fn staged(&self) -> io::Result<Vec<StagedFile>> {
        // `--raw` gives each file's staged blob id beside its path, so no path is ever passed on
        // the command line (Windows caps one at about 32,000 characters). `--no-renames` lists a
        // rename as an addition at its new path, whatever `diff.renames` says.
        let staged = run(
            &self.root,
            &[
                "diff",
                "--cached",
                "--raw",
                "-z",
                "--no-abbrev",
                "--no-renames",
                "--no-ext-diff",
                "--diff-filter=ACM",
            ],
        )?;
        let unstaged = run(
            &self.root,
            &["diff", "--name-only", "-z", "--no-renames", "--no-ext-diff"],
        )?;
        let unstaged: HashSet<&[u8]> = fields(&unstaged).collect();

        let mut files = Vec::new();
        let mut fields = fields(&staged);
        while let (Some(header), Some(path)) = (fields.next(), fields.next()) {
            // `:<old mode> <new mode> <old id> <new id> <status>`
            let header = String::from_utf8_lossy(header);
            let parts: Vec<&str> = header.split(' ').collect();
            let [_, mode, _, blob, _] = parts.as_slice() else {
                return Err(io::Error::other(format!(
                    "unexpected git diff --raw line: {header:?}"
                )));
            };
            if !mode.starts_with("100") {
                continue;
            }
            files.push(StagedFile {
                path: String::from_utf8_lossy(path).into_owned(),
                blob: (*blob).to_owned(),
                partially_staged: unstaged.contains(path),
            });
        }
        Ok(files)
    }

    /// Where to read `file`'s staged content.
    ///
    /// A file with no unstaged changes is read in place. Otherwise the staged blob is copied to a
    /// temporary file of the same name under `<common dir>/asset-check/tmp/`; if the blob is a
    /// Git LFS pointer, the LFS object it points to is copied instead.
    pub fn content(&self, file: &StagedFile) -> io::Result<CheckedPath> {
        if !file.partially_staged {
            return Ok(CheckedPath {
                path: self.root.join(&file.path),
                temp_dir: None,
            });
        }
        let temp_dir = self.temp_dir()?;
        let name = Path::new(&file.path)
            .file_name()
            .unwrap_or("staged".as_ref());
        // Owned from here on, so the folder is removed if anything below fails.
        let checked = CheckedPath {
            path: temp_dir.join(name),
            temp_dir: Some(temp_dir),
        };

        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["cat-file", "blob", &file.blob])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stdout(File::create(&checked.path)?)
            .stderr(Stdio::piped())
            .output()
            .map_err(not_found)?;
        if !output.status.success() {
            return Err(failed(&["cat-file", "blob", &file.blob], &output));
        }

        if let Some(oid) = lfs_pointer(&checked.path)? {
            let object = self.lfs_object(&oid);
            if !object.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "the Git LFS object for {} isn't on this machine ({})",
                        file.path,
                        object.display()
                    ),
                ));
            }
            // Removed first: on macOS a copy to a new file is a clone, which costs nothing.
            fs::remove_file(&checked.path)?;
            fs::copy(&object, &checked.path)?;
        }
        Ok(checked)
    }

    /// `<common dir>/lfs/objects/<oid[0..2]>/<oid[2..4]>/<oid>`.
    fn lfs_object(&self, oid: &str) -> PathBuf {
        self.common_dir
            .join("lfs")
            .join("objects")
            .join(&oid[0..2])
            .join(&oid[2..4])
            .join(oid)
    }

    /// A new, empty folder under `<common dir>/asset-check/tmp/`, unique to this process and call.
    fn temp_dir(&self) -> io::Result<PathBuf> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = self
            .common_dir
            .join("asset-check")
            .join("tmp")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        // One left by a killed process with the same id.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// The LFS object id in the Git LFS pointer file at `path`, or `None` if it isn't one.
///
/// A pointer is a small text file: `version https://git-lfs.github.com/spec/v1`, then
/// `oid sha256:<64 hex digits>` and `size <bytes>`, one per line.
fn lfs_pointer(path: &Path) -> io::Result<Option<String>> {
    const MAX_POINTER: u64 = 1024;
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_POINTER + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_POINTER {
        return Ok(None);
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(None);
    };
    let mut lines = text.lines();
    if lines.next() != Some("version https://git-lfs.github.com/spec/v1") {
        return Ok(None);
    }
    Ok(lines
        .filter_map(|line| line.strip_prefix("oid sha256:"))
        .find(|oid| oid.len() == 64 && oid.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase))
}

/// The NUL-separated fields of `-z` output.
fn fields(out: &[u8]) -> impl Iterator<Item = &[u8]> {
    out.split(|&byte| byte == 0)
        .filter(|field| !field.is_empty())
}

/// Runs `git -C <dir> <args>` and returns its output, whatever its exit status.
fn output(dir: &Path, args: &[&str]) -> io::Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        // Read-only: never take the index lock to refresh it, which inside a hook `git commit`
        // already holds.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(not_found)
}

/// Runs `git -C <dir> <args>` and returns its standard output, or an error with git's message.
fn run(dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let output = output(dir, args)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(failed(args, &output))
    }
}

fn failed(args: &[&str], output: &Output) -> io::Error {
    let message = String::from_utf8_lossy(&output.stderr);
    io::Error::other(format!(
        "git {} failed ({}): {}",
        args.join(" "),
        output.status,
        message.trim()
    ))
}

fn not_found(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::NotFound {
        io::Error::new(io::ErrorKind::NotFound, "git isn't on PATH")
    } else {
        error
    }
}
