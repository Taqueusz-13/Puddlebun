//! Stage-1 control panel (temporary — stage 2 replaces it with the glass synth panel).

use crate::camera::{CamMode, CameraRig};
use crate::settings::Settings;
use egui::{Color32, RichText};

#[derive(Default, Clone)]
pub struct Stats {
    pub fps: f32,
    pub frame_ms: f32,
    pub active_drops: u32,
    pub impacts_per_s: f32,
    pub audible_per_s: f32,
    pub voices: u32,
    pub intensity: f32,
    pub wind: f32,
    pub puddle: f32,
    pub shower: f32,
    pub lights: u32,
    pub ripple_steps: u32,
    pub audio_device: String,
    pub gpu: String,
}

#[derive(Default)]
pub struct UiActions {
    pub screenshot: bool,
    pub reset: bool,
}

pub fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgba_unmultiplied(14, 16, 22, 215);
    visuals.panel_fill = Color32::from_rgba_unmultiplied(14, 16, 22, 215);
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.window_stroke = egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(160, 190, 255, 40));
    ctx.set_visuals(visuals);
}

fn slider(ui: &mut egui::Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, label: &str) -> bool {
    ui.add(egui::Slider::new(v, range).text(label)).changed()
}

pub fn draw(ctx: &egui::Context, s: &mut Settings, stats: &Stats, cam: &mut CameraRig, hint_alpha: f32) -> UiActions {
    let mut act = UiActions::default();

    if hint_alpha > 0.0 {
        egui::Area::new(egui::Id::unique("puddlebun-hint"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -28.0))
            .interactable(false)
            .show(ctx, |ui| {
                let a = (hint_alpha * 255.0) as u8;
                ui.label(
                    RichText::new("Tab  controls    F  free-fly    1–6  viewpoints    N  next    M  mute    F11  fullscreen    F12  screenshot")
                        .color(Color32::from_rgba_unmultiplied(220, 228, 255, a))
                        .size(14.0),
                );
            });
    }

    if s.show_stats && !s.show_panel {
        egui::Area::new(egui::Id::unique("puddlebun-stats"))
            .anchor(egui::Align2::LEFT_TOP, egui::vec2(14.0, 12.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{:.0} fps  {:.2} ms   drops {}   impacts {:.0}/s   voices {}   {:.1} mm/h",
                        stats.fps, stats.frame_ms, stats.active_drops, stats.impacts_per_s, stats.voices, stats.intensity
                    ))
                    .color(Color32::from_rgba_unmultiplied(220, 228, 255, 200))
                    .monospace(),
                );
            });
    }

    if !s.show_panel {
        return act;
    }

    egui::Window::new(RichText::new("Puddlebun").strong())
        .default_pos(egui::pos2(18.0, 18.0))
        .default_width(330.0)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            ui.label(RichText::new("Stage 1 controls — the glass synth panel arrives in stage 2").small().weak());
            ui.add_space(4.0);

            egui::CollapsingHeader::new("Rain & wind").default_open(true).show(ui, |ui| {
                ui.add(egui::Slider::new(&mut s.intensity_mm_h, 0.1..=120.0).logarithmic(true).text("rain (mm/h)"));
                slider(ui, &mut s.wind_speed, 0.0..=20.0, "wind (m/s)");
                slider(ui, &mut s.wind_dir_deg, 0.0..=360.0, "wind direction (°)");
                slider(ui, &mut s.gustiness, 0.0..=1.0, "gustiness");
                ui.checkbox(&mut s.living_weather, "Living weather (drifting showers)");
                ui.label(RichText::new(format!(
                    "now: {:.1} mm/h · wind {:.1} m/s{}",
                    stats.intensity,
                    stats.wind,
                    if stats.shower > 0.05 { format!(" · shower {:.0}%", stats.shower * 100.0) } else { String::new() }
                )).small().weak());
            });

            egui::CollapsingHeader::new("Street").default_open(true).show(ui, |ui| {
                slider(ui, &mut s.wetness, 0.0..=1.0, "wetness");
                ui.add_enabled(!s.auto_puddles, egui::Slider::new(&mut s.puddle_level, 0.0..=1.0).text("puddles"));
                ui.checkbox(&mut s.auto_puddles, "Puddles fill and dry by themselves");
                slider(ui, &mut s.fog, 0.0..=1.0, "mist");
                slider(ui, &mut s.lamp_kelvin, 1800.0..=6000.0, "lamp colour (K)");
                slider(ui, &mut s.lamp_intensity, 0.0..=3.0, "lamp brightness");
                slider(ui, &mut s.window_lights, 0.0..=1.0, "windows lit");
                slider(ui, &mut s.ripple_strength, 0.0..=2.5, "ripple strength");
            });

            egui::CollapsingHeader::new("Sound").default_open(true).show(ui, |ui| {
                ui.checkbox(&mut s.muted, "Mute (M)");
                slider(ui, &mut s.master_volume, 0.0..=1.5, "master");
                slider(ui, &mut s.near_volume, 0.0..=2.0, "nearby drops");
                slider(ui, &mut s.far_volume, 0.0..=2.0, "distant rain");
                slider(ui, &mut s.reverb, 0.0..=1.0, "street echo");
            });

            egui::CollapsingHeader::new("Camera & look").default_open(false).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for i in 0..cam.shots.len() {
                        let sel = cam.current == i && cam.mode == CamMode::Drift;
                        if ui.selectable_label(sel, format!("{} {}", i + 1, cam.shots[i].name)).clicked() {
                            cam.go_to(i);
                        }
                    }
                });
                let free = cam.mode == CamMode::Free;
                if ui.selectable_label(free, "Free-fly (F): WASD, hold right mouse to look, Shift = fast, Space/C = up/down").clicked() {
                    cam.toggle_free();
                }
                ui.checkbox(&mut s.drift, "Drift to the next viewpoint automatically");
                ui.add_enabled(s.drift, egui::Slider::new(&mut s.drift_interval_s, 20.0..=1800.0).logarithmic(true).text("every (s)"));
                slider(ui, &mut s.exposure_ev, -3.0..=3.0, "exposure (EV)");
                slider(ui, &mut s.bloom, 0.0..=2.5, "bloom");
                let mut shutter_ms = s.shutter * 1000.0;
                if ui.add(egui::Slider::new(&mut shutter_ms, 2.0..=40.0).text("shutter (ms)")).changed() {
                    s.shutter = shutter_ms / 1000.0;
                }
                slider(ui, &mut s.sky_glow, 0.0..=3.0, "town glow");
            });

            egui::CollapsingHeader::new("Quality").default_open(false).show(ui, |ui| {
                let mut p = s.max_particles as f32 / 1000.0;
                if ui.add(egui::Slider::new(&mut p, 50.0..=3000.0).logarithmic(true).text("max drops (k) — restart")).changed() {
                    s.max_particles = (p * 1000.0) as u32;
                }
                slider(ui, &mut s.reflection_scale, 0.25..=1.0, "reflection resolution");
                ui.checkbox(&mut s.show_stats, "Show stats overlay when panel is hidden");
            });

            ui.separator();
            ui.label(RichText::new(format!(
                "{:.0} fps · {:.2} ms · {} drops · {:.0} impacts/s · {:.0} voiced/s · {} voices · {} lights",
                stats.fps, stats.frame_ms, stats.active_drops, stats.impacts_per_s, stats.audible_per_s, stats.voices, stats.lights
            )).small().monospace());
            ui.label(RichText::new(format!("{}  ·  {}", stats.gpu, stats.audio_device)).small().weak());
            ui.horizontal(|ui| {
                if ui.button("Screenshot (F12)").clicked() {
                    act.screenshot = true;
                }
                if ui.button("Reset all").clicked() {
                    act.reset = true;
                }
            });
        });
    act
}
