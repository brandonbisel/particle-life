//! On-disk persistence for the wgpu pipeline cache blob (Vulkan only).
//!
//! The cache is stored in the platform cache directory under
//! `particle-life/pipeline_cache.bin`.  A corrupt or incompatible blob is
//! silently ignored — the caller passes `fallback: true` to wgpu so it creates
//! a fresh empty cache instead of panicking.

use std::path::PathBuf;

fn cache_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;

    #[cfg(target_os = "macos")]
    let base = {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        home.join("Library/Caches")
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut p = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            p.push(".cache");
            p
        });

    Some(base.join("particle-life").join("pipeline_cache.bin"))
}

/// Load the cache blob from disk. Returns `None` if the file does not exist or
/// cannot be read.
pub fn load() -> Option<Vec<u8>> {
    let path = cache_path()?;
    std::fs::read(&path).ok()
}

/// Write the cache blob to disk. Silently discards errors (cache is best-effort).
pub fn save(data: &[u8]) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, data);
}
