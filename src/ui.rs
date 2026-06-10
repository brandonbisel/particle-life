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

pub fn draw_toolbar(
    ctx: &egui::Context,
    tool: &mut Tool,
    tool_range: &mut f32,
    mouse_strength: &mut f32,
) {
    egui::Window::new("Tools")
        .default_pos([10.0, 560.0])
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
                }
                _ => {}
            }
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
