//! egui panel and overlay drawing functions.
//!
//! All functions are stateless — they receive mutable references to the data
//! they display and return response structs that `app.rs` acts on.

use std::collections::VecDeque;
use std::io::Cursor;

use crate::simulation::{MAX_SPECIES, PALETTE_DEFAULT, SimulationState};
use crate::{benchmark, config};

use egui_phosphor::regular as ph;

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Named world-background presets for the Appearance panel.
pub const BG_PRESETS: &[(&str, [u8; 3])] = &[
    ("Void", [3, 3, 5]),
    ("Deep Space", [3, 5, 15]),
    ("Midnight", [5, 10, 25]),
    ("Obsidian", [15, 12, 18]),
    ("Charcoal", [20, 20, 22]),
    ("Ivory", [235, 230, 220]),
    ("White", [255, 255, 255]),
];

/// Apply egui visuals for `theme`.  `os_dark` is used when `theme` is `System`.
/// `overlay_alpha` (0–255) is applied to `window_fill` and `panel_fill` so panels
/// can be made translucent.
///
/// Call at startup and whenever the theme, OS preference, or opacity changes.
pub fn apply_theme(ctx: &egui::Context, theme: config::UiTheme, overlay_alpha: u8, os_dark: bool) {
    match theme {
        config::UiTheme::System => {
            ctx.set_visuals(if os_dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
        }
        config::UiTheme::Dark => ctx.set_visuals(egui::Visuals::dark()),
        config::UiTheme::Light => ctx.set_visuals(egui::Visuals::light()),
        config::UiTheme::Midnight => {
            let mut v = egui::Visuals::dark();
            v.panel_fill = egui::Color32::from_rgb(8, 10, 20);
            v.window_fill = egui::Color32::from_rgb(10, 12, 25);
            v.window_stroke.color = egui::Color32::from_rgb(40, 50, 80);
            v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(12, 15, 30);
            v.widgets.inactive.bg_fill = egui::Color32::from_rgb(20, 24, 45);
            v.widgets.hovered.bg_fill = egui::Color32::from_rgb(30, 36, 65);
            v.widgets.active.bg_fill = egui::Color32::from_rgb(40, 50, 90);
            v.selection.bg_fill = egui::Color32::from_rgb(30, 80, 170);
            ctx.set_visuals(v);
        }
        // Colors from the Nord palette by Arctic Ice Studio — nordtheme.com (MIT)
        config::UiTheme::Nord => {
            let mut v = egui::Visuals::dark();
            v.panel_fill = egui::Color32::from_rgb(46, 52, 64);
            v.window_fill = egui::Color32::from_rgb(59, 66, 82);
            v.window_stroke.color = egui::Color32::from_rgb(76, 86, 106);
            v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(59, 66, 82);
            v.widgets.inactive.bg_fill = egui::Color32::from_rgb(67, 76, 94);
            v.widgets.hovered.bg_fill = egui::Color32::from_rgb(76, 86, 106);
            v.widgets.active.bg_fill = egui::Color32::from_rgb(136, 192, 208);
            v.selection.bg_fill = egui::Color32::from_rgb(94, 129, 172);
            v.override_text_color = Some(egui::Color32::from_rgb(236, 239, 244));
            ctx.set_visuals(v);
        }
        // Colors from the Catppuccin Mocha palette by the Catppuccin org — catppuccin.com (MIT)
        config::UiTheme::Catppuccin => {
            let mut v = egui::Visuals::dark();
            v.panel_fill = egui::Color32::from_rgb(24, 24, 37);
            v.window_fill = egui::Color32::from_rgb(30, 30, 46);
            v.window_stroke.color = egui::Color32::from_rgb(49, 50, 68);
            v.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 30, 46);
            v.widgets.inactive.bg_fill = egui::Color32::from_rgb(49, 50, 68);
            v.widgets.hovered.bg_fill = egui::Color32::from_rgb(58, 60, 78);
            v.widgets.active.bg_fill = egui::Color32::from_rgb(108, 112, 134);
            v.selection.bg_fill = egui::Color32::from_rgb(137, 180, 250);
            v.override_text_color = Some(egui::Color32::from_rgb(205, 214, 244));
            ctx.set_visuals(v);
        }
    }
    if overlay_alpha < 255 {
        let mut vis = ctx.style().visuals.clone();
        let a = overlay_alpha;
        let c = vis.window_fill;
        vis.window_fill = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
        let c = vis.panel_fill;
        vis.panel_fill = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
        ctx.set_visuals(vis);
    }
}

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
    /// Toggle the preset gallery window.
    pub toggle_gallery: bool,
    /// The species palette was modified; caller should push it to the GPU.
    pub palette_changed: bool,
    /// Palette randomize was requested; caller calls `sim.randomize_palette()`.
    pub randomize_palette: bool,
    /// A share code was pasted and Apply clicked; caller decodes and applies it.
    pub apply_share_code: Option<String>,
    /// User clicked "Paste" in the share-code context menu; caller should write
    /// clipboard text into egui temp storage under `share_code_paste_buf`.
    pub paste_share_code: bool,
    /// Theme or background colour changed; caller should apply the new visuals and
    /// persist `appearance`.
    pub appearance_changed: bool,
    /// The pop-out / dock button in the Attraction Matrix was clicked.
    pub matrix_pop_out_toggled: bool,
}

/// Actions requested by the Performance / benchmark panel.
pub struct BenchmarkPanelResponse {
    /// Start the full benchmark suite.
    pub start: bool,
    /// Export collected results to CSV.
    pub export_csv: bool,
    /// Start a quick single-point benchmark at the current particle count.
    pub start_quick: bool,
    /// Start the capacity binary-search benchmark.
    pub start_capacity: bool,
    /// Export capacity results to CSV.
    pub export_capacity_csv: bool,
    /// Cancel the running suite benchmark.
    pub cancel: bool,
    /// Cancel the running capacity benchmark.
    pub cancel_capacity: bool,
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

/// Draw the bottom-center horizontal toolbar: icon tool buttons, Reset View, Screenshot, Gallery,
/// Appearance, and About toggles.
///
/// Returns `(reset_view_clicked, take_screenshot, toggle_gallery, toggle_appearance, toggle_about, toolbar_screen_rect)`.
/// The caller should pass the rect to [`draw_tool_options`] so it can position itself above the toolbar.
pub fn draw_toolbar(
    ctx: &egui::Context,
    tool: &mut Tool,
    gallery_open: bool,
    appearance_open: bool,
    about_open: bool,
    bench_running: bool,
) -> (bool, bool, bool, bool, bool, egui::Rect) {
    // Use Area+Frame instead of Window so the panel sizes to content with no cached minimum.
    let response = egui::Area::new(egui::Id::new("toolbar"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -10.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::window(ui.style())
                .show(ui, |ui| {
                    let mut rv = false;
                    let mut take_screenshot = false;
                    let mut toggle_gallery = false;
                    let mut toggle_appearance = false;
                    let mut toggle_about = false;
                    let icon_sz = egui::Vec2::splat(32.0);

                    ui.horizontal(|ui| {
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

                        ui.add_enabled_ui(!bench_running, |ui| {
                            let r = ui
                                .add_sized(
                                    icon_sz,
                                    egui::SelectableLabel::new(*tool == Tool::Attract, ph::MAGNET),
                                )
                                .on_hover_text(
                                    "Attract — hold to pull nearby particles toward the cursor",
                                );
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
                        });

                        ui.separator();

                        if ui
                            .add_sized(icon_sz, egui::Button::new(ph::ARROWS_COUNTER_CLOCKWISE))
                            .on_hover_text("Reset view to default zoom and position")
                            .clicked()
                        {
                            rv = true;
                        }

                        if ui
                            .add_sized(icon_sz, egui::Button::new(ph::CAMERA))
                            .on_hover_text("Save a screenshot to the screenshots/ folder")
                            .clicked()
                        {
                            take_screenshot = true;
                        }

                        if ui
                            .add_enabled_ui(!bench_running, |ui| {
                                ui.add_sized(
                                    icon_sz,
                                    egui::SelectableLabel::new(gallery_open, ph::IMAGES),
                                )
                                .on_hover_text("Preset Gallery — browse presets visually")
                                .clicked()
                            })
                            .inner
                        {
                            toggle_gallery = true;
                        }

                        ui.separator();

                        if ui
                            .add_sized(
                                icon_sz,
                                egui::SelectableLabel::new(appearance_open, ph::PALETTE),
                            )
                            .on_hover_text("Appearance — colours, theme, and opacity")
                            .clicked()
                        {
                            toggle_appearance = true;
                        }

                        if ui
                            .add_sized(icon_sz, egui::SelectableLabel::new(about_open, ph::INFO))
                            .on_hover_text("About Particle Life")
                            .clicked()
                        {
                            toggle_about = true;
                        }
                    });

                    (
                        rv,
                        take_screenshot,
                        toggle_gallery,
                        toggle_appearance,
                        toggle_about,
                    )
                })
                .inner
        });

    let (rv, take_screenshot, toggle_gallery, toggle_appearance, toggle_about) = response.inner;
    (
        rv,
        take_screenshot,
        toggle_gallery,
        toggle_appearance,
        toggle_about,
        response.response.rect,
    )
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
    palette: &[u32],
) {
    if !matches!(tool, Tool::Attract | Tool::Repel | Tool::Spawn) {
        return;
    }

    // Anchor the panel's bottom edge 5px above the top of the bottom toolbar.
    let screen_h = ctx.screen_rect().height();
    let y_offset = -(screen_h - toolbar_rect.top()) - 5.0;

    egui::Area::new(egui::Id::new("tool_options"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, y_offset])
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

/// Draw the floating preset gallery window.
///
/// Clicking a thumbnail immediately selects and applies that preset.
/// Returns `true` if a preset was clicked (caller should set `apply_preset`).
pub fn draw_gallery(
    ctx: &egui::Context,
    presets: &[config::Preset],
    thumbnails: &[Option<egui::TextureHandle>],
    selected_preset: &mut usize,
    gallery_open: &mut bool,
) -> bool {
    let mut apply = false;
    egui::Window::new("Preset Gallery")
        .open(gallery_open)
        .resizable(true)
        .default_size([560.0, 420.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let thumb_w = 170.0_f32;
                let thumb_h = 106.0_f32;
                let spacing = 8.0_f32;
                let cols = ((ui.available_width() + spacing) / (thumb_w + spacing))
                    .floor()
                    .max(1.0) as usize;
                let thumb_size = egui::Vec2::new(thumb_w, thumb_h);

                egui::Grid::new("preset_gallery_grid")
                    .num_columns(cols)
                    .spacing([spacing, spacing])
                    .show(ui, |ui| {
                        for (i, preset) in presets.iter().enumerate() {
                            let is_selected = i == *selected_preset;

                            ui.vertical(|ui| {
                                let (rect, resp) =
                                    ui.allocate_exact_size(thumb_size, egui::Sense::click());
                                let painter = ui.painter();

                                if let Some(Some(handle)) = thumbnails.get(i) {
                                    painter.image(
                                        handle.id(),
                                        rect,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        egui::Color32::WHITE,
                                    );
                                } else {
                                    painter.rect_filled(rect, 3.0, egui::Color32::from_gray(35));
                                    painter.text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        &preset.name,
                                        egui::FontId::proportional(11.0),
                                        egui::Color32::from_gray(140),
                                    );
                                }

                                if is_selected {
                                    painter.rect_stroke(
                                        rect,
                                        egui::CornerRadius::same(2),
                                        egui::Stroke::new(
                                            2.0,
                                            egui::Color32::from_rgb(100, 180, 255),
                                        ),
                                        egui::StrokeKind::Outside,
                                    );
                                } else if resp.hovered() {
                                    painter.rect_stroke(
                                        rect,
                                        egui::CornerRadius::same(2),
                                        egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
                                        egui::StrokeKind::Outside,
                                    );
                                }

                                let resp = if !preset.description.is_empty() {
                                    resp.on_hover_text(&preset.description)
                                } else {
                                    resp
                                };
                                if resp.clicked() {
                                    *selected_preset = i;
                                    apply = true;
                                }

                                ui.label(egui::RichText::new(&preset.name).small());
                            });

                            if (i + 1) % cols == 0 {
                                ui.end_row();
                            }
                        }
                        // End the final partial row if needed
                        if !presets.is_empty() && !presets.len().is_multiple_of(cols) {
                            ui.end_row();
                        }
                    });
            });
        });
    apply
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

/// Vivid palette — saturated primary hues (sRGB packed `0xFF_BB_GG_RR`).
pub const PALETTE_VIVID: [u32; 16] = [
    0xFF3232DC, // red         (220, 50, 50)
    0xFF32DC32, // green       (50, 220, 50)
    0xFFDC5032, // blue        (50, 80, 220)
    0xFF28C8DC, // yellow      (220, 200, 40)
    0xFFDC28A0, // purple      (160, 40, 220)
    0xFFD2D228, // cyan        (40, 210, 210)
    0xFF1E82DC, // orange      (220, 130, 30)
    0xFFB446DC, // pink        (220, 70, 180)
    0xFF3296DC, // teal        (220, 150, 50)
    0xFF96DC32, // lime        (50, 220, 150)
    0xFFDC9632, // indigo      (50, 150, 220)
    0xFF32DCAA, // amber       (170, 220, 50)
    0xFFDC3264, // magenta     (100, 50, 220)
    0xFF64DC32, // mint        (50, 220, 100)
    0xFF3264DC, // deep-orange (220, 100, 50)
    0xFFAA32DC, // chartreuse  (220, 50, 170)
];
/// Neon palette — maximum-brightness electric colours (sRGB packed `0xFF_BB_GG_RR`).
pub const PALETTE_NEON: [u32; 16] = [
    0xFF1414FF, // neon-red        (255, 20, 20)
    0xFF14FF14, // neon-green      (20, 255, 20)
    0xFFFF7814, // neon-blue       (20, 120, 255)
    0xFF00F0FF, // neon-yellow     (255, 240, 0)
    0xFFF000FF, // neon-magenta    (255, 0, 240)
    0xFFFFF000, // neon-cyan       (0, 240, 255)
    0xFF008CFF, // neon-orange     (255, 140, 0)
    0xFF00FF96, // neon-lime       (150, 255, 0)
    0xFF5050FF, // neon-rose       (255, 80, 80)
    0xFF50FF50, // neon-spring     (80, 255, 80)
    0xFFFF5050, // neon-azure      (80, 80, 255)
    0xFF00AAFF, // neon-gold       (255, 170, 0)
    0xFFAA00FF, // neon-violet     (255, 0, 170)
    0xFFFFAA00, // neon-teal       (0, 170, 255)
    0xFF0050FF, // neon-amber      (255, 80, 0)
    0xFF0096FF, // neon-chartreuse (255, 150, 0)
];
/// Pastel palette — soft, desaturated tints (sRGB packed `0xFF_BB_GG_RR`).
pub const PALETTE_PASTEL: [u32; 16] = [
    0xFFB4B4FF, // pastel-red       (255, 180, 180)
    0xFFB4FFB4, // pastel-green     (180, 255, 180)
    0xFFFFC3B4, // pastel-blue      (180, 195, 255)
    0xFFB4FAFF, // pastel-yellow    (255, 250, 180)
    0xFFFFB4E1, // pastel-purple    (225, 180, 255)
    0xFFF5F5B4, // pastel-cyan      (180, 245, 245)
    0xFFB4DCFF, // pastel-orange    (255, 220, 180)
    0xFFE1B4F5, // pastel-pink      (245, 180, 225)
    0xFFB4D4FF, // pastel-rose      (255, 212, 180)
    0xFFD4FFB4, // pastel-mint      (180, 255, 212)
    0xFFFFD4B4, // pastel-periwinkle(180, 212, 255)
    0xFFB4FFE8, // pastel-lemon     (232, 255, 180)
    0xFFFFB4C8, // pastel-lilac     (200, 180, 255)
    0xFFE8FFB4, // pastel-seafoam   (180, 255, 232)
    0xFFB4C8FF, // pastel-peach     (255, 200, 180)
    0xFFC8B4FF, // pastel-butter    (255, 180, 200)
];
/// Dark palette — deep, low-luminance tones (sRGB packed `0xFF_BB_GG_RR`).
pub const PALETTE_DARK: [u32; 16] = [
    0xFF1E1E96, // dark-red       (150, 30, 30)
    0xFF1E961E, // dark-green     (30, 150, 30)
    0xFFA0321E, // dark-blue      (30, 50, 160)
    0xFF1482A0, // dark-amber     (160, 130, 20)
    0xFFA01464, // dark-purple    (100, 20, 160)
    0xFF828214, // dark-teal      (20, 130, 130)
    0xFF145AA0, // dark-orange    (160, 90, 20)
    0xFF6E1E96, // dark-pink      (150, 30, 110)
    0xFF321E96, // dark-crimson   (150, 30, 50)
    0xFF1E9650, // dark-emerald   (50, 150, 30)
    0xFF503296, // dark-indigo    (30, 50, 80)
    0xFF1496C8, // dark-gold      (200, 150, 20)
    0xFF961450, // dark-maroon    (80, 20, 150)
    0xFF50821E, // dark-forest    (30, 130, 80)
    0xFF144696, // dark-brown     (150, 70, 20)
    0xFF50326E, // dark-plum      (110, 50, 80)
];

/// All named palette themes in display order for the Theme combo-box.
pub const PALETTE_THEMES: &[(&str, [u32; 16])] = &[
    ("Default", PALETTE_DEFAULT),
    ("Vivid", PALETTE_VIVID),
    ("Neon", PALETTE_NEON),
    ("Pastel", PALETTE_PASTEL),
    ("Dark", PALETTE_DARK),
];

// ── Preset thumbnails ─────────────────────────────────────────────────────────

fn decode_png_rgba(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let pixels = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgb => pixels
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255u8])
            .collect(),
        png::ColorType::Rgba => pixels.to_vec(),
        _ => return None,
    };
    Some((info.width as usize, info.height as usize, rgba))
}

/// Try to load a PNG thumbnail for `name`.
///
/// Searches: `assets/images/preset-{slug}.png` (builtins),
/// `assets/presets/{name}.png` (bundled), `presets/{name}.png` (user).
pub fn load_preset_thumbnail(name: &str, ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let slug = name.to_lowercase().replace(' ', "-");
    let paths = [
        format!("assets/images/preset-{slug}.png"),
        format!("assets/presets/{name}.png"),
        format!("presets/{name}.png"),
    ];
    for path in &paths {
        if let Ok(bytes) = std::fs::read(path)
            && let Some((w, h, rgba)) = decode_png_rgba(&bytes)
        {
            let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            return Some(ctx.load_texture(
                format!("preset_thumb_{name}"),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }
    None
}

/// Load a thumbnail for every preset; `None` where no matching image is found.
pub fn load_preset_thumbnails(
    presets: &[config::Preset],
    ctx: &egui::Context,
) -> Vec<Option<egui::TextureHandle>> {
    presets
        .iter()
        .map(|p| load_preset_thumbnail(&p.name, ctx))
        .collect()
}

// ── Palette helpers ───────────────────────────────────────────────────────────

/// Extract an egui `Color32` from a packed sRGB `0xFF_BB_GG_RR` palette entry.
fn species_color(idx: usize, palette: &[u32]) -> egui::Color32 {
    let packed = palette[idx];
    egui::Color32::from_rgb(
        (packed & 0xFF) as u8,
        ((packed >> 8) & 0xFF) as u8,
        ((packed >> 16) & 0xFF) as u8,
    )
}

/// Draw the main "Particle Life" settings panel: particles, species, physics, presets, border,
/// and attraction matrix.  Appearance / palette settings live in [`draw_appearance_overlay`].
pub fn draw_ui(
    ctx: &egui::Context,
    sim: &mut SimulationState,
    bench_running: bool,
    matrix_popped_out: &mut bool,
    time_scale: &mut f32,
) -> UiResponse {
    let mut resp = UiResponse::default();

    // Wide enough for 6 default species columns without triggering the horizontal scrollbar.
    // Each cell ≈ col_w + Frame/DragValue padding.  Add row-label col, window chrome, and
    // the outer vertical-scrollbar width (8 px) so the horizontal scroll never appears.
    let matrix_default_cols = 6_f32;
    let cell_w = 36.0 + 28.0; // col_width + margins/padding at ≤8 species
    let min_w = matrix_default_cols * cell_w + 56.0; // row-label + chrome + vert-scrollbar gutter

    egui::Window::new("Particle Life")
        .default_pos([10.0, 10.0])
        .resizable(true)
        .min_width(min_w)
        .show(ctx, |ui| {
            if bench_running {
                ui.disable();
            }
            egui::ScrollArea::vertical()
                .max_height(ctx.screen_rect().height() - 30.0)
                .show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut sim.particle_count, 100..=2_000_000)
                    .text("Particles")
                    .logarithmic(true),
            )
            .on_hover_text("Total number of particles — respawn required to take effect");
            ui.add(egui::Slider::new(&mut sim.species_count, 2..=MAX_SPECIES).text("Species"))
                .on_hover_text(
                    "Number of distinct species — each has a unique color and interaction profile; respawn required",
                );
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
                    .on_hover_text("Pause or resume the simulation (Space)")
                    .clicked()
                {
                    sim.paused = !sim.paused;
                }
                if sim.paused
                    && ui
                        .button("Step ▸")
                        .on_hover_text("Advance one physics frame (→ key)")
                        .clicked()
                {
                    sim.step_requested = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Speed:");
                ui.add(
                    egui::Slider::new(time_scale, 0.05_f32..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                        .custom_parser(|s| {
                            s.trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)
                        }),
                )
                .on_hover_text(
                    "Simulation speed relative to real time; slow motion helps reveal \
                     structure in chaotic presets (Space to pause, → to step)",
                );
            });
            ui.checkbox(&mut sim.random_species_dist, "Random population")
                .on_hover_text(
                    "When enabled, Respawn assigns each species a random share of particles \
                     instead of equal shares",
                );

            egui::CollapsingHeader::new("World Size")
                .default_open(false)
                .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("World size:");
                ui.checkbox(&mut sim.auto_density, "Auto-density")
                    .on_hover_text(
                        "Automatically scale the world so particle density stays roughly constant \
                         as particle count changes.  Keeps GPU load roughly linear with \
                         particle count instead of quadratic.",
                    );
            });
            if sim.auto_density {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut sim.perf_auto, false, "Fixed density")
                        .on_hover_text(
                            "Scale world to maintain a fixed target density, applied on Respawn. \
                             World size is computed from the density slider and current particle count.",
                        );
                    ui.radio_value(&mut sim.perf_auto, true, "Auto-performance")
                        .on_hover_text(
                            "Dynamically adjust world size every ~2 s to approach a target FPS. \
                             No respawn needed — only r_max changes.",
                        );
                });
                if sim.perf_auto {
                    ui.add(
                        egui::Slider::new(&mut sim.perf_target_fps, 15.0_f32..=240.0_f32)
                            .text("Target FPS")
                            .step_by(5.0),
                    )
                    .on_hover_text(
                        "Target frame rate for the auto-performance controller.  The world size is \
                         adjusted every ~2 s to approach this value without a respawn.  Higher target \
                         → larger world, lower density; lower target → smaller world, higher density.",
                    );
                } else {
                    let ref_area = 1280.0_f32 * 720.0_f32;
                    let mut ref_count =
                        (sim.density_target * ref_area).round().clamp(500.0, 2_000_000.0) as usize;
                    if ui
                        .add(
                            egui::Slider::new(&mut ref_count, 500..=2_000_000)
                                .text("Density equiv.")
                                .logarithmic(true),
                        )
                        .on_hover_text(
                            "Target density expressed as an equivalent particle count in a 1280×720 \
                             world.  Lower = sparser (faster); higher = denser (richer physics).  \
                             Applied on Respawn.",
                        )
                        .changed()
                    {
                        sim.density_target = ref_count as f32 / ref_area;
                    }
                }
            }
            ui.horizontal(|ui| {
                let editable = !sim.auto_density;
                ui.add_enabled(
                    editable,
                    egui::DragValue::new(&mut sim.world_width)
                        .speed(10.0)
                        .range(100.0..=200_000.0)
                        .prefix("W: "),
                )
                .on_hover_text(
                    "World width in simulation units.  Larger worlds dilute particle \
                     density, reducing the normalised interaction radius and GPU load.",
                );
                ui.add_enabled(
                    editable,
                    egui::DragValue::new(&mut sim.world_height)
                        .speed(10.0)
                        .range(100.0..=200_000.0)
                        .prefix("H: "),
                )
                .on_hover_text("World height in simulation units.  See world width.");
                if ui
                    .button("Match Window")
                    .on_hover_text(
                        "Set world size to match the current window dimensions and disable auto-density",
                    )
                    .clicked()
                {
                    resp.match_win = true;
                }
            });
                });   // CollapsingHeader (World Size)

            egui::CollapsingHeader::new("Physics")
                .default_open(false)
                .show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut sim.r_min, 0.001_f32..=0.1_f32)
                    .text("r_min")
                    .step_by(0.001),
            )
            .on_hover_text(
                "Hard-core repulsion radius as a fraction of the reference world height (720 units). \
                 Particles closer than this always repel, regardless of species.",
            );
            ui.add(
                egui::Slider::new(&mut sim.r_max, 0.01_f32..=0.3_f32)
                    .text("r_max")
                    .step_by(0.005),
            )
            .on_hover_text(
                "Maximum interaction distance as a fraction of the reference world height (720 units). \
                 At larger world sizes the effective GPU radius shrinks proportionally, \
                 keeping neighbour count and performance constant.",
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
                });   // CollapsingHeader (Physics)

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
                if ui
                    .radio_value(&mut sim.border_mode, 3u32, "Matrix")
                    .on_hover_text("Matrix — per-species wall force set in the Attraction Matrix below")
                    .clicked()
                {
                    sim.randomize_wall_row();
                }
            });
            if sim.border_mode == 1 || sim.border_mode == 3 {
                let label = if sim.border_mode == 3 { "Wall Force" } else { "Repel Force" };
                ui.add(
                    egui::Slider::new(&mut sim.border_repel_strength, 0.1..=30.0)
                        .text(label)
                        .step_by(0.1),
                )
                .on_hover_text("Global scale for boundary spring force");
            }

            ui.separator();

            egui::CollapsingHeader::new("Presets")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button("Gallery…")
                            .on_hover_text("Browse presets visually with thumbnails")
                            .clicked()
                        {
                            resp.toggle_gallery = true;
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

                    ui.separator();
                    ui.label("Matrix share code:");

                    // Copy row – display current code with a Copy button.
                    let code =
                        crate::config::encode_matrix(sim.species_count, &sim.attraction);
                    ui.horizontal(|ui| {
                        let mut display = code.clone();
                        ui.add(
                            egui::TextEdit::singleline(&mut display)
                                .desired_width(170.0)
                                .interactive(false),
                        );
                        if ui
                            .button("Copy")
                            .on_hover_text("Copy this code to the clipboard")
                            .clicked()
                        {
                            ui.ctx().copy_text(code);
                        }
                    });

                    // Paste row – input field + Apply button.
                    let paste_id = egui::Id::new("share_code_paste_buf");
                    let mut paste: String =
                        ui.data(|d| d.get_temp(paste_id).unwrap_or_default());
                    ui.horizontal(|ui| {
                        let te = ui.add(
                            egui::TextEdit::singleline(&mut paste)
                                .desired_width(170.0)
                                .hint_text("Paste code…"),
                        );
                        te.context_menu(|ui| {
                            if ui.button("Paste").clicked() {
                                resp.paste_share_code = true;
                                ui.close_menu();
                            }
                        });
                        let valid = !paste.is_empty()
                            && crate::config::decode_matrix(&paste).is_ok();
                        if ui
                            .add_enabled(valid, egui::Button::new("Apply"))
                            .on_hover_text("Apply the pasted matrix to the current simulation")
                            .clicked()
                        {
                            resp.apply_share_code = Some(std::mem::take(&mut paste));
                        }
                    });
                    if !paste.is_empty() && let Err(e) = crate::config::decode_matrix(&paste) {
                        ui.colored_label(egui::Color32::RED, format!("Invalid: {e}"));
                    }
                    ui.data_mut(|d| d.insert_temp(paste_id, paste));
                });


            ui.separator();

            egui::CollapsingHeader::new("Attraction Matrix")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui
                            .button("Randomize Matrix")
                            .on_hover_text(
                                "Fill the attraction matrix with random values; particles are not moved",
                            )
                            .clicked()
                        {
                            resp.randomize = true;
                        }
                        let pop_icon = if *matrix_popped_out { ph::ARROWS_IN } else { ph::ARROWS_OUT };
                        let pop_tip = if *matrix_popped_out { "Dock matrix into this panel" } else { "Pop out matrix into its own window" };
                        if ui.small_button(pop_icon).on_hover_text(pop_tip).clicked() {
                            resp.matrix_pop_out_toggled = true;
                        }
                    });

                    // Species visibility toggles: click a swatch to hide/show that species.
                    ui.horizontal(|ui| {
                        ui.label("Show:");
                        let n = sim.species_count;
                        for i in 0..n {
                            let vis = sim.species_visible[i];
                            let base_color = species_color(i, &sim.palette);
                            let swatch = if vis {
                                base_color
                            } else {
                                egui::Color32::from_rgba_unmultiplied(
                                    base_color.r() / 3,
                                    base_color.g() / 3,
                                    base_color.b() / 3,
                                    180,
                                )
                            };
                            let mut label = egui::RichText::new(format!("S{}", i + 1))
                                .color(swatch);
                            if !vis {
                                label = label.strikethrough();
                            }
                            if ui
                                .selectable_label(vis, label)
                                .on_hover_text(if vis {
                                    format!("Species {} visible — click to hide", i + 1)
                                } else {
                                    format!("Species {} hidden — click to show", i + 1)
                                })
                                .clicked()
                            {
                                sim.species_visible[i] = !sim.species_visible[i];
                                resp.palette_changed = true;
                            }
                        }
                    });

                    if *matrix_popped_out {
                        ui.label("(Matrix is in a separate window)");
                    } else {

                    let n = sim.species_count;
                    let col_width = if n <= 8 { 36.0 } else { 22.0 };

                    egui::ScrollArea::horizontal()
                        .id_salt("attraction_scroll")
                        .show(ui, |ui| {
                    egui::Grid::new("attraction_grid")
                        .min_col_width(col_width)
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
                                            let resp = ui.add(
                                                egui::DragValue::new(
                                                    &mut sim.attraction[i * MAX_SPECIES + j],
                                                )
                                                .range(-1.0_f32..=1.0_f32)
                                                .speed(0.01)
                                                .custom_formatter(|n, _| {
                                                    if n >= 0.0 {
                                                        format!(" {n:.4}")
                                                    } else {
                                                        format!("{n:.4}")
                                                    }
                                                })
                                                .custom_parser(|s| s.trim().parse().ok()),
                                            );
                                            if resp.changed() {
                                                sim.mark_attraction_dirty();
                                            }
                                        });
                                }
                                ui.end_row();
                            }

                            // Wall row: only visible in Matrix border mode.
                            if sim.border_mode == 3 {
                                ui.colored_label(egui::Color32::GRAY, "Wall");
                                for j in 0..n {
                                    let bg = attraction_cell_color(sim.attraction[MAX_SPECIES * MAX_SPECIES + j]);
                                    egui::Frame::new()
                                        .fill(bg)
                                        .inner_margin(egui::Margin::same(2))
                                        .show(ui, |ui| {
                                            ui.visuals_mut().widgets.inactive.weak_bg_fill =
                                                egui::Color32::TRANSPARENT;
                                            let resp = ui.add(
                                                egui::DragValue::new(&mut sim.attraction[MAX_SPECIES * MAX_SPECIES + j])
                                                    .range(-1.0_f32..=1.0_f32)
                                                    .speed(0.01)
                                                    .fixed_decimals(4),
                                            );
                                            if resp.changed() {
                                                sim.mark_attraction_dirty();
                                            }
                                        });
                                }
                                ui.end_row();
                            }
                        });
                    });   // ScrollArea (horizontal, attraction matrix)
                    }   // else: matrix not popped out
                });   // CollapsingHeader (Attraction Matrix)
            });   // ScrollArea::vertical (outer)
        }); // Window

    resp
}

/// Draw the dedicated Appearance panel: palette colours, UI theme, overlay opacity, background.
///
/// `open` is toggled by the toolbar Appearance button; the window has no built-in close button.
pub fn draw_appearance_overlay(
    ctx: &egui::Context,
    sim: &mut SimulationState,
    appearance: &mut config::AppearanceConfig,
    os_dark: bool,
    open: &mut bool,
) -> UiResponse {
    let mut resp = UiResponse::default();
    if !*open {
        return resp;
    }

    egui::Window::new("Appearance")
        .open(open)
        .default_pos([10.0, 120.0])
        .resizable(false)
        .min_width(220.0)
        .show(ctx, |ui| {
            // ── Particle Radius ───────────────────────────────────────────────
            ui.add(
                egui::Slider::new(&mut sim.particle_radius, 0.5_f32..=12.0_f32)
                    .text("Radius")
                    .step_by(0.5),
            )
            .on_hover_text(
                "Visual radius of each particle in screen pixels.  \
                 Scales with camera zoom; independent of world size.",
            );
            ui.separator();

            // ── UI Theme ──────────────────────────────────────────────────────
            let theme_name = |t: config::UiTheme| match t {
                config::UiTheme::System => "System",
                config::UiTheme::Dark => "Dark",
                config::UiTheme::Light => "Light",
                config::UiTheme::Midnight => "Midnight",
                config::UiTheme::Nord => "Nord",
                config::UiTheme::Catppuccin => "Catppuccin",
            };
            ui.horizontal(|ui| {
                egui::ComboBox::from_label("UI Theme")
                    .selected_text(theme_name(appearance.ui_theme))
                    .show_ui(ui, |ui| {
                        for t in [
                            config::UiTheme::System,
                            config::UiTheme::Dark,
                            config::UiTheme::Light,
                            config::UiTheme::Midnight,
                            config::UiTheme::Nord,
                            config::UiTheme::Catppuccin,
                        ] {
                            let attribution = match t {
                                config::UiTheme::Nord => {
                                    Some("Nord palette by Arctic Ice Studio — nordtheme.com")
                                }
                                config::UiTheme::Catppuccin => Some(
                                    "Catppuccin Mocha palette by the Catppuccin org — catppuccin.com",
                                ),
                                _ => None,
                            };
                            let label =
                                ui.selectable_label(appearance.ui_theme == t, theme_name(t));
                            let label = if let Some(a) = attribution {
                                label.on_hover_text(a)
                            } else {
                                label
                            };
                            if label.clicked() {
                                appearance.ui_theme = t;
                                resp.appearance_changed = true;
                            }
                        }
                    });
            });

            // ── Overlay Opacity ───────────────────────────────────────────────
            let mut opacity_pct = appearance.overlay_alpha as f32 / 2.55;
            if ui
                .add(
                    egui::Slider::new(&mut opacity_pct, 0.0_f32..=100.0_f32)
                        .text("Opacity")
                        .suffix("%")
                        .fixed_decimals(0),
                )
                .on_hover_text("Transparency of all overlay panels (100% = fully opaque)")
                .changed()
            {
                appearance.overlay_alpha = (opacity_pct * 2.55).round() as u8;
                apply_theme(ctx, appearance.ui_theme, appearance.overlay_alpha, os_dark);
                resp.appearance_changed = true;
            }

            ui.separator();

            // ── Background colour ─────────────────────────────────────────────
            ui.label("Background:");
            ui.horizontal(|ui| {
                let current_preset = BG_PRESETS
                    .iter()
                    .find(|(_, c)| c == &appearance.bg_color)
                    .map(|(n, _)| *n)
                    .unwrap_or("Custom");
                egui::ComboBox::from_id_salt("bg_preset")
                    .selected_text(current_preset)
                    .show_ui(ui, |ui| {
                        for &(name, color) in BG_PRESETS {
                            if ui
                                .selectable_label(appearance.bg_color == color, name)
                                .clicked()
                            {
                                appearance.bg_color = color;
                                resp.appearance_changed = true;
                            }
                        }
                    });
                let mut c32 = egui::Color32::from_rgb(
                    appearance.bg_color[0],
                    appearance.bg_color[1],
                    appearance.bg_color[2],
                );
                if egui::color_picker::color_edit_button_srgba(
                    ui,
                    &mut c32,
                    egui::color_picker::Alpha::Opaque,
                )
                .changed()
                {
                    appearance.bg_color = [c32.r(), c32.g(), c32.b()];
                    resp.appearance_changed = true;
                }
            });

            ui.separator();

            // ── Palette ───────────────────────────────────────────────────────
            ui.label("Palette:");
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
                    resp.palette_changed = true;
                    resp.randomize_palette = true;
                }
            });

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

    resp
}

/// Draw the pop-out Attraction Matrix window.
///
/// Only called when `open` is true (matrix is popped out of the main panel).
/// Returns true if "Randomize Matrix" was clicked.
pub fn draw_matrix_window(ctx: &egui::Context, sim: &mut SimulationState, open: &mut bool) -> bool {
    let mut randomize = false;
    let screen_w = ctx.screen_rect().width();

    egui::Window::new("Attraction Matrix")
        .open(open)
        .default_pos(egui::pos2(screen_w * 0.55, 50.0))
        .resizable(true)
        .show(ctx, |ui| {
            if ui
                .button("Randomize Matrix")
                .on_hover_text(
                    "Fill the attraction matrix with random values; particles are not moved",
                )
                .clicked()
            {
                randomize = true;
            }

            let n = sim.species_count;
            let col_width = if n <= 8 { 36.0 } else { 22.0 };

            egui::ScrollArea::horizontal()
                .id_salt("matrix_window_scroll")
                .show(ui, |ui| {
                    egui::Grid::new("matrix_window_grid")
                        .min_col_width(col_width)
                        .show(ui, |ui| {
                            ui.label("");
                            for j in 0..n {
                                ui.colored_label(
                                    species_color(j, &sim.palette),
                                    format!("S{}", j + 1),
                                );
                            }
                            ui.end_row();

                            for i in 0..n {
                                ui.colored_label(
                                    species_color(i, &sim.palette),
                                    format!("S{}", i + 1),
                                );
                                for j in 0..n {
                                    let v = sim.attraction[i * MAX_SPECIES + j];
                                    let bg = attraction_cell_color(v);
                                    egui::Frame::new()
                                        .fill(bg)
                                        .inner_margin(egui::Margin::same(2))
                                        .show(ui, |ui| {
                                            ui.visuals_mut().widgets.inactive.weak_bg_fill =
                                                egui::Color32::TRANSPARENT;
                                            let r = ui.add(
                                                egui::DragValue::new(
                                                    &mut sim.attraction[i * MAX_SPECIES + j],
                                                )
                                                .range(-1.0_f32..=1.0_f32)
                                                .speed(0.01)
                                                .custom_formatter(|n, _| {
                                                    if n >= 0.0 {
                                                        format!(" {n:.4}")
                                                    } else {
                                                        format!("{n:.4}")
                                                    }
                                                })
                                                .custom_parser(|s| s.trim().parse().ok()),
                                            );
                                            if r.changed() {
                                                sim.mark_attraction_dirty();
                                            }
                                        });
                                }
                                ui.end_row();
                            }

                            if sim.border_mode == 3 {
                                ui.colored_label(egui::Color32::GRAY, "Wall");
                                for j in 0..n {
                                    let bg = attraction_cell_color(
                                        sim.attraction[MAX_SPECIES * MAX_SPECIES + j],
                                    );
                                    egui::Frame::new()
                                        .fill(bg)
                                        .inner_margin(egui::Margin::same(2))
                                        .show(ui, |ui| {
                                            ui.visuals_mut().widgets.inactive.weak_bg_fill =
                                                egui::Color32::TRANSPARENT;
                                            let r = ui.add(
                                                egui::DragValue::new(
                                                    &mut sim.attraction
                                                        [MAX_SPECIES * MAX_SPECIES + j],
                                                )
                                                .range(-1.0_f32..=1.0_f32)
                                                .speed(0.01)
                                                .custom_formatter(|n, _| {
                                                    if n >= 0.0 {
                                                        format!(" {n:.4}")
                                                    } else {
                                                        format!("{n:.4}")
                                                    }
                                                })
                                                .custom_parser(|s| s.trim().parse().ok()),
                                            );
                                            if r.changed() {
                                                sim.mark_attraction_dirty();
                                            }
                                        });
                                }
                                ui.end_row();
                            }
                        });
                });
        });

    randomize
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
#[allow(clippy::too_many_arguments)]
pub fn draw_perf_overlay(
    ctx: &egui::Context,
    frame_times: &VecDeque<f32>,
    sim: &SimulationState,
    quick_bench: &benchmark::QuickBench,
    runner: &mut benchmark::BenchmarkRunner,
    capacity: &mut benchmark::CapacityBench,
    vsync: bool,
    vsync_managed: bool,
    vsync_available: bool,
    per_species_count: &[usize],
) -> BenchmarkPanelResponse {
    let mut resp = BenchmarkPanelResponse {
        start: false,
        export_csv: false,
        start_quick: false,
        start_capacity: false,
        export_capacity_csv: false,
        cancel: false,
        cancel_capacity: false,
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

    let grid_w = ((2.0 / sim.r_max_normalised()) as usize).max(5);
    let n_cells = grid_w * grid_w;
    let density = sim.particle_count as f32 / n_cells as f32;

    egui::Window::new("Performance")
        .anchor(egui::Align2::RIGHT_TOP, [-10.0, 10.0])
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

                    let r_max_norm = sim.r_max_normalised();
                    let est_neighbors = (sim.particle_count as f32
                        * std::f32::consts::PI
                        * r_max_norm
                        * r_max_norm) as u32;

                    ui.label("Density").on_hover_text(
                        "Particles per pixel² of world area and estimated average neighbours \
                         per particle.  Both stay constant when auto-density is on.",
                    );
                    ui.label(format!(
                        "{:.4} p/px²  ~{est_neighbors} nbrs",
                        sim.density()
                    ));
                    ui.end_row();

                    ui.label("r_max (GPU)").on_hover_text(
                        "Effective interaction radius sent to the GPU: r_max × 720 / world_height. \
                         At the default world (height 720) this equals the slider value. \
                         Shrinks as the world grows, keeping the physical reach constant.",
                    );
                    ui.label(format!("{r_max_norm:.4}"));
                    ui.end_row();

                    if sim.auto_density && sim.perf_auto {
                        let avg_fps = 1.0 / avg_dt;
                        let at_limit = sim.perf_at_limit();
                        let on_target = avg_fps >= sim.perf_target_fps * 0.95;
                        let (label, color, tip) = if at_limit && !on_target {
                            (
                                "GPU limited",
                                egui::Color32::from_rgb(255, 160, 60),
                                "The world is at the maximum size where physics still improve. \
                                 The target FPS is unachievable at the current particle count — \
                                 lower the target or reduce particles.",
                            )
                        } else if on_target {
                            (
                                "On target",
                                egui::Color32::from_rgb(100, 220, 100),
                                "Auto-performance has converged; FPS is within 5% of the target.",
                            )
                        } else {
                            (
                                "Converging",
                                egui::Color32::from_rgb(120, 180, 255),
                                "Auto-performance is adjusting world size every ~2 s toward the target FPS.",
                            )
                        };
                        ui.label("Auto-perf");
                        ui.colored_label(color, label).on_hover_text(tip);
                        ui.end_row();
                    }
                });

            if !per_species_count.is_empty() {
                ui.separator();
                egui::CollapsingHeader::new("Species")
                    .default_open(false)
                    .show(ui, |ui| {
                        let total = per_species_count.iter().sum::<usize>().max(1);
                        for (i, &count) in per_species_count.iter().enumerate() {
                            let frac = count as f32 / total as f32;
                            let color = species_color(i, &sim.palette);
                            ui.horizontal(|ui| {
                                // Species label in its own color; count in the default text
                                // color so it stays readable regardless of the species hue.
                                ui.colored_label(color, format!("S{}:", i + 1));
                                ui.label(format!("{count} ({:.0}%)", frac * 100.0));
                            });
                        }
                    });
            }

            ui.separator();

            // Global vsync toggle (Quick Bench follows this setting)
            if vsync_managed {
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "VSync (managed)"))
                    .on_hover_text(
                        "VSync is disabled while Auto-performance is active — the FPS controller \
                         needs to see true GPU frame times, not the monitor refresh rate.  \
                         Your preference will be restored when Auto-performance is turned off.",
                    );
            } else if vsync_available {
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
                    .on_hover_text(
                        "VSync toggle requires PresentMode::Immediate support from the adapter",
                    );
            }

            ui.separator();

            // Quick bench
            if let Some((elapsed, total, is_warmup)) = quick_bench.progress() {
                let phase = if is_warmup { "Warmup" } else { "Collecting" };
                ui.label(format!("{phase}…"));
                ui.add(egui::ProgressBar::new(elapsed / total).show_percentage());
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
                    "Measure average FPS at the current particle count (5s warmup + 15s collection)",
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

                    ui.label(
                        "Runs every preset × particle-count combination at fixed 1280×720 \
                         (5s warmup + 15s collection each). ~5 minutes for 16 combos.",
                    );
                    ui.add_space(4.0);

                    if runner.is_running() {
                        if let Some((done, total, elapsed, target, is_warmup)) = runner.progress() {
                            let phase = if is_warmup { "Warmup" } else { "Collecting" };
                            let preset_name = crate::config::builtin_presets()
                                [benchmark::BenchmarkRunner::combo_preset_idx(done)]
                                .name
                                .clone();
                            let particles = benchmark::BENCHMARK_TIERS
                                [benchmark::BenchmarkRunner::combo_tier_idx(done)]
                                .particles;
                            ui.label(format!(
                                "Combo {}/{} — {} — {}",
                                done + 1,
                                total,
                                preset_name,
                                fmt_particles(particles),
                            ));
                            ui.label(format!("{phase} ({:.1}/{:.0}s)", elapsed, target));
                            ui.add(egui::ProgressBar::new(elapsed / target).show_percentage());
                            ui.add_space(2.0);
                            let overall =
                                (done as f32 + (elapsed / target).min(1.0)) / total as f32;
                            ui.label(format!("Overall: combo {}/{}", done + 1, total));
                            ui.add(egui::ProgressBar::new(overall).show_percentage());
                        }
                        ui.add_space(2.0);
                        if ui
                            .button("Cancel")
                            .on_hover_text("Stop the suite and discard partial results")
                            .clicked()
                        {
                            resp.cancel = true;
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

            ui.separator();

            // Capacity benchmark: binary search for max particles at target FPS
            let any_bench_running =
                runner.is_running() || quick_bench.is_running() || capacity.is_running();
            egui::CollapsingHeader::new("Capacity Benchmark")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        "Binary-searches for the maximum particle count that sustains a target \
                         FPS at fixed 1280×720. Takes ~4 minutes for 4 presets.",
                    );
                    ui.add_space(4.0);

                    if capacity.is_running() {
                        if let Some(p) = capacity.progress() {
                            let preset_name =
                                crate::config::builtin_presets()[p.preset_idx].name.clone();
                            let phase = if p.is_warmup { "Warmup" } else { "Collecting" };
                            ui.label(format!(
                                "Preset {}/{} — {} — iter {}/{}",
                                p.preset_idx + 1,
                                p.total_presets,
                                preset_name,
                                p.iter + 1,
                                p.max_iters,
                            ));
                            ui.label(format!("Testing {} particles — {phase}", fmt_particles(p.particles)));
                            ui.add(
                                egui::ProgressBar::new(p.elapsed / p.target_secs)
                                    .show_percentage(),
                            );
                            ui.add_space(2.0);
                            let step_frac = (p.elapsed / p.target_secs).min(1.0);
                            let overall = (p.preset_idx as f32 * p.max_iters as f32
                                + p.iter as f32
                                + step_frac)
                                / (p.total_presets as f32 * p.max_iters as f32);
                            ui.label(format!("Overall: preset {}/{}", p.preset_idx + 1, p.total_presets));
                            ui.add(egui::ProgressBar::new(overall).show_percentage());
                        }
                        ui.add_space(2.0);
                        if ui
                            .button("Cancel")
                            .on_hover_text("Stop the search and discard partial results")
                            .clicked()
                        {
                            resp.cancel_capacity = true;
                        }
                    } else if capacity.is_done() && !capacity.results.is_empty() {
                        ui.label(format!("Results — target {:.0} fps:", capacity.target_fps));
                        egui::Grid::new("cap_results_grid")
                            .num_columns(3)
                            .striped(true)
                            .min_col_width(55.0)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("Preset").strong());
                                ui.label(egui::RichText::new("Max particles").strong());
                                ui.label(egui::RichText::new("Achieved fps").strong());
                                ui.end_row();
                                for r in &capacity.results {
                                    ui.label(&r.preset_name);
                                    if r.max_particles == 0 {
                                        ui.label("< 1,000")
                                            .on_hover_text("Target not achievable even at minimum");
                                        ui.label("—");
                                    } else if r.capped {
                                        ui.label(format!("≥ {:>9}", fmt_particles(r.max_particles)))
                                            .on_hover_text(
                                                "GPU can sustain the target even at the 2M particle limit",
                                            );
                                        ui.label(format!("{:.0}", r.achieved_fps));
                                    } else {
                                        ui.label(format!("{:>9}", fmt_particles(r.max_particles)));
                                        ui.label(format!("{:.0}", r.achieved_fps));
                                    }
                                    ui.end_row();
                                }
                            });
                        ui.add_space(4.0);
                        if ui
                            .button("Export CSV…")
                            .on_hover_text("Save capacity results to a CSV file")
                            .clicked()
                        {
                            resp.export_capacity_csv = true;
                        }
                        ui.add_space(2.0);
                    }

                    if !capacity.is_running() {
                        // Target FPS setting (disabled during run)
                        ui.horizontal(|ui| {
                            ui.label("Target FPS:");
                            ui.add(
                                egui::DragValue::new(&mut capacity.target_fps)
                                    .range(10.0..=240.0)
                                    .speed(1.0)
                                    .suffix(" fps"),
                            );
                        });
                        ui.add_space(2.0);
                        let btn = ui
                            .add_enabled(
                                !any_bench_running,
                                egui::Button::new("Start Capacity Bench"),
                            )
                            .on_hover_text(
                                "Binary-search each preset for the highest particle count that \
                                 sustains the target FPS (adaptive warmup 5–20s + 5s collect; Ecosystem waits for cluster to stabilise)",
                            );
                        if btn.clicked() {
                            resp.start_capacity = true;
                        }
                    }
                });
        });

    resp
}

fn fmt_particles(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ── About window ──────────────────────────────────────────────────────────────

/// Draw the About window showing version info, repo link, and a button to open
/// the third-party licenses file.
pub fn draw_about_window(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }

    egui::Window::new("About Particle Life")
        .open(open)
        .default_size([360.0, 220.0])
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading(egui::RichText::new("Particle Life").size(22.0));
                ui.label(egui::RichText::new("Version 0.5.0").weak());
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Author: Brandon Bisel");
                ui.separator();
                ui.label("License: MIT");
            });
            ui.add_space(4.0);
            ui.hyperlink_to(
                "View on GitHub",
                "https://github.com/brandonbisel/particle-life",
            );

            ui.add_space(8.0);

            if ui.button("Open Third-Party Licenses").clicked() {
                // Embedded at compile time so the correct licenses always ship with the binary.
                const LICENSES_HTML: &str = include_str!("../THIRD_PARTY_LICENSES.html");
                let path = std::env::temp_dir().join("particle_life_licenses.html");
                if std::fs::write(&path, LICENSES_HTML).is_ok() {
                    let _ = open::that(&path);
                }
            }
        });
}
