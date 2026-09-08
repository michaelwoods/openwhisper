use anyhow::Result;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::time::{Duration, Instant};

use super::{HudController, HudState};

pub fn run_hud_window(controller: HudController) -> Result<()> {
    #[cfg(target_os = "linux")]
    let event_loop_builder: Option<eframe::EventLoopBuilderHook> = Some(Box::new(|builder| {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        use winit::platform::x11::EventLoopBuilderExtX11;
        EventLoopBuilderExtWayland::with_any_thread(builder, true);
        EventLoopBuilderExtX11::with_any_thread(builder, true);
    }));

    #[cfg(not(target_os = "linux"))]
    let event_loop_builder = None;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenWhisper HUD")
            .with_app_id("net.local.openwhisper.hud")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_inner_size([290.0, 48.0])
            .with_taskbar(false)
            .with_active(false)
            .with_mouse_passthrough(true),
        event_loop_builder,
        ..Default::default()
    };

    let ctrl_init = controller.clone();
    eframe::run_native(
        "OpenWhisper HUD",
        native_options,
        Box::new(move |cc| {
            ctrl_init.set_ctx(cc.egui_ctx.clone());
            crate::gui::configure_fallback_fonts(&cc.egui_ctx);
            Ok(Box::new(HudApp::new(controller)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run HUD window: {e}"))
}

pub struct HudApp {
    controller: HudController,
    start_time: Instant,
    last_pos: Option<Pos2>,
}

impl HudApp {
    pub fn new(controller: HudController) -> Self {
        Self {
            controller,
            start_time: Instant::now(),
            last_pos: None,
        }
    }
}

impl eframe::App for HudApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Fully transparent window canvas
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.controller.set_ctx(ui.ctx().clone());

        let now = Instant::now();
        let (state, smoothed_rms, alpha, position) = {
            let lock = self.controller.model();
            let model = lock.read().unwrap();
            let alpha = model.calculate_alpha(now);
            (
                model.state.clone(),
                model.smoothed_rms,
                alpha,
                model.position,
            )
        };

        // Screen position calculation based on monitor size
        if let Some(monitor_size) = ui.ctx().input(|i| i.viewport().monitor_size)
            && monitor_size.x > 100.0
            && monitor_size.y > 100.0
        {
            let win_w = 290.0;
            let win_h = 48.0;
            let margin_y = 60.0;
            let margin_x = 40.0;
            let (x, y) = match position {
                crate::config::HudPosition::BottomCenter => (
                    (monitor_size.x - win_w) / 2.0,
                    monitor_size.y - win_h - margin_y,
                ),
                crate::config::HudPosition::TopCenter => ((monitor_size.x - win_w) / 2.0, margin_y),
                crate::config::HudPosition::BottomRight => (
                    monitor_size.x - win_w - margin_x,
                    monitor_size.y - win_h - margin_y,
                ),
                crate::config::HudPosition::TopRight => {
                    (monitor_size.x - win_w - margin_x, margin_y)
                }
            };
            let target_pos = Pos2::new(x, y);
            if self.last_pos != Some(target_pos) {
                self.last_pos = Some(target_pos);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::OuterPosition(target_pos));
            }
        }

        if alpha <= 0.01 {
            // Window is completely transparent/idle.
            // Do not schedule busy repaints when idle to save CPU/battery.
            // When user starts recording, `controller.set_recording()` wakes up egui via `request_repaint()`.
            return;
        }

        // Render floating pill container
        let rect = Rect::from_min_size(Pos2::new(4.0, 4.0), Vec2::new(282.0, 40.0));
        let painter = ui.painter();

        // Background color and subtle border glow
        let bg_color = Color32::from_rgba_unmultiplied(15, 23, 42, (230.0 * alpha) as u8);
        let border_stroke = match &state {
            HudState::Recording { .. } => Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(239, 68, 68, (140.0 * alpha) as u8),
            ),
            HudState::Transcribing { .. } => Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(56, 189, 248, (140.0 * alpha) as u8),
            ),
            HudState::Completed { .. } => Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(34, 197, 94, (140.0 * alpha) as u8),
            ),
            HudState::Error { .. } => Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(245, 158, 11, (140.0 * alpha) as u8),
            ),
            HudState::Idle => Stroke::NONE,
        };

        painter.rect(rect, 20.0, bg_color, border_stroke, StrokeKind::Inside);

        let center_y = rect.center().y;

        match state {
            HudState::Recording { started_at, .. } => {
                let elapsed = now.saturating_duration_since(started_at);
                let elapsed_secs = elapsed.as_secs();
                let elapsed_millis = (elapsed.subsec_millis()) / 100;
                let timer_text = format!(
                    "{:02}:{:02}.{}",
                    elapsed_secs / 60,
                    elapsed_secs % 60,
                    elapsed_millis
                );

                // Pulsing red recording dot
                let pulse = (now.saturating_duration_since(self.start_time).as_secs_f32() * 5.0)
                    .sin()
                    * 0.5
                    + 0.5;
                let glow_radius = 4.0 + pulse * 2.0;
                let dot_center = Pos2::new(rect.min.x + 18.0, center_y);

                painter.circle_filled(
                    dot_center,
                    glow_radius,
                    Color32::from_rgba_unmultiplied(239, 68, 68, (80.0 * alpha) as u8),
                );
                painter.circle_filled(
                    dot_center,
                    4.0,
                    Color32::from_rgba_unmultiplied(239, 68, 68, (255.0 * alpha) as u8),
                );

                // "Listening" label
                painter.text(
                    Pos2::new(rect.min.x + 30.0, center_y),
                    egui::Align2::LEFT_CENTER,
                    "Listening",
                    egui::FontId::proportional(13.0),
                    Color32::from_rgba_unmultiplied(241, 245, 249, (255.0 * alpha) as u8),
                );

                // Elapsed timer
                painter.text(
                    Pos2::new(rect.min.x + 115.0, center_y),
                    egui::Align2::LEFT_CENTER,
                    timer_text,
                    egui::FontId::monospace(12.0),
                    Color32::from_rgba_unmultiplied(148, 163, 184, (240.0 * alpha) as u8),
                );

                // 5-bar dynamic audio visualizer
                let bar_start_x = rect.max.x - 65.0;
                let bar_width = 3.5;
                let bar_spacing = 3.0;

                for i in 0..5 {
                    let phase = (now.saturating_duration_since(self.start_time).as_secs_f32()
                        * 9.0
                        + i as f32 * 0.85)
                        .sin()
                        * 0.5
                        + 0.5;
                    let height = 4.0 + (smoothed_rms * 16.0 * (0.6 + 0.4 * phase)).clamp(0.0, 18.0);
                    let bar_x = bar_start_x + i as f32 * (bar_width + bar_spacing);
                    let bar_rect = Rect::from_min_max(
                        Pos2::new(bar_x, center_y - height / 2.0),
                        Pos2::new(bar_x + bar_width, center_y + height / 2.0),
                    );

                    let bar_color = Color32::from_rgba_unmultiplied(
                        239,
                        68,
                        68,
                        ((140.0 + smoothed_rms * 115.0) * alpha) as u8,
                    );
                    painter.rect_filled(bar_rect, 1.5, bar_color);
                }
            }

            HudState::Transcribing { started_at } => {
                let elapsed = now.saturating_duration_since(started_at).as_secs_f32();
                let pulse = (elapsed * 6.0).sin() * 0.5 + 0.5;

                // Cyan acoustic dot
                let dot_center = Pos2::new(rect.min.x + 18.0, center_y);
                painter.circle_filled(
                    dot_center,
                    5.0 + pulse * 2.0,
                    Color32::from_rgba_unmultiplied(56, 189, 248, (80.0 * alpha) as u8),
                );
                painter.circle_filled(
                    dot_center,
                    4.0,
                    Color32::from_rgba_unmultiplied(56, 189, 248, (255.0 * alpha) as u8),
                );

                // "Transcribing..." text
                painter.text(
                    Pos2::new(rect.min.x + 30.0, center_y),
                    egui::Align2::LEFT_CENTER,
                    "Transcribing...",
                    egui::FontId::proportional(13.0),
                    Color32::from_rgba_unmultiplied(56, 189, 248, (255.0 * alpha) as u8),
                );

                // Wave progress dots
                let dots_start_x = rect.max.x - 55.0;
                for i in 0..4 {
                    let dot_pulse =
                        ((elapsed * 4.0 - i as f32 * 0.5).sin() * 0.5 + 0.5).clamp(0.2, 1.0);
                    let x = dots_start_x + i as f32 * 10.0;
                    painter.circle_filled(
                        Pos2::new(x, center_y),
                        2.5 * dot_pulse,
                        Color32::from_rgba_unmultiplied(
                            56,
                            189,
                            248,
                            (220.0 * dot_pulse * alpha) as u8,
                        ),
                    );
                }
            }

            HudState::Completed { text_preview, .. } => {
                // Crisp antialiased vector checkmark (zero font dependency)
                let check_color =
                    Color32::from_rgba_unmultiplied(34, 197, 94, (255.0 * alpha) as u8);
                let p_start = Pos2::new(rect.min.x + 13.0, center_y);
                let p_mid = Pos2::new(rect.min.x + 17.0, center_y + 4.0);
                let p_end = Pos2::new(rect.min.x + 23.0, center_y - 4.0);
                let stroke = Stroke::new(2.0, check_color);
                painter.line_segment([p_start, p_mid], stroke);
                painter.line_segment([p_mid, p_end], stroke);

                // Truncated preview text
                painter.text(
                    Pos2::new(rect.min.x + 32.0, center_y),
                    egui::Align2::LEFT_CENTER,
                    &text_preview,
                    egui::FontId::proportional(12.5),
                    Color32::from_rgba_unmultiplied(241, 245, 249, (240.0 * alpha) as u8),
                );
            }

            HudState::Error { message, .. } => {
                // Crisp antialiased vector warning sign (zero font dependency)
                let warn_color =
                    Color32::from_rgba_unmultiplied(245, 158, 11, (255.0 * alpha) as u8);
                let warn_center = Pos2::new(rect.min.x + 18.0, center_y);
                painter.circle_stroke(warn_center, 6.5, Stroke::new(1.5, warn_color));
                painter.line_segment(
                    [
                        Pos2::new(warn_center.x, warn_center.y - 3.0),
                        Pos2::new(warn_center.x, warn_center.y + 0.5),
                    ],
                    Stroke::new(1.6, warn_color),
                );
                painter.circle_filled(
                    Pos2::new(warn_center.x, warn_center.y + 3.0),
                    1.0,
                    warn_color,
                );

                let error_trunc = crate::notification::safe_truncate_chars(&message, 30);

                painter.text(
                    Pos2::new(rect.min.x + 32.0, center_y),
                    egui::Align2::LEFT_CENTER,
                    error_trunc,
                    egui::FontId::proportional(12.0),
                    Color32::from_rgba_unmultiplied(248, 113, 113, (240.0 * alpha) as u8),
                );
            }

            HudState::Idle => {}
        }

        // Animate at ~40 FPS while active
        ui.ctx().request_repaint_after(Duration::from_millis(25));
    }
}
