use std::collections::VecDeque;

use crate::simulation::{SimulationState, MAX_SPECIES, PALETTE};

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Pan,
    ZoomIn,
    ZoomOut,
    Attract,
    Repel,
    Spawn,
}

/// Returns `true` if the "Reset View" button was clicked.
pub fn draw_toolbar(
    ctx: &egui::Context,
    tool: &mut Tool,
    tool_range: &mut f32,
    mouse_strength: &mut f32,
    spawn_species: &mut Option<usize>,
    spawn_rate: &mut u32,
    n_species: usize,
) -> bool {
    let mut reset_view = false;

    egui::Window::new("Tools")
        .anchor(egui::Align2::LEFT_BOTTOM, [10.0, -10.0])
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(tool, Tool::Pan,     "Pan");
                ui.selectable_value(tool, Tool::ZoomIn,  "Zoom +");
                ui.selectable_value(tool, Tool::ZoomOut, "Zoom -");
                ui.selectable_value(tool, Tool::Attract, "Attract");
                ui.selectable_value(tool, Tool::Repel,   "Repel");
                ui.selectable_value(tool, Tool::Spawn,   "Spawn");
                ui.separator();
                if ui.button("Reset View").clicked() {
                    reset_view = true;
                }
            });
            match *tool {
                Tool::Attract | Tool::Repel => {
                    ui.add(
                        egui::Slider::new(tool_range, 0.02..=0.4)
                            .text("Range")
                            .step_by(0.01),
                    );
                    ui.add(
                        egui::Slider::new(mouse_strength, 0.1..=10.0)
                            .text("Strength")
                            .step_by(0.1),
                    );
                }
                Tool::Spawn => {
                    ui.add(
                        egui::Slider::new(tool_range, 0.01..=0.3)
                            .text("Radius")
                            .step_by(0.005),
                    );
                    ui.add(
                        egui::Slider::new(spawn_rate, 1..=500)
                            .text("Rate (per frame)")
                            .logarithmic(true),
                    );
                    // Species color palette
                    ui.horizontal_wrapped(|ui| {
                        let any_sel = spawn_species.is_none();
                        if ui.selectable_label(any_sel, "Any").clicked() {
                            *spawn_species = None;
                        }
                        for i in 0..n_species {
                            let color = species_color(i);
                            let is_sel = *spawn_species == Some(i);
                            let (rect, resp) =
                                ui.allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::click());
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
                _ => {}
            }
        });

    reset_view
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

fn species_color(idx: usize) -> egui::Color32 {
    let packed = PALETTE[idx];
    egui::Color32::from_rgb(
        ((packed >> 0) & 0xFF) as u8,
        ((packed >> 8) & 0xFF) as u8,
        ((packed >> 16) & 0xFF) as u8,
    )
}

/// Returns (respawn, randomize_matrix).
pub fn draw_ui(ctx: &egui::Context, sim: &mut SimulationState) -> (bool, bool) {
    let mut respawn = false;
    let mut randomize = false;

    egui::Window::new("Particle Life")
        .default_pos([10.0, 10.0])
        .show(ctx, |ui| {
            ui.add(
                egui::Slider::new(&mut sim.particle_count, 100..=500_000)
                    .text("Particles")
                    .logarithmic(true),
            );
            ui.add(egui::Slider::new(&mut sim.species_count, 2..=8).text("Species"));
            ui.add(
                egui::Slider::new(&mut sim.particle_radius, 0.5_f32..=12.0_f32)
                    .text("Size (px)")
                    .step_by(0.5),
            );
            ui.horizontal(|ui| {
                if ui.button("Respawn").clicked() {
                    respawn = true;
                }
                let pause_label = if sim.paused { "Resume" } else { "Pause" };
                if ui.button(pause_label).clicked() {
                    sim.paused = !sim.paused;
                }
            });

            ui.separator();

            ui.add(
                egui::Slider::new(&mut sim.r_min, 0.001_f32..=0.1_f32)
                    .text("r_min")
                    .step_by(0.001),
            );
            ui.add(
                egui::Slider::new(&mut sim.r_max, 0.01_f32..=0.3_f32)
                    .text("r_max")
                    .step_by(0.005),
            );
            ui.add(
                egui::Slider::new(&mut sim.friction, 0.0_f32..=5.0_f32)
                    .text("Friction")
                    .step_by(0.05),
            );
            ui.add(
                egui::Slider::new(&mut sim.force_scale, 0.0001_f32..=0.05_f32)
                    .text("Force")
                    .step_by(0.0001),
            );
            if ui.button("Reset Defaults").clicked() {
                sim.reset_params();
            }

            ui.horizontal(|ui| {
                ui.label("Border:");
                ui.radio_value(&mut sim.border_mode, 0u32, "Wrap");
                ui.radio_value(&mut sim.border_mode, 1u32, "Repel");
                ui.radio_value(&mut sim.border_mode, 2u32, "Static");
            });
            if sim.border_mode == 1 {
                ui.add(
                    egui::Slider::new(&mut sim.border_repel_strength, 0.1..=30.0)
                        .text("Repel Force")
                        .step_by(0.1),
                );
            }

            ui.separator();

            egui::CollapsingHeader::new("Attraction Matrix")
                .default_open(true)
                .show(ui, |ui| {
                    if ui.button("Randomize Matrix").clicked() {
                        randomize = true;
                    }

                    let n = sim.species_count;

                    egui::Grid::new("attraction_grid")
                        .min_col_width(36.0)
                        .show(ui, |ui| {
                            // Header row: blank corner + one label per column species
                            ui.label("");
                            for j in 0..n {
                                ui.colored_label(species_color(j), format!("S{}", j + 1));
                            }
                            ui.end_row();

                            // Data rows: row species label + N drag values
                            for i in 0..n {
                                ui.colored_label(species_color(i), format!("S{}", i + 1));
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

    (respawn, randomize)
}

/// Convert a simulation world coordinate to an egui screen position.
fn world_to_screen(world: [f32; 2], center: [f32; 2], zoom: f32, rect: egui::Rect) -> egui::Pos2 {
    let sx = ((world[0] - center[0]) * zoom + 0.5) * rect.width()  + rect.left();
    let sy = (0.5 - (world[1] - center[1]) * zoom) * rect.height() + rect.top();
    egui::pos2(sx, sy)
}

/// Draw a border rectangle around the simulation world [0,1]².
/// Color reflects the active border mode: blue=wrap, amber=repel, red=static.
pub fn draw_world_border(
    ctx: &egui::Context,
    camera_center: [f32; 2],
    camera_zoom: f32,
    border_mode: u32,
) {
    let color = match border_mode {
        1 => egui::Color32::from_rgba_unmultiplied(255, 190,  80, 90), // amber — repel
        2 => egui::Color32::from_rgba_unmultiplied(255,  90,  90, 90), // red   — static
        _ => egui::Color32::from_rgba_unmultiplied(180, 210, 255, 70), // blue  — wrap
    };

    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("world_border"),
    ));

    let tl = world_to_screen([0.0, 1.0], camera_center, camera_zoom, rect);
    let br = world_to_screen([1.0, 0.0], camera_center, camera_zoom, rect);
    painter.rect_stroke(
        egui::Rect::from_min_max(tl, br),
        egui::CornerRadius::ZERO,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Middle,
    );
}

/// Draw a circle around the cursor showing the active tool's range.
/// For Attract/Repel, also draws a radial gradient fill that approximates the
/// quadratic force falloff. Only shown for tools that use a range.
pub fn draw_cursor_indicator(ctx: &egui::Context, tool: Tool, tool_range: f32, camera_zoom: f32) {
    if !matches!(tool, Tool::Attract | Tool::Repel | Tool::Spawn) {
        return;
    }
    let Some(cursor) = ctx.input(|i| i.pointer.hover_pos()) else { return };

    let screen_radius = tool_range * camera_zoom * ctx.screen_rect().height();

    let (r, g, b) = match tool {
        Tool::Attract => (100u8, 200u8, 255u8),
        Tool::Repel   => (255u8, 100u8, 100u8),
        Tool::Spawn   => (100u8, 255u8, 130u8),
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

pub fn draw_perf_overlay(ctx: &egui::Context, frame_times: &VecDeque<f32>, sim: &SimulationState) {
    let n = frame_times.len();
    if n == 0 {
        return;
    }

    let latest_dt = *frame_times.back().unwrap();
    let avg_dt: f32 = frame_times.iter().sum::<f32>() / n as f32;
    let min_dt: f32 = frame_times.iter().cloned().fold(f32::MAX, f32::min);
    let max_dt: f32 = frame_times.iter().cloned().fold(0.0_f32, f32::max);

    let grid_w = ((2.0 / sim.r_max) as usize).max(5);
    let n_cells = grid_w * grid_w;
    let density = sim.particle_count as f32 / n_cells as f32;

    egui::Window::new("Performance")
        .anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0])
        .resizable(false)
        .collapsible(true)
        .default_open(true)
        .show(ctx, |ui| {
            ui.set_min_width(250.0);
            egui::Grid::new("perf_grid")
                .num_columns(2)
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    ui.label("FPS");
                    ui.label(format!("{:>5.0}  avg {:>5.0}", 1.0 / latest_dt, 1.0 / avg_dt));
                    ui.end_row();

                    ui.label("Frame");
                    ui.label(format!(
                        "{:>5.1} ms  ({:>5.1}–{:>5.1})",
                        latest_dt * 1000.0,
                        min_dt * 1000.0,
                        max_dt * 1000.0,
                    ));
                    ui.end_row();

                    ui.label("Particles");
                    ui.label(format!("{}", sim.particle_count_gpu()));
                    ui.end_row();

                    ui.label("Grid");
                    ui.label(format!("{n_cells} cells  {density:.0} avg/cell"));
                    ui.end_row();
                });
        });
}
