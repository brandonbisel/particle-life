# Benchmark Runner

Run performance benchmarks for the ParticleLife simulation, compare against baselines, and
surface regressions. All benchmarks use `cargo run --release` and disable vsync automatically.

## Arguments

`$ARGUMENTS` may be one or more of the following tokens, space-separated (order does not matter):

| Token | Effect |
|-------|--------|
| `suite` | Run the full 16-combo suite benchmark (~5–6 minutes) |
| `capacity` | Run the capacity binary-search benchmark (~4–5 minutes) |
| `both` | Run suite first, then capacity |
| `compare` | Compare results against stored baselines |
| `save-baseline` | Overwrite stored baselines with this run's results |

If no arguments are given, default to running `suite compare`.

## Baseline File Locations

- Suite baseline: `benchmarks/baseline_suite.csv`
- Capacity baseline: `benchmarks/baseline_capacity.csv`

## Step-by-Step Instructions

### 1. Parse Arguments

Read `$ARGUMENTS`. Set boolean flags:
- `run_suite` = `suite` or `both` present, OR no arguments given
- `run_capacity` = `capacity` or `both` present
- `do_compare` = `compare` present, OR no arguments given
- `do_save` = `save-baseline` present

### 2. Run the Suite Benchmark (if `run_suite`)

Execute and wait for exit:
```
cargo run --release -- --bench --bench-output benchmarks/run_suite.csv
```

Confirm exit code 0 and that `benchmarks/run_suite.csv` was written. If it failed, report
the error and stop. Inform the user when complete.

Suite CSV columns: `preset,particles,species,world_w,world_h,vp_w,vp_h,avg_fps,min_fps,max_fps,avg_frame_ms,frames,wall_secs,vsync`

### 3. Run the Capacity Benchmark (if `run_capacity`)

Execute and wait for exit:
```
cargo run --release -- --capacity-bench --bench-output benchmarks/run_capacity.csv
```

Confirm exit code 0 and that `benchmarks/run_capacity.csv` was written. Inform the user when complete.

Capacity CSV columns: `preset,target_fps,max_particles,achieved_fps,capped,vp_w,vp_h`

### 4. Parse and Summarize Results

#### Suite Summary

For each row, compute:
- **Throughput** = `frames / wall_secs` (true rendered-frame rate)

> **Important**: `avg_fps` inflates 1.1–2.1× at ≤100K particles due to near-zero-dt winit
> events. Always use throughput as the primary metric. At 500K, avg_fps ≈ throughput.

Build a summary table grouped by preset (rows) × particle tier (columns: 10K / 50K / 100K / 500K).
Show throughput (tp) in each cell, and note avg_fps separately for the 500K tier only.

```
Preset      |    10K (tp) |    50K (tp) |   100K (tp) | 500K (tp/avg_fps)
------------|-------------|-------------|-------------|-------------------
Clusters    |      X,XXX  |      X,XXX  |      X,XXX  |       XX / XX.X
...
```

Call out: fastest/slowest preset at 500K; any rows where avg_fps > throughput × 1.5 at ≤100K
(note: this is the inflation artefact, not a real perf difference).

#### Capacity Summary

```
Preset      | Max Particles | Achieved FPS | Capped?
------------|---------------|--------------|--------
Clusters    |       XXX,XXX |         XX.X |   no
```

If `capped` is `true`: note this is a lower bound (hit buffer limit of 2,000,000 particles).

### 5. Compare Against Baseline (if `do_compare`)

#### Suite Comparison

Check if `benchmarks/baseline_suite.csv` exists. If not: say "No suite baseline found. Run
with `save-baseline` first." Otherwise:

For each matching row (same `preset` + `particles`), compute delta using **throughput**:
- **Regression**: new throughput < baseline throughput × 0.95 (>5% drop)
- **Improvement**: new throughput > baseline throughput × 1.05 (>5% gain)
- **Stable**: otherwise

For the 500K tier only, also compare `avg_fps` directly (inflation negligible at that load).

Flag viewport mismatch (`vp_w`/`vp_h` differs between baseline and new run) — do not compare
results recorded at different resolutions.

Delta table:
```
Preset      | Particles | Baseline tp | New tp | Delta   | Status
------------|-----------|-------------|--------|---------|----------
Clusters    |    10,000 |       X,XXX |  X,XXX | +X.X%   | IMPROVED
Chains      |   500,000 |          XX |     XX | −X.X%   | REGRESSED !!
...
```

If any 500K-tier regressions exist, call them out with a **bold warning** — 500K is the most
reliable indicator of real GPU throughput change.

> **Ecosystem note**: Ecosystem 500K has high frame-time variance (dense cluster scatters and
> reforms). Recommend a second run before declaring a regression there.

#### Capacity Comparison

Check `benchmarks/baseline_capacity.csv`. Compare `max_particles` per preset:
- Regression: new < baseline × 0.95
- Improvement: new > baseline × 1.05

Report a delta table similar to above.

### 6. Save Baseline (if `do_save`)

If a suite run was performed this session:
- Copy `benchmarks/run_suite.csv` → `benchmarks/baseline_suite.csv`

If a capacity run was performed this session:
- Copy `benchmarks/run_capacity.csv` → `benchmarks/baseline_capacity.csv`

If `save-baseline` was requested but no run was performed, say: "Nothing to save — no
benchmark was run this session. Include `suite` or `capacity` to run first."

After saving, confirm which baseline files were updated.

### 7. Final Report

End with a one-paragraph summary covering: what was run (or compared), overall health
(no regressions / N regressions found), the most important finding, and next recommended
action (e.g., investigate regression, re-run for variance check, save baseline).

## Build Requirement

Always use `--release`. Debug builds are catastrophically slow and not representative.
