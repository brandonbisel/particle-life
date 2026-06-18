# Release Preparation

Cuts a `release/x.y.z` branch from `dev`, bumps the version, runs all checks and benchmarks,
updates documentation, and opens a PR to `main`. Runs the licenses and benchmark skills
as sub-steps.

Estimated total time: 8–12 minutes (dominated by the benchmark suite ~5–6 min).

## Arguments

`$ARGUMENTS` may be one of:

| Token | Effect |
|-------|--------|
| *(none)* | Full workflow; auto-recommend version bump type |
| `patch` | Force a patch bump (x.y.Z) |
| `minor` | Force a minor bump (x.Y.0) |
| `major` | Force a major bump (X.0.0) |
| `skip-bench` | Skip the benchmark step (useful when baselines are already current) |

---

## Step 1 — Pre-flight Checks

Run the following in order and **stop immediately** if any check fails:

```sh
git status --porcelain
```
If output is non-empty: "Working tree is dirty. Commit or stash all changes before running /release." and stop.

```sh
git rev-parse --abbrev-ref HEAD
```
If not `dev`: "Must run /release from the `dev` branch." and stop.

```sh
git fetch --tags
```

---

## Step 2 — Determine Version Bump

Read the current version from `Cargo.toml` (`[package] version = "x.y.z"`).

If a bump type was given in `$ARGUMENTS`, use it. Otherwise:

Get commits since the last release tag:
```sh
git log $(git describe --tags --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)..HEAD --oneline
```

Recommend a bump type using these rules:
- **major** — any commit message contains `BREAKING`, `breaking change`, or `!:` (conventional-commit breaking marker)
- **minor** — any commit adds a new feature (`feat:` prefix or "add", "new", "introduce" in subject)
- **patch** — otherwise (fixes, docs, chores, refactors)

Show the commit list and your recommendation. Then compute the new version string (e.g., `0.5.0` → patch → `0.5.1`).

Ask the user: "Bump version from X to Y? (patch/minor/major recommendation: Z)" — wait for confirmation or override before continuing.

---

## Step 3 — Create Release Branch

```sh
git checkout -b release/<new-version>
```

Confirm the branch was created.

---

## Step 4 — Bump Version in Cargo.toml

Edit `Cargo.toml`: change the `version = "..."` line in `[package]` to the new version.

Run `cargo check` to regenerate `Cargo.lock` with the new version:
```sh
cargo check
```

---

## Step 5 — Run Licenses Skill

Invoke the licenses skill with the `commit` argument so it regenerates `THIRD_PARTY_LICENSES.html`
and commits it automatically if it changed:

```
/licenses commit
```

If the licenses skill reports any untracked embedded assets or Cargo.toml metadata issues,
report them to the user and ask whether to continue or stop for fixes.

---

## Step 6 — Review README.md and AGENTS.md

Read both files. Read the commit log from Step 2.

Check for stale or missing content in each file:

**README.md** — look for:
- Version numbers or badges that should reflect the new release version
- Feature descriptions that are now outdated or missing based on recent commits
- CLI option lists or usage examples that changed

**AGENTS.md** — look for:
- Architecture descriptions that no longer match the code (e.g., module table, pass counts)
- Dependency version constraints that were bumped
- New modules, structs, or patterns added since the last release
- Branching strategy section — verify it still accurately describes the workflow

For each file, report your findings as a checklist:
```
README.md review:
[x] Version references: up to date
[ ] Feature list: missing mention of <X> added in commit <hash>

AGENTS.md review:
[x] Architecture accurate
[ ] Dependency constraints: wgpu bumped to 25 but docs say 24
```

If you identify changes that are clearly needed (stale version strings, obviously missing features),
make the edits directly and show the diff. For judgment calls, describe the gap and ask the user
whether to update.

If both files look current: "README.md and AGENTS.md look up to date."

---

## Step 7 — Run Automated Checks

Run each check in order. Report pass/fail for each. Stop and report if any check fails —
do not skip to the next step.

```sh
cargo fmt --check
```
**FAIL**: "cargo fmt check failed — run `cargo fmt` and commit the result."

```sh
cargo clippy -- -D warnings
```
**FAIL**: "Clippy found warnings (treated as errors). Fix before proceeding."

```sh
cargo test
```
**FAIL**: "Test suite failed. Fix failing tests before proceeding."

```sh
cargo doc --no-deps 2>&1
```
Filter for `warning:` or `error[` lines. Report any found; they are non-blocking but should
be noted in the checklist.

If all pass: "All checks passed (fmt / clippy / test / doc)."

---

## Step 8 — Run Benchmark Suite (skip if `skip-bench`)

Run the full benchmark suite and save it as the new baseline:

```sh
cargo run --release -- --bench --bench-output benchmarks/run_suite.csv
```

Wait for completion (≈5–6 minutes). Confirm `benchmarks/run_suite.csv` was written.

Copy to baseline:
```sh
cp benchmarks/run_suite.csv benchmarks/baseline_suite.csv
```

### Generate benchmarks.md

Read `benchmarks/run_suite.csv`. Compute throughput = `frames / wall_secs` for each row
(use throughput, not avg_fps, per the benchmark skill guidance).

Write `benchmarks/benchmarks.md` with this format:

```markdown
# Performance Benchmarks

**Version:** <new-version>  
**Date:** <today's date YYYY-MM-DD>  
**Platform:** Linux (AMD RX 9070 XT, RADV Vulkan)

> Throughput = frames ÷ wall_seconds (reliable primary metric).  
> avg_fps inflates 1.1–2.1× at ≤100K particles due to near-zero-dt winit events.

## Suite Results

| Preset | 10K (tp) | 50K (tp) | 100K (tp) | 500K (tp / avg fps) |
|--------|----------|----------|-----------|---------------------|
| ...    |          |          |           |                     |

*Viewport: WxH px*
```

Fill in the table from the CSV. For 500K, show both throughput and avg_fps.
Note the viewport dimensions from the CSV's `vp_w`/`vp_h` columns.

Commit benchmark results:
```sh
git add benchmarks/run_suite.csv benchmarks/baseline_suite.csv benchmarks/benchmarks.md
git commit -m "chore: update benchmarks for v<new-version>"
```

---

## Step 9 — Commit Version Bump and Documentation Changes

Stage and commit:
```sh
git add Cargo.toml Cargo.lock
```

Also stage any README.md or AGENTS.md edits made in Step 6:
```sh
git add README.md AGENTS.md
```

```sh
git commit -m "chore: bump version to <new-version>"
```

(Only include files that actually changed — don't add unmodified files.)

---

## Step 10 — Final Checks and Push

Run a final status check:
```sh
git log dev..HEAD --oneline
```

Show the user the commits that will be on the release branch. Confirm before pushing.

```sh
git push -u origin release/<new-version>
```

---

## Step 11 — Create Pull Request to `main`

Get the commit log for the PR body:
```sh
git log dev..HEAD --oneline
```

Create the PR:
```sh
gh pr create \
  --base main \
  --title "Release v<new-version>" \
  --body "$(cat <<'EOF'
## Release v<new-version>

### Changes
<bullet list of commits from Step 2 git log, grouped by type>

### Checklist
- [x] Version bumped in Cargo.toml
- [x] THIRD_PARTY_LICENSES.html regenerated
- [x] cargo fmt / clippy / test / doc all pass
- [x] Benchmark baselines updated
- [ ] README.md reviewed
- [ ] AGENTS.md reviewed

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Mark README and AGENTS checklist items based on whether changes were needed/made in Step 6.

---

## Final Summary

Print a release prep summary:
```
Release v<new-version> — prep complete
========================================
Branch:    release/<new-version>
PR:        <URL>

Checks:    fmt [pass] · clippy [pass] · test [pass] · doc [pass/N warnings]
Licenses:  updated / unchanged
Benchmarks: updated / skipped
Docs:      README [updated/ok] · AGENTS [updated/ok]

Next: get the PR reviewed, then merge to main to trigger the GitHub Release workflow.
After merging, run /mergeback to sync main → dev.
```
