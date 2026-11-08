use checking_fuel::AppState;
use eframe::egui;
use egui::{Color32, RichText};

const ERROR_BACKGROUND_COLOR: Color32 = Color32::from_rgb(50, 30, 30);
const WARNING_BACKGROUND_COLOR: Color32 = Color32::from_rgb(50, 45, 30);

pub fn render_errors_tab(ui: &mut egui::Ui, state: &mut AppState) {
    ui.horizontal(|ui| {
        ui.heading(RichText::new("Parsing Errors").size(24.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("Clear All").size(14.0)).clicked() {
                state.clear_errors();
            }
        });
    });
    ui.add_space(8.0);

    ui.label(
        RichText::new("This page shows parsing and data errors that occur during operation.")
            .size(14.0)
            .color(Color32::GRAY),
    );
    ui.add_space(16.0);

    if state.parse_errors.is_empty() {
        egui::Frame::none()
            .fill(Color32::from_rgb(30, 40, 30))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("✓")
                            .size(24.0)
                            .color(Color32::from_rgb(100, 255, 100)),
                    );
                    ui.label(
                        RichText::new("No errors recorded")
                            .size(16.0)
                            .color(Color32::from_rgb(150, 255, 150)),
                    );
                });
            });
        return;
    }

    // Error count summary
    ui.horizontal(|ui| {
        ui.label(
            super::num(
                format!("Total errors: {}", state.parse_errors.len()),
                16.0,
                Color32::from_rgb(255, 150, 150),
            )
            .strong(),
        );
    });
    ui.add_space(8.0);

    // Scrollable error list (newest first)
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (idx, error) in state.parse_errors.iter().rev().enumerate() {
                let bg_color = if error.error_type == "Warning" {
                    WARNING_BACKGROUND_COLOR
                } else {
                    ERROR_BACKGROUND_COLOR
                };

                egui::Frame::none()
                    .fill(bg_color)
                    .rounding(4.0)
                    .inner_margin(12.0)
                    .outer_margin(egui::Margin::symmetric(0.0, 4.0))
                    .stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(80, 50, 50)))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // The loop starts at the newest error, so count down.
                            let error_num = state.parse_errors.len() - idx;
                            ui.label(
                                RichText::new(format!("#{}", error_num))
                                    .size(12.0)
                                    .color(Color32::GRAY),
                            );

                            // Timestamp
                            ui.label(
                                RichText::new(error.timestamp.format("%H:%M:%S%.3f").to_string())
                                    .size(12.0)
                                    .color(Color32::from_rgb(150, 150, 200)),
                            );

                            // Source
                            ui.label(
                                RichText::new(format!("[{}]", error.source))
                                    .size(12.0)
                                    .strong()
                                    .color(Color32::from_rgb(255, 200, 100)),
                            );

                            // Error type
                            ui.label(
                                RichText::new(format!("({})", error.error_type))
                                    .size(12.0)
                                    .color(Color32::from_rgb(200, 150, 150)),
                            );
                        });

                        ui.add_space(4.0);

                        // Error message
                        ui.label(
                            RichText::new(&error.message)
                                .size(13.0)
                                .color(Color32::WHITE),
                        );

                        // Raw data preview if available
                        if let Some(ref preview) = error.raw_data_preview {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Data: ").size(11.0).color(Color32::GRAY));
                                ui.label(
                                    RichText::new(preview)
                                        .size(11.0)
                                        .family(egui::FontFamily::Monospace)
                                        .color(Color32::from_rgb(150, 150, 150)),
                                );
                            });
                        }
                    });
            }
        });
}
