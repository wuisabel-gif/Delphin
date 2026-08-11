# Maintainer Checklist

This checklist captures repository settings that are configured in GitHub rather
than in the codebase. Use it after changing workflows, labels, or release
processes.

## Branch Protection

Recommended settings for `main`:

- Require a pull request before merging.
- Require status checks to pass before merging.
- Require branches to be up to date before merging if the queue is busy.
- Dismiss stale approvals when new commits are pushed.
- Restrict force pushes and branch deletion on `main`.
- Allow maintainers to bypass only when an urgent release or repository repair
  needs it.

Recommended required checks:

- `fmt (ubuntu-latest)`
- `fmt (macos-latest)`
- `clippy (ubuntu-latest)`
- `clippy (macos-latest)`
- `test (ubuntu-latest)`
- `test (macos-latest)`
- `title and body`
- `cargo audit`

If the dependency audit workflow has not merged yet, add `cargo audit` after it
lands.

## Pull Requests

- Keep PRs as drafts until they are ready for review.
- Prefer small, single-purpose PRs.
- Use the PR template checklist for Rust checks and local-first scope.
- For contributor-facing changes, confirm docs and examples still point to the
  current commands.

## Labels

Keep these labels available for contributor triage:

- `good first issue`
- `help wanted`
- `bug`
- `enhancement`
- `documentation`

Use `good first issue` only when the issue has a narrow scope and enough context
for a newcomer to start without reverse-engineering the whole supervisor loop.

## Releases

- Move the changelog's `Unreleased` section to the release version and date.
- Confirm `Cargo.toml` has the intended version.
- Run CI plus `cargo package`.
- Publish crates.io and Homebrew artifacts before advertising install commands.
- Create the GitHub release from the changelog entry.
