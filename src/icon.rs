//! Procedurally generated 64×64 app icon.
//!
//! Six soft Gaussian circles in the six simulation palette colours, arranged
//! in a ring on the dark background used by the renderer.  No asset files
//! are needed — the icon is derived entirely from the simulation constants.

const ICON_SIZE: u32 = 64;

/// Generates the raw RGBA pixel data for the 64×64 icon.
fn rgba_pixels() -> Vec<u8> {
    let w = ICON_SIZE as usize;
    let h = ICON_SIZE as usize;
    let mut pixels = vec![0u8; w * h * 4];

    // Dark background matching the simulation clear colour (~rgb(5,5,12))
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = 5;
        chunk[1] = 5;
        chunk[2] = 12;
        chunk[3] = 255;
    }

    // PALETTE decoded as (R, G, B) — simulation stores 0xFFBBGGRR
    let colors: [(u8, u8, u8); 6] = [
        (220, 60, 60),  // species 0 — red
        (60, 220, 60),  // species 1 — green
        (60, 80, 220),  // species 2 — blue
        (220, 200, 50), // species 3 — yellow
        (160, 50, 220), // species 4 — purple
        (50, 210, 210), // species 5 — cyan
    ];

    let cx = w as f32 / 2.0 - 0.5;
    let cy = h as f32 / 2.0 - 0.5;
    let ring_r = w as f32 * 0.32;
    let dot_sigma = w as f32 * 0.085;
    let sigma2 = dot_sigma * dot_sigma;
    let n = colors.len() as f32;

    for (i, &(cr, cg, cb)) in colors.iter().enumerate() {
        // Start from top (−π/2) and proceed clockwise
        let angle = i as f32 * std::f32::consts::TAU / n - std::f32::consts::FRAC_PI_2;
        let dot_cx = cx + ring_r * angle.cos();
        let dot_cy = cy + ring_r * angle.sin();

        for py in 0..h {
            for px in 0..w {
                let dx = px as f32 - dot_cx;
                let dy = py as f32 - dot_cy;
                let a = (-(dx * dx + dy * dy) / (2.0 * sigma2)).exp();
                if a < 0.004 {
                    continue;
                }
                let idx = (py * w + px) * 4;
                let br = pixels[idx] as f32;
                let bg_val = pixels[idx + 1] as f32;
                let bb = pixels[idx + 2] as f32;
                pixels[idx] = (cr as f32 * a + br * (1.0 - a)) as u8;
                pixels[idx + 1] = (cg as f32 * a + bg_val * (1.0 - a)) as u8;
                pixels[idx + 2] = (cb as f32 * a + bb * (1.0 - a)) as u8;
            }
        }
    }

    pixels
}

/// Creates the winit window icon.  Works on X11 / XWayland.
/// On native Wayland the compositor uses the XDG mechanism instead —
/// see `install_xdg_resources` below.
pub fn app_icon() -> winit::window::Icon {
    let pixels = rgba_pixels();
    winit::window::Icon::from_rgba(pixels, ICON_SIZE, ICON_SIZE).expect("valid icon dimensions")
}

/// Encodes the icon RGBA data as a PNG byte vector.
fn icon_png_bytes() -> Vec<u8> {
    let pixels = rgba_pixels();
    let mut buf = Vec::new();
    let mut enc = png::Encoder::new(&mut buf, ICON_SIZE, ICON_SIZE);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("png header")
        .write_image_data(&pixels)
        .expect("png data");
    buf
}

/// On Linux, installs the icon PNG and a `.desktop` file into the user's XDG
/// data directories so that Wayland compositors (and X11 window managers) can
/// display the correct icon in the taskbar.
///
/// Writes:
///   `~/.local/share/icons/hicolor/64x64/apps/particle-life.png`
///   `~/.local/share/applications/particle-life.desktop`
///
/// The `.desktop` `Exec` field is set to the current binary path so the
/// compositor can link the running process back to the entry.
#[cfg(target_os = "linux")]
pub fn install_xdg_resources() {
    use std::fs;

    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let home = std::path::PathBuf::from(home);

    // ── icon PNG ──────────────────────────────────────────────────────────────
    let icon_dir = home.join(".local/share/icons/hicolor/64x64/apps");
    if fs::create_dir_all(&icon_dir).is_ok() {
        let _ = fs::write(icon_dir.join("particle-life.png"), icon_png_bytes());
    }

    // ── .desktop file ─────────────────────────────────────────────────────────
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned))
        .unwrap_or_else(|| "particle-life".to_owned());

    let desktop = format!(
        "[Desktop Entry]\n\
         Version=1.0\n\
         Type=Application\n\
         Name=Particle Life\n\
         GenericName=Particle Simulator\n\
         Comment=GPU-accelerated emergent particle life simulator\n\
         Exec={exe}\n\
         Icon=particle-life\n\
         Categories=Science;Simulation;\n\
         StartupWMClass=particle-life\n"
    );

    let apps_dir = home.join(".local/share/applications");
    if fs::create_dir_all(&apps_dir).is_ok() {
        let _ = fs::write(apps_dir.join("particle-life.desktop"), desktop);
    }
}
