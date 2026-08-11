# Next Release Cutoff Proposal

This is a maintainer-facing proposal for the next Delphin release cutoff. Treat
it as a draft until the release PR is opened.

## Proposed Release

- Version: `0.3.0`
- Cutoff date: 2026-08-16
- Target release window: 2026-08-17 to 2026-08-18

## Scope Included At Cutoff

The release should include the work already in `main` by the cutoff date:

- Replay support and the reusable library API.
- PTY conformance hardening for crashes, silent agents, resize propagation, and
  link-loss scenarios.
- Local memory hardening, including schema validation and literal recall search.
- Non-Unicode environment forwarding.
- Contributor and maintainer polish: issue forms, split CI checks, PR hygiene,
  dependency audit, architecture docs, contributor quickstart, maintainer
  checklist, and Code of Conduct.

## Allowed After Cutoff

After the cutoff, only merge changes that reduce release risk:

- CI, packaging, or release automation fixes.
- Documentation corrections for commands, install paths, or release notes.
- Crash fixes or data-loss fixes with focused tests.
- Security or privacy fixes.
- Changes needed for `cargo package`, crates.io publication, Homebrew packaging,
  or GitHub release creation.

## Deferred After Cutoff

Hold these for the next development cycle unless they block the release:

- New CLI flags or configuration surface.
- New arbiter policies.
- Idle-detection behavior changes.
- Memory schema changes.
- Large README rewrites or demo updates.
- Broad refactors that are not required for packaging or release correctness.

## Release Gate

Before cutting the release:

- `cargo fmt --all --check` passes.
- `cargo clippy --all-targets -- -D warnings` passes.
- `cargo test --all` passes on Linux and macOS.
- `cargo audit` passes, with any non-failing warnings acknowledged.
- `cargo package` succeeds.
- `CHANGELOG.md` moves `Unreleased` to `0.3.0 - 2026-08-18` or the actual
  release date.
- `Cargo.toml` version matches the changelog.
- README install instructions match the artifacts that are actually available.

## Merge Policy During Cutoff

- Prefer small PRs with one clear purpose.
- Require a focused regression test for behavior fixes.
- Require explicit maintainer approval for any scope expansion.
- If a change risks delaying the release, defer it unless it fixes a release
  blocker.
