# License & Attribution Maintenance

Pre-release skill to refresh `THIRD_PARTY_LICENSES.html`, scan for newly embedded non-Cargo
assets that need attribution, and check rustdoc health. Run when cutting a `release/x.y.z`
branch before the PR to `main`.

## Arguments

`$ARGUMENTS` may be one of:
- *(none)* — run the full checklist (Steps 1–4)
- `html` — only regenerate `THIRD_PARTY_LICENSES.html` (Step 1)
- `audit` — only scan for new assets + check rustdoc + verify Cargo.toml (Steps 2–4, no HTML generation)
- `commit` — after regenerating HTML, automatically commit it with message
  `"chore: regenerate THIRD_PARTY_LICENSES.html"`

## Step-by-Step Instructions

### Step 1 — Regenerate `THIRD_PARTY_LICENSES.html` (skip if `audit`)

Run:
```
cargo about generate about.hbs -o THIRD_PARTY_LICENSES.html
```

If `cargo-about` is not installed, tell the user to install it with:
```
cargo install cargo-about --features=cli
```

After generation, run:
```
git diff --stat THIRD_PARTY_LICENSES.html
```

If the file changed, extract and list the changed crate names (diff the `Used by:` sections).
Report: "THIRD_PARTY_LICENSES.html updated — N lines changed."
If unchanged: "THIRD_PARTY_LICENSES.html is already up to date."

If the `commit` argument was given, commit the file:
```
git add THIRD_PARTY_LICENSES.html
git commit -m "chore: regenerate THIRD_PARTY_LICENSES.html"
```
Otherwise, if the file changed, ask the user whether to commit it before proceeding.

### Step 2 — Scan for new untracked embedded assets (skip if `html`)

Run:
```
grep -rn "// Colors from\|// Source:\|// Credit:\|// Copyright" src/
```

Read `about.hbs` and extract the crate/asset names listed under the `<div class="embedded-assets">` section.

For each attribution comment found in `src/`, check whether the referenced source appears
in the Embedded Assets section of `about.hbs`. Flag any that are missing so they can be
added to the template before the next HTML generation.

Also look for any new third-party color palette patterns that don't use standard comment
markers — search for hardcoded hex color literals in clusters (5+ consecutive `0x??` or
`[u8; 3]` values with nearby comments referencing an external design system).

Report: list of tracked assets, list of any untracked ones that need to be added to `about.hbs`.

### Step 3 — Check rustdoc (skip if `html`)

Run:
```
cargo doc --no-deps 2>&1
```

Filter output for lines containing `warning:` or `error[`. Exclude lines about inactive
`#[cfg]` directives for other platforms (these are expected on single-platform builds).

Group findings by source file and report them as a checklist. If there are no warnings,
report: "Rustdoc: clean."

Note: CI already runs `cargo doc --no-deps` as a gate — this step surfaces issues early
during release prep before CI fires.

### Step 4 — Verify Cargo.toml metadata (skip if `html`)

Read `Cargo.toml`. Confirm the following fields are present and non-empty in `[package]`:
- `version`
- `authors`
- `license`
- `description`
- `repository`

Flag any missing or placeholder values (e.g., `"TODO"`, `"YOUR_NAME"`, empty string).

### Step 5 — Final Pre-Release Checklist

Print a structured checklist summarizing all steps:

```
Pre-Release License & Attribution Checklist
============================================
[x] THIRD_PARTY_LICENSES.html regenerated (or unchanged)
[x] No new untracked embedded assets found in src/
[ ] Rustdoc has N warnings to address:
    - src/foo.rs:42 — missing docs on pub fn bar
[x] Cargo.toml metadata complete
```

Each failing item should include the specific file:line and what action is needed.

End with a one-sentence release readiness verdict: either "Ready to proceed with release"
or "Address the N items above before merging to main."
