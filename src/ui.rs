//! egui panel and overlay drawing functions.
//!
//! All functions are stateless — they receive mutable references to the data
//! they display and return response structs that `app.rs` acts on.

use std::collections::VecDeque;

use crate::simulation::{MAX_SPECIES, PALETTE_DEFAULT, SimulationState};
use crate::{benchmark, config};

use egui_phosphor::regular as ph;

// ── UI response ───────────────────────────────────────────────────────────────

/// Actions requested by the main Particle Life panel.
#[derive(Default)]
pub struct UiResponse {
    pub respawn: bool,
    pub randomize: bool,
    /// Resize world to match the current window dimensions.
    pub match_win: bool,
    /// Apply the currently selected preset to the simulation.
    pub apply_preset: bool,
    /// Open a file dialog to import a preset (caller handles the dialog).
    pub import_preset: bool,
    /// Open a file dialog to export a preset (caller handles the dialog).
    pub export_preset: bool,
    /// The species palette was modified; caller should push it to the GPU.
    pub palette_changed: bool,
    /// Palette randomize was requested; caller calls `sim.randomize_palette()`.
    pub randomize_palette: bool,
}

/// Actions requested by the Performance / benchmark panel.
pub struct BenchmarkPanelResponse {
    /// Start the full benchmark suite.
    pub start: bool,
    /// Export collected results to CSV.
    pub export_csv: bool,
    /// Start a quick single-point benchmark at the current particle count.
    pub start_quick: bool,
    /// `Some(v)` when the user toggled the global vsync checkbox.
    pub vsync: Option<bool>,
}

/// The active mouse tool selected in the toolbar.
#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Pan,
    ZoomIn,
    ZoomOut,
    Attract,
    Repel,
    Spawn,
}

/// Draw the right-side vertical toolbar: icon tool buttons and Reset View.
///
/// Returns `(reset_view_clicked, toolbar_screen_rect)`.  The caller should
/// pass the rect to [`draw_tool_options`] so it can position itself flush
/// against the toolbar's left edge.
pub fn draw_toolbar(ctx: &egui::Context, tool: &mut Tool) -> (bool, egui::Rect) {
    // Use Area+Frame instead of Window so the panel sizes to content with no cached minimum.
    let response = egui::Area::new(egui::Id::new("toolbar"))
        .anchor(egui::Align2::RIGHT_CENTER, [-10.0, 0.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style())
                .show(ui, |ui| {
                    let mut rv = false;
                    let icon_sz = egui::Vec2::splat(32.0);

                    let r = ui
                        .add_sized(
                            icon_sz,
                            egui::SelectableLabel::new(*tool == Tool::Pan, ph::HAND_GRABBING),
                        )
                        .on_hover_text("Pan — drag to move the camera");
                    if r.clicked() {
                        *tool = Tool::Pan;
                    }

                    let r = ui
                        .add_sized(
                            icon_sz,
                            egui::SelectableLabel::new(
                                *tool == Tool::ZoomIn,
                                ph::MAGNIFYING_GLASS_PLUS,
                            ),
                        )
                        .on_hover_text("Zoom In — click to zoom in centered on the cursor");
                    if r.clicked() {
                        *tool = Tool::ZoomIn;
                    }

                    let r = ui
                        .add_sized(
                            icon_sz,
                            egui::SelectableLabel::new(
                                *tool == Tool::ZoomOut,
                                ph::MAGNIFYING_GLASS_MINUS,
                            ),
                        )
                        .on_hover_text("Zoom Out — click to zoom out centered on the cursor");
                    if r.clicked() {
                        *tool = Tool::ZoomOut;
                    }

                    let r = ui
                        .add_sized(
                            icon_sz,
                            egui::SelectableLabel::new(*tool == Tool::Attract, ph::MAGNET),
                        )
                        .on_hover_text("Attract — hold to pull nearby particles toward the cursor");
                    if r.clicked() {
                        *tool = Tool::Attract;
                    }

                    let r = ui
                        .add_sized(
                            icon_sz,
                            egui::SelectableLabel::new(
                                *tool == Tool::Repel,
                                ph::ARROWS_OUT_CARDINAL,
                            ),
                        )
                        .on_hover_text(
                            "Repel — hold to push nearby particles away from the cursor",
                        );
                    if r.clicked() {
                        *tool = Tool::Repel;
                    }

                    let r = ui
                        .add_sized(
                            icon_sz,
                            egui::SelectableLabel::new(*tool == Tool::Spawn, ph::SPARKLE),
                        )
                        .on_hover_text("Spawn — hold to emit new particles at the cursor");
                    if r.clicked() {
                        *tool = Tool::Spawn;
                    }

                    ui.separator();

                    if ui
                        .add_sized(icon_sz, egui::Button::new(ph::ARROWS_COUNTER_CLOCKWISE))
                        .on_hover_text("Reset view to default zoom and position")
                        .clicked()
                    {
                        rv = true;
                    }

                    rv
                })
                .inner
        });

    (response.inner, response.response.rect)
}

/// Draw the floating tool-options panel when a parametric tool is active.
///
/// The panel appears flush to the left of the toolbar (using `toolbar_rect`
/// from the current frame's [`draw_toolbar`] call) and disappears entirely
/// when Pan, ZoomIn, or ZoomOut is selected.
#[allow(clippy::too_many_arguments)]
pub fn draw_tool_options(
    ctx: &egui::Context,
    tool: Tool,
    toolbar_rect: egui::Rect,
    tool_range: &mut f32,
    mouse_strength: &mut f32,
    spawn_species: &mut Option<usize>,
    spawn_rate: &mut u32,
    n_species: usize,
    palette: &[u32; 8],
) {
    if !matches!(tool, Tool::Attract | Tool::Repel | Tool::Spawn) {
        return;
    }

    // Anchor the panel's right edge 5px to the left of the toolbar.
    let vp_width = ctx.screen_rect().width();
    let x_offset = -(vp_width - toolbar_rect.left() + 5.0);

    egui::Area::new(egui::Id::new("tool_options"))
        .anchor(egui::Align2::RIGHT_CENTER, [x_offset, 0.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| match tool {
                Tool::Attract | Tool::Repel => {
                    ui.add(
                        egui::Slider::new(tool_range, 0.02..=0.4)
                            .text("Range")
                            .step_by(0.01),
                    )
                    .on_hover_text("Radius of the mouse influence zone");
                    ui.add(
                        egui::Slider::new(mouse_strength, 0.1..=10.0)
                            .text("Strength")
                            .step_by(0.1),
                    )
                    .on_hover_text("How strongly the tool affects nearby particles");
                }
                Tool::Spawn => {
                    ui.add(
                        egui::Slider::new(tool_range, 0.01..=0.3)
                            .text("Radius")
                            .step_by(0.005),
                    )
                    .on_hover_text("Radius of the spawn zone around the cursor");
                    ui.add(
                        egui::Slider::new(spawn_rate, 1..=500)
                            .text("Rate (per frame)")
                            .logarithmic(true),
                    )
                    .on_hover_text("Number of particles spawned per frame while holding");
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        let any_sel = spawn_species.is_none();
                        if ui
                            .selectable_label(any_sel, "Any")
                            .on_hover_text("Spawn a random species")
                            .clicked()
                        {
                            *spawn_species = None;
                        }
                        for i in 0..n_species {
                            let color = species_color(i, palette);
                            let is_sel = *spawn_species == Some(i);
                            let (rect, resp) = ui
                                .allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::click());
                            ui.painter().rect_filled(rect, 3.0, color);
                            if is_sel {
                                ui.painter().rect_stroke(
                                    rect,
                                    egui::CornerRadius::same(3),
                                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            if resp.clicked() {
                                *spawn_species = Some(i);
                            }
                        }
                    });
                }
                _ => unreachable!(),
            });
        });
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)) as u8
}

fn attraction_cell_color(v: f32) -> egui::Color32 {
    let t = v.clamp(-1.0, 1.0);
    if t >= 0.0 {
        egui::Color32::from_rgb(lerp_u8(40, 20, t), lerp_u8(40, 120, t), lerp_u8(40, 20, t))
    } else {
        let s = -t;
        egui::Color32::from_rgb(lerp_u8(40, 120, s), lerp_u8(40, 20, s), lerp_u8(40, 20, s))
    }
}

/// Palette theme definitions (sRGB packed `0xFF_BB_GG_RR`).
pub const PALETTE_VIVID: [u32; 8] = [
    0xFF3232DC, // red    (220, 50, 50)
    0xFF32DC32, // green  (50, 220, 50)
    0xFFDC5032, // blue   (50, 80, 220)
    0xFF28C8DC, // yellow (220, 200, 40)
    0xFFDC28A0, // purple (160, 40, 220)
    0xFFD2D228, // cyan   (40, 210, 210)
    0xFF1E82DC, // orange (220, 130, 30)
    0xFFB446DC, // pink   (220, 70, 180)
];
pub const PALETTE_NEON: [u32; 8] = [
    0xFF1414FF, // neon-red     (255, 20, 20)
    0xFF14FF14, // neon-green   (20, 255, 20)
    0xFFFF7814, // neon-blue    (20, 120, 255)
    0xFF00F0FF, // neon-yellow  (255, 240, 0)
    0xFFF000FF, // neon-magenta (255, 0, 240)
    0xFFFFF000, // neon-cyan    (0, 240, 255)
    0xFF008CFF, // neon-orange  (255, 140, 0)
    0xFF00FF96, // neon-lime    (150, 255, 0)
];
pub const PALETTE_PASTEL: [u32; 8] = [
    0xFFB4B4FF, // pastel-red    (255, 180, 180)
    0xFFB4FFB4, // pastel-green  (180, 255, 180)
    0xFFFFC3B4, // pastel-blue   (180, 195, 255)
    0xFFB4FAFF, // pastel-yellow (255, 250, 180)
    0xFFFFB4E1, // pastel-purple (225, 180, 255)
    0xFFF5F5B4, // pastel-cyan   (180, 245, 245)
    0xFFB4DCFF, // pastel-orange (255, 220, 180)
    0xFFE1B4F5, // pastel-pink   (245, 180, 225)
];
pub const PALETTE_DARK: [u32; 8] = [
    0xFF1E1E96, // dark-red    (150, 30, 30)
    0xFF1E961E, // dark-green  (30, 150, 30)
    0xFFA0321E, // dark-blue   (30, 50, 160)
    0xFF1482A0, // dark-amber  (160, 130, 20)
    0xFFA01464, // dark-purple (100, 20, 160)
    0xFF828214, // dark-teal   (20, 130, 130)
    0xFF145AA0, // dark-orange (160, 90, 20)
    0xFF6E1E96, // dark-pink   (150, 30, 110)
];

pub const PALETTE_THEMES: &[(&str, [u32; 8])] = &[
    ("Default", PALETTE_DEFAULT),
    ("Vivid", PALETTE_VIVID),
    ("Neon", PALETTE_NEON),
    ("Pastel", PALETTE_PASTEL),
    ("Dark", PALETTE_DARK),
];

/// Extract an egui `Color32` from a packed sRGB `0xFF_BB_GG_RR` palette entry.
fn species_color(idx: usize, palette: &[u32; 8]) -> egui::Color32 {
    let packed = palette[idx];
    egui::Color32::from_rgb(
        (packed & 0xFF) as u8,
        ((packed >> 8) & 0xFF) as u8,
        ((packed >> 16) & 0xFF) as u8,
    )
}

/// Draw the main "Particle Life" settings panel: particles, species, physics, presets, border.
pub fn draw_ui(
    ctx: &egui::Context,
    sim: &mut SimulationState,
    presets: &[config::Preset],
    selected_preset: &mut usize,
) -> UiResponse {
    let mut resp = UiResponse::default();

    egui::Window::new("Particle Life")
        .anchor(egui::Align2::LEFT_TOP, [10.0, 10.0])
        .show(ctx, |ui| {
            ui.add(
                egui::Slider::new(&mut sim.particle_count, 100..=500_000)
                    .text("Particles")
                    .logarithmic(true),
            )
            .on_hover_text("Total number of particles — respawn required to take effect");
            ui.add(egui::Slider::new(&mut sim.species_count, 2..=8).text("Species"))
                .on_hover_text(
                    "Number of distinct species — each has a unique color and interaction profile; respawn required",
                );
            ui.add(
                egui::Slider::new(&mut sim.particle_radius, 0.5_f32..=12.0_f32)
                    .text("Radius")
                    .step_by(0.5),
            )
            .on_hover_text("Visual rendering radius of each particle in pixels");
            ui.horizontal(|ui| {
                if ui
                    .button("Respawn")
                    .on_hover_text(
                        "Scatter all particles at random positions; preserves the attraction matrix",
                    )
                    .clicked()
                {
                    resp.respawn = true;
                }
                let pause_label = if sim.paused { "Resume" } else { "Pause" };
                if ui
                    .button(pause_label)
                    .on_hover_text("Pause or resume the simulation")
                    .clicked()
                {
                    sim.paused = !sim.paused;
                }
            });

            ui.separator();

            ui.label("World size (units):");
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(&mut sim.world_width)
                        .speed(10.0)
                        .range(100.0..=10000.0)
                        .prefix("W: "),
                )
                .on_hover_text("World width in simulation units");
                ui.add(
                    egui::DragValue::new(&mut sim.world_height)
                        .speed(10.0)
                        .range(100.0..=10000.0)
                        .prefix("H: "),
                )
                .on_hover_text("World height in simulation units");
                if ui
                    .button("Match Window")
                    .on_hover_text("Resize the world to match the current window aspect ratio")
                    .clicked()
                {
                    resp.match_win = true;
                }
            });

            ui.separator();

            ui.add(
                egui::Slider::new(&mut sim.r_min, 0.001_f32..=0.1_f32)
                    .text("r_min")
                    .step_by(0.001),
            )
            .on_hover_text(
                "Hard-core repulsion radius — particles closer than this always repel each other, regardless of species",
            );
            ui.add(
                egui::Slider::new(&mut sim.r_max, 0.01_f32..=0.3_f32)
                    .text("r_max")
                    .step_by(0.005),
            )
            .on_hover_text(
                "Maximum interaction distance — particles beyond this range are invisible to each other",
            );
            ui.add(
                egui::Slider::new(&mut sim.friction, 0.0_f32..=5.0_f32)
                    .text("Friction")
                    .step_by(0.05),
            )
            .on_hover_text(
                "Velocity decay rate — velocity half-life ≈ ln(2)/friction (≈1.4s at default 0.5)",
            );
            ui.add(
                egui::Slider::new(&mut sim.force_scale, 0.0001_f32..=0.05_f32)
                    .text("Force")
                    .step_by(0.0001),
            )
            .on_hover_text("Global multiplier for all attraction and repulsion forces");
            if ui
                .button("Reset Defaults")
                .on_hover_text(
                    "Restore r_min, r_max, friction, and force_scale to their default values",
                )
                .clicked()
            {
                sim.reset_params();
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Border:");
                ui.radio_value(&mut sim.border_mode, 0u32, "Wrap")
                    .on_hover_text(
                        "Wrap — particles that leave one edge reappear on the opposite side (torus)",
                    );
                ui.radio_value(&mut sim.border_mode, 1u32, "Repel")
                    .on_hover_text("Repel — particles are pushed back from the world boundary");
                ui.radio_value(&mut sim.border_mode, 2u32, "Static")
                    .on_hover_text("Static — particles stop at the world boundary (hard wall)");
            });
            if sim.border_mode == 1 {
                ui.add(
                    egui::Slider::new(&mut sim.border_repel_strength, 0.1..=30.0)
                        .text("Repel Force")
                        .step_by(0.1),
                )
                .on_hover_text("Strength of the boundary repulsion spring");
            }

            ui.separator();

            egui::CollapsingHeader::new("Presets")
                .default_open(false)
                .show(ui, |ui| {
                    let label = presets
                        .get(*selected_preset)
                        .map(|p| p.name.as_str())
                        .unwrap_or("—");
                    egui::ComboBox::from_label("")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for (i, preset) in presets.iter().enumerate() {
                                ui.selectable_value(selected_preset, i, &preset.name);
                            }
                        });
                    if let Some(p) = presets.get(*selected_preset)
                        && !p.description.is_empty()
                    {
                        ui.label(egui::RichText::new(&p.description).weak());
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .button("Apply")
                            .on_hover_text("Apply the selected preset to the simulation")
                            .clicked()
                        {
                            resp.apply_preset = true;
                        }
                        if ui
                            .button("Import…")
                            .on_hover_text("Load a preset from a TOML file")
                            .clicked()
                        {
                            resp.import_preset = true;
                        }
                        if ui
                            .button("Export…")
                            .on_hover_text("Save the current simulation state as a TOML preset file")
                            .clicked()
                        {
                            resp.export_preset = true;
                        }
                    });
                });

            ui.separator();

            egui::CollapsingHeader::new("Palette")
                .default_open(false)
                .show(ui, |ui| {
                    // Theme picker
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_label("Theme")
                            .selected_text(
                                PALETTE_THEMES
                                    .iter()
                                    .find(|(_, p)| p == &sim.palette)
                                    .map(|(n, _)| *n)
                                    .unwrap_or("Custom"),
                            )
                            .show_ui(ui, |ui| {
                                for &(name, theme) in PALETTE_THEMES {
                                    if ui.selectable_label(sim.palette == theme, name).clicked() {
                                        sim.palette = theme;
                                        resp.palette_changed = true;
                                    }
                                }
                            });
                        if ui
                            .button("Randomize")
                            .on_hover_text("Generate a new random palette for all active species")
                            .clicked()
                        {
                            // palette is randomized by the caller via UiResponse
                            resp.palette_changed = true;
                            resp.randomize_palette = true;
                        }
                    });

                    ui.separator();

                    // Per-species colour pickers
                    let n = sim.species_count;
                    ui.horizontal_wrapped(|ui| {
                        for i in 0..n {
                            let mut color = species_color(i, &sim.palette);
                            let label = format!("S{}", i + 1);
                            ui.vertical(|ui| {
                                ui.label(&label);
                                if egui::color_picker::color_edit_button_srgba(
                                    ui,
                                    &mut color,
                                    egui::color_picker::Alpha::Opaque,
                                )
                                .changed()
                                {
                                    let r = color.r() as u32;
                                    let g = color.g() as u32;
                                    let b = color.b() as u32;
                                    sim.palette[i] = 0xFF00_0000 | (b << 16) | (g << 8) | r;
                                    resp.palette_changed = true;
                                }
                            });
                        }
                    });
                });

            ui.separator();

            egui::CollapsingHeader::new("Attraction Matrix")
                .default_open(true)
                .show(ui, |ui| {
                    if ui
                        .button("Randomize Matrix")
                        .on_hover_text(
                            "Fill the attraction matrix with random values; particles are not moved",
                        )
                        .clicked()
                    {
                        resp.randomize = true;
                    }

                    let n = sim.species_count;

                    egui::Grid::new("attraction_grid")
                        .min_col_width(36.0)
                        .show(ui, |ui| {
                            // Header row: blank corner + one label per column species
                            ui.label("");
                            for j in 0..n {
                                ui.colored_label(species_color(j, &sim.palette), format!("S{}", j + 1));
                            }
                            ui.end_row();

                            // Data rows: row species label + N drag values
                            for i in 0..n {
                                ui.colored_label(species_color(i, &sim.palette), format!("S{}", i + 1));
                                for j in 0..n {
                                    let v = sim.attraction[i * MAX_SPECIES + j];
                                    let bg = attraction_cell_color(v);
                                    egui::Frame::new()
                                        .fill(bg)
                                        .inner_margin(egui::Margin::same(2))
                                        .show(ui, |ui| {
                                            ui.visuals_mut().widgets.inactive.weak_bg_fill =
                                                egui::Color32::TRANSPARENT;
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut sim.attraction[i * MAX_SPECIES + j],
                                                )
                                                .range(-1.0_f32..=1.0_f32)
                                                .speed(0.01),
                                            );
                                        });
                                }
                                ui.end_row();
                            }
                        });
                });
        });

    resp
}

/// Convert a simulation world coordinate to an egui screen position.
fn world_to_screen(
    world: [f32; 2],
    center: [f32; 2],
    world_aspect: f32,
    shader_zoom: f32,
    rect: egui::Rect,
) -> egui::Pos2 {
    let sx = (world[0] - center[0]) * world_aspect * shader_zoom * rect.height() + rect.center().x;
    let sy = rect.center().y - (world[1] - center[1]) * shader_zoom * rect.height();
    egui::pos2(sx, sy)
}

/// Draw a border rectangle around the simulation world `[0,1]²`.
///
/// Rendered on the `Background` layer so it sits behind particles.
/// Colour reflects the active border mode: blue = wrap, amber = repel, red = static.
pub fn draw_world_border(
    ctx: &egui::Context,
    camera_center: [f32; 2],
    world_aspect: f32,
    shader_zoom: f32,
    border_mode: u32,
) {
    let color = match border_mode {
        1 => egui::Color32::from_rgba_unmultiplied(255, 190, 80, 90), // amber — repel
        2 => egui::Color32::from_rgba_unmultiplied(255, 90, 90, 90),  // red   — static
        _ => egui::Color32::from_rgba_unmultiplied(180, 210, 255, 70), // blue  — wrap
    };

    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("world_border"),
    ));

    let tl = world_to_screen([0.0, 1.0], camera_center, world_aspect, shader_zoom, rect);
    let br = world_to_screen([1.0, 0.0], camera_center, world_aspect, shader_zoom, rect);
    painter.rect_stroke(
        egui::Rect::from_min_max(tl, br),
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Middle,
    );
}

/// Draw a brush-size circle around the cursor for range-based tools.
///
/// Attract/Repel also render a radial gradient fill approximating the quadratic
/// force falloff.  No-ops for Pan, ZoomIn, and ZoomOut.
pub fn draw_cursor_indicator(ctx: &egui::Context, tool: Tool, tool_range: f32, shader_zoom: f32) {
    if !matches!(tool, Tool::Attract | Tool::Repel | Tool::Spawn) {
        return;
    }
    if ctx.is_pointer_over_area() {
        return;
    }
    let Some(cursor) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };

    let screen_radius = tool_range * shader_zoom * ctx.screen_rect().height();

    let (r, g, b) = match tool {
        Tool::Attract => (100u8, 200u8, 255u8),
        Tool::Repel => (255u8, 100u8, 100u8),
        Tool::Spawn => (100u8, 255u8, 130u8),
        _ => unreachable!(),
    };

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("cursor_indicator"),
    ));

    // Radial gradient fill for Attract/Repel — approximates the quadratic falloff
    // by stacking concentric filled circles from the outside inward. Each circle
    // adds a small alpha; the center accumulates all layers, the edge only the outermost.
    if matches!(tool, Tool::Attract | Tool::Repel) {
        const RINGS: usize = 24;
        for ring in 0..RINGS {
            let frac = ring as f32 / RINGS as f32; // 0 = outermost, ~1 = innermost
            let r_px = screen_radius * (1.0 - frac);
            let fill = egui::Color32::from_rgba_unmultiplied(r, g, b, 5);
            painter.circle_filled(cursor, r_px, fill);
        }
    }

    // Outer ring (border of the influence zone)
    let border = egui::Color32::from_rgba_unmultiplied(r, g, b, 180);
    painter.circle_stroke(cursor, screen_radius, egui::Stroke::new(1.5, border));
}

/// Draw the top-right Performance panel with live FPS stats, Quick Bench, and the Suite Benchmark.
///
/// The window starts collapsed; the benchmark sub-sections are inside a
/// [`CollapsingHeader`](egui::CollapsingHeader) so they don't clutter the default view.
pub fn draw_perf_overlay(
    ctx: &egui::Context,
    frame_times: &VecDeque<f32>,
    sim: &SimulationState,
    quick_bench: &benchmark::QuickBench,
    runner: &mut benchmark::BenchmarkRunner,
    vsync: bool,
    vsync_available: bool,
) -> BenchmarkPanelResponse {
    let mut resp = BenchmarkPanelResponse {
        start: false,
        export_csv: false,
        start_quick: false,
        vsync: None,
    };

    let n = frame_times.len();
    if n == 0 {
        return resp;
    }

    let latest_dt = *frame_times.back().unwrap();
    let avg_dt: f32 = frame_times.iter().sum::<f32>() / n as f32;
    let min_dt: f32 = frame_times.iter().cloned().fold(f32::MAX, f32::min);
    let max_dt: f32 = frame_times.iter().cloned().fold(0.0_f32, f32::max);

    let grid_w = ((2.0 / sim.r_max) as usize).max(5);
    let n_cells = grid_w * grid_w;
    let density = sim.particle_count as f32 / n_cells as f32;

    egui::Window::new("Performance")
        .anchor(egui::Align2::LEFT_BOTTOM, [10.0, -10.0])
        .resizable(false)
        .collapsible(true)
        .default_open(false)
        .show(ctx, |ui| {
            ui.set_min_width(250.0);
            egui::Grid::new("perf_grid")
                .num_columns(2)
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.label("FPS")
                        .on_hover_text("Current and average frames per second");
                    ui.label(format!(
                        "{:>5.0}  avg {:>5.0}",
                        1.0 / latest_dt,
                        1.0 / avg_dt
                    ));
                    ui.end_row();

                    ui.label("Frame")
                        .on_hover_text("Current frame time with min/max over the sample window");
                    ui.label(format!(
                        "{:>5.1} ms  ({:>5.1}–{:>5.1})",
                        latest_dt * 1000.0,
                        min_dt * 1000.0,
                        max_dt * 1000.0,
                    ));
                    ui.end_row();

                    ui.label("Particles")
                        .on_hover_text("Number of particles currently allocated on the GPU");
                    ui.label(format!("{}", sim.particle_count_gpu()));
                    ui.end_row();

                    ui.label("Grid").on_hover_text(
                        "Spatial grid cell count and average particles per cell (cell size = r_max/2)",
                    );
                    ui.label(format!("{n_cells} cells  {density:.0} avg/cell"));
                    ui.end_row();
                });

            ui.separator();

            // Global vsync toggle (Quick Bench follows this setting)
            if vsync_available {
                let mut vsync_val = vsync;
                if ui
                    .checkbox(&mut vsync_val, "VSync")
                    .on_hover_text(
                        "Lock frame rate to the monitor refresh rate; Quick Bench follows this setting",
                    )
                    .changed()
                {
                    resp.vsync = Some(vsync_val);
                }
            } else {
                ui.add_enabled(false, egui::Checkbox::new(&mut true, "VSync (unavailable)"))
                    .on_hover_text("VSync toggle requires PresentMode::Immediate support from the adapter");
            }

            ui.separator();

            // Quick bench
            if let Some((frame, total, is_warmup)) = quick_bench.progress() {
                let phase = if is_warmup { "Warmup" } else { "Collecting" };
                ui.label(format!("{phase}…"));
                ui.add(egui::ProgressBar::new(frame as f32 / total as f32).show_percentage());
            } else if let Some((avg, min, max, particles)) = quick_bench.result() {
                ui.label(format!("Quick bench — {} particles", particles));
                egui::Grid::new("qbench_grid")
                    .num_columns(2)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        ui.label("Avg FPS");
                        ui.label(format!("{avg:.0}"));
                        ui.end_row();
                        ui.label("Min FPS");
                        ui.label(format!("{min:.0}"));
                        ui.end_row();
                        ui.label("Max FPS");
                        ui.label(format!("{max:.0}"));
                        ui.end_row();
                    });
                if ui
                    .button("Run Again")
                    .on_hover_text("Re-run the quick benchmark at the current particle count")
                    .clicked()
                {
                    resp.start_quick = true;
                }
            } else if ui
                .button("Quick Bench")
                .on_hover_text(
                    "Measure average FPS at the current particle count (short warmup + collection run)",
                )
                .clicked()
            {
                resp.start_quick = true;
            }

            ui.separator();

            // Full suite benchmark (collapsed by default to keep the panel tidy)
            egui::CollapsingHeader::new("Suite Benchmark")
                .default_open(false)
                .show(ui, |ui| {
                    // Vsync toggle for suite runs; disabled while a run is in progress
                    let mut suite_vsync = !runner.vsync_off;
                    let cb = ui
                        .add_enabled(
                            !runner.is_running(),
                            egui::Checkbox::new(&mut suite_vsync, "VSync during suite"),
                        )
                        .on_hover_text(
                            "Run the suite with VSync on; default is off for accurate throughput numbers",
                        );
                    if cb.changed() {
                        runner.vsync_off = !suite_vsync;
                    }

                    if runner.is_running() {
                        if let Some((done, total, elapsed, target, is_warmup)) = runner.progress() {
                            let phase = if is_warmup { "Warmup" } else { "Collecting" };
                            ui.label(format!(
                                "Combo {}/{} — {} ({:.0}/{:.0}s)",
                                done + 1,
                                total,
                                phase,
                                elapsed,
                                target
                            ));
                            ui.add(egui::ProgressBar::new(elapsed / target).show_percentage());
                        }
                    } else if runner.is_done() {
                        ui.label(format!("{} results ready", runner.results.len()));
                        if ui
                            .button("Export CSV…")
                            .on_hover_text("Save benchmark results to a CSV file")
                            .clicked()
                        {
                            resp.export_csv = true;
                        }
                    } else {
                        ui.label(format!(
                            "{} combos (4 presets × {} tiers)",
                            benchmark::BenchmarkRunner::num_combos(),
                            benchmark::BENCHMARK_TIERS.len(),
                        ));
                        if ui
                            .button("Start Suite")
                            .on_hover_text(
                                "Run all preset × particle-count combinations (5s warmup + 15s collection each)",
                            )
                            .clicked()
                        {
                            resp.start = true;
                        }
                    }
                });
        });

    resp
}
