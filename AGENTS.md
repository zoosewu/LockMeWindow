## Agent skills

### Issue tracker

Issues are tracked as local Markdown under `.scratch/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Use the default five-role vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository. See `docs/agents/domain.md`.

## Commit convention

Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): description`. Scope is optional.

Release notes are generated from these types:

- `feat` — Added
- `fix` — Fixed
- `perf`, `refactor` — Changed
- `chore` — Internal
- `test`, `build`, `ci`, `docs` — not listed

## Releases

Releases are managed by release-please (`.github/workflows/release-please.yml`). Every push to `main` updates the `chore: release x.y.z` pull request. Merging it tags `vX.Y.Z`, creates the GitHub Release, and attaches `lock-me-window.exe` with its `.sha256`.

Do not edit the version in `Cargo.toml`/`Cargo.lock` or `CHANGELOG.md` by hand.
