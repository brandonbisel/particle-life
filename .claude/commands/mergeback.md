# Mergeback: main → dev

After a release has been merged to `main`, run this skill to sync those changes back into `dev`.
This keeps `dev` up to date with any release-branch commits (version bump, benchmark updates,
doc edits, regenerated licenses) so future feature branches don't diverge from what shipped.

## Arguments

`$ARGUMENTS` may be one of:

| Token | Effect |
|-------|--------|
| *(none)* | Full mergeback workflow (merge main → dev, push) |
| `pr` | Create a PR for the merge instead of pushing directly |
| `dry-run` | Show what would happen without making any changes |

---

## Step 1 — Pre-flight Checks

Run the following and **stop immediately** if any check fails:

```sh
git status --porcelain
```
If output is non-empty: "Working tree is dirty. Commit or stash all changes before running /mergeback." and stop.

```sh
git fetch origin
```

Check that `main` has had a release merged recently:
```sh
git log origin/main --oneline -5
```
Show the user the last 5 commits on `origin/main` and confirm a release merge is present.
If the top commit doesn't look like a release (no version bump, no merge commit from a release branch), warn:
"The latest commit on `main` doesn't look like a release merge. Proceed anyway? (y/n)"

Check that `dev` doesn't already contain everything from `main`:
```sh
git log origin/dev..origin/main --oneline
```
If output is empty: "dev is already up to date with main. Nothing to do." and stop.

---

## Step 2 — Show What Will Be Merged

Display the commits on `main` that are not yet in `dev`:
```sh
git log origin/dev..origin/main --oneline
```

If `$ARGUMENTS` contains `dry-run`:
Report what would be merged and stop. "Dry run complete — no changes made."

---

## Step 3 — Switch to dev and Merge

```sh
git checkout dev
git pull origin dev
```

Merge `main` into `dev`:
```sh
git merge origin/main --no-ff -m "chore: merge main into dev after release"
```

If there are merge conflicts:
- List the conflicting files
- Attempt to resolve automatically if conflicts are trivial (e.g., both sides changed different lines in AGENTS.md)
- For complex conflicts: "Merge conflict in <file> — manual resolution needed." Show the conflict markers and stop

If `$ARGUMENTS` contains `pr`, skip to Step 4b instead.

---

## Step 4a — Push dev (default)

```sh
git push origin dev
```

Confirm push succeeded. Report the commits that were merged.

Skip Step 4b.

---

## Step 4b — Create PR (if `pr` argument)

Push the current state to a temporary branch and open a PR:
```sh
git checkout -b mergeback/main-to-dev
git push -u origin mergeback/main-to-dev
```

```sh
gh pr create \
  --base dev \
  --title "chore: merge main into dev after release" \
  --body "$(cat <<'EOF'
## Mergeback: main → dev

Syncs release branch commits (version bump, benchmarks, docs, licenses) back into `dev`
so future feature work branches from the correct base.

### Commits being merged
<list from Step 2>

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Final Summary

```
Mergeback complete
==================
Merged:  origin/main → dev
Commits: <N> commit(s) synced
Method:  direct push / PR (<URL>)

dev is now up to date with the released state of main.
```
