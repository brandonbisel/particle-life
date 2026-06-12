# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

See **AGENTS.md** for full architecture, critical patterns, dependency constraints, and branching/CI details.

## Commands

```sh
cargo check                  # fastest compile check (no link)
cargo test                   # unit tests in config.rs and simulation.rs (headless, no GPU)
cargo build --release        # release build
cargo run --release          # run the app
cargo fmt --check            # CI formatting gate
cargo clippy -- -D warnings  # CI: all warnings are errors
```

Always use `--release` for any performance work — debug builds are not representative.