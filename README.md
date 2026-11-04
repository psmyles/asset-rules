# asset-rules

The shared Rust crate behind Asset Check, which checks art assets (textures, models) against a
technical artist's rules when they are committed to a Unity project.

Both checkers depend on it: `fire-cli` (images, in fire) and `3d-review-cli` (models, in
3D Review). It answers one question the same way for every caller (the git hook, the Fork command,
the Unity tool, CI): **which rules apply to this file?** It also defines the results format both
CLIs write.

## What it is not

- **It is not the rules repo.** The rules repo is a separate git submodule that only the technical
  artist writes. It holds `rules.json` and the per-profile rules files. This crate reads that data;
  it doesn't contain it.
- **It does not know what a rule means.** It hands each CLI the path of the rules file that
  applies, and the CLI parses that file and runs the checks.

## Using it

The CLIs depend on a tagged release:

```toml
asset-rules = { git = "https://github.com/psmyles/asset-rules", tag = "v0.1.0" }
```

To build a CLI against unreleased changes, give the CLI's repository an untracked
`.cargo/config.toml`:

```toml
[patch."https://github.com/psmyles/asset-rules"]
asset-rules = { path = "../asset-rules" }
```

## Developing

Pure Rust plus the `git` command. Builds on the latest stable Rust (1.88 at least). The gate:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The work plan is [`docs/PLAN.md`](docs/PLAN.md).

## Licence

MIT. See [`LICENSE`](LICENSE).
