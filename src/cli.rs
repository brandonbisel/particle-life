//! Command-line interface definitions.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "ParticleLife",
    about = "GPU-accelerated particle life simulator"
)]
pub struct CliArgs {
    /// Run the full benchmark suite (all presets × particle tiers) and write results to CSV, then exit.
    #[arg(long)]
    pub bench: bool,

    /// Run the capacity benchmark (binary-search max particles per preset at target FPS) and write results to CSV, then exit.
    #[arg(long)]
    pub capacity_bench: bool,

    /// CSV output path for benchmark results.
    /// Defaults to `bench_results.csv` for --bench or `capacity_results.csv` for --capacity-bench.
    #[arg(long, value_name = "FILE")]
    pub bench_output: Option<PathBuf>,

    /// Preset to apply on launch. Accepts a preset name (case-insensitive, e.g. "Chains") or a
    /// 0-based index. Built-in names: Clusters, Chains, Ecosystem, Symbiosis.
    #[arg(long, value_name = "NAME_OR_INDEX")]
    pub preset: Option<String>,

    /// Open in borderless fullscreen on launch.
    #[arg(long)]
    pub fullscreen: bool,

    /// World size as WxH (e.g. 1920x1080).
    #[arg(long, value_name = "WxH", value_parser = parse_world_size)]
    pub world_size: Option<(u32, u32)>,

    /// Particle count (clamped to 100–2 000 000).
    #[arg(long, value_name = "N")]
    pub particles: Option<usize>,

    /// Attraction matrix share code (base64, same format as the in-app share box).
    #[arg(long, value_name = "CODE")]
    pub matrix: Option<String>,

    /// Save a screenshot to PATH then exit. Combine with --preset and --capture-delay
    /// to automate gallery thumbnail generation for new bundled presets.
    #[arg(long, value_name = "PATH")]
    pub capture: Option<PathBuf>,

    /// Seconds to run the simulation before taking the --capture screenshot (default: 5).
    #[arg(long, value_name = "SECS", default_value_t = 5.0)]
    pub capture_delay: f32,
}

fn parse_world_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("expected WxH (e.g. 1920x1080), got {s:?}"))?;
    let w: u32 = w
        .parse()
        .map_err(|_| format!("invalid width {w:?} in {s:?}"))?;
    let h: u32 = h
        .parse()
        .map_err(|_| format!("invalid height {h:?} in {s:?}"))?;
    if w == 0 || h == 0 {
        return Err(format!("world size must be non-zero, got {s:?}"));
    }
    Ok((w, h))
}
