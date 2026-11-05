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
- **It doesn't run checks.** It hands each CLI the path of the rules file that applies, and the
  CLI runs the checks. Each CLI describes its rules in a catalog (`rules-catalog.toml`); this crate
  reads catalogs and checks rules files against them, but the checks themselves are the CLI's.

## What it holds

- `rules.json`, versions 1 and 2 (`check` and `skip`): loading, `rules check`, resolution and
  `rules explain`.
- The results format both CLIs write, the cache, and git (the staged files and their content).
- Rule catalogs (`catalog`): the format each CLI describes its rules in.
- The one JSON style (`json`) every tool writes `rules.json` and rules files in.

## Using it

The CLIs depend on a tagged release:

```toml
asset-rules = { git = "https://github.com/psmyles/asset-rules", tag = "v0.1.0" }
```

To build a CLI against unreleased changes, patch it from a `.cargo/config.toml` in the folder that
holds both checkouts. Not the CLI repository's own: fire and 3D Review both commit theirs, for the
static C runtime. Cargo merges the two, and one file serves both CLIs:

```toml
# <folder holding asset-rules, fire and 3d-review-rs>/.cargo/config.toml
[patch."https://github.com/psmyles/asset-rules"]
asset-rules = { path = "asset-rules" }
```

A path there is relative to the folder holding that `.cargo`. Remove the patch before committing a
CLI's `Cargo.lock`, which would otherwise record the local checkout.

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
