use eframe::egui;
use egui::{Color32, RichText};
use egui_plot::{Line, Plot, PlotPoints, VLine};
use checking_fuel::AppState;

/// Width of the caption column in the current frame grid. It fits the longest
/// caption ("DriverCarFuelMaxLtr (SDK):"), so that the values align.
const DEBUG_LABEL_COL_WIDTH: f32 = 190.0;

pub fn render_debug_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    color_low: Color32,
    color_avg: Color32,
    color_push: Color32,
    color_max: Color32,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.heading(RichText::new("Debug: Fuel Estimations Over Time").size(24.0));
            ui.add_space(8.0);

            // Show the fuel capacity section first. Its data is available immediately.
            render_fuel_capacity_debug(ui, state);

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(12.0);

            if state.lap_snapshots.is_empty() {
                ui.label(
                    RichText::new("Complete some laps to see fuel estimation charts.")
                        .size(16.0)
                        .color(Color32::GRAY),
                );
                return;
            }

            // One point for each lap snapshot.
            let mut low_points = Vec::new();
            let mut avg_points = Vec::new();
            let mut push_points = Vec::new();
            let mut max_points = Vec::new();
            let mut fuel_points = Vec::new();
            let mut fuel_used_points = Vec::new();
            let mut yellow_laps = Vec::new();

            for s in state.lap_snapshots.iter() {
                let x = s.lap_number as f64;

                if let Some(val) = s.low_estimate {
                    low_points.push([x, val as f64]);
                }
                if let Some(val) = s.avg_estimate {
                    avg_points.push([x, val as f64]);
                }
                if let Some(val) = s.push_estimate {
                    push_points.push([x, val as f64]);
                }
                if let Some(val) = s.max_estimate {
                    max_points.push([x, val as f64]);
                }

                fuel_points.push([x, s.fuel_level_at_start as f64]);

                if let Some(fuel_used) = s.fuel_used {
                    fuel_used_points.push([x, fuel_used as f64]);
                }

                if s.was_yellow_lap {
                    yellow_laps.push(x);
                }
            }

            let low_points: PlotPoints = low_points.into();
            let avg_points: PlotPoints = avg_points.into();
            let push_points: PlotPoints = push_points.into();
            let max_points: PlotPoints = max_points.into();
            let fuel_points: PlotPoints = fuel_points.into();
            let fuel_used_points: PlotPoints = fuel_used_points.into();

            // Fuel estimations plot
            ui.label(
                RichText::new(format!(
                    "Fuel Per Lap Estimations ({} laps)",
                    state.lap_snapshots.len()
                ))
                .size(18.0)
                .strong(),
            );
            Plot::new("fuel_estimations")
                .height(250.0)
                .legend(egui_plot::Legend::default())
                .x_axis_label("Lap")
                .y_axis_label("L/Lap")
                .show(ui, |plot_ui| {
                    // Gray line: the fuel that each lap used.
                    plot_ui.line(
                        Line::new(fuel_used_points)
                            .color(Color32::GRAY)
                            .width(2.0)
                            .name("Actual"),
                    );

                    plot_ui.line(Line::new(low_points).color(color_low).name("LOW"));
                    plot_ui.line(Line::new(avg_points).color(color_avg).name("AVG"));
                    plot_ui.line(Line::new(push_points).color(color_push).name("PUSH"));
                    plot_ui.line(Line::new(max_points).color(color_max).name("MAX"));

                    // Add yellow flag indicators for yellow laps
                    for x in &yellow_laps {
                        plot_ui.vline(VLine::new(*x).color(Color32::YELLOW).width(2.0));
                    }
                });

            ui.add_space(12.0);

            // Fuel level plot
            ui.label(RichText::new("Fuel Level at Lap Start").size(18.0).strong());
            Plot::new("fuel_level")
                .height(250.0)
                .legend(egui_plot::Legend::default())
                .x_axis_label("Lap")
                .y_axis_label("Liters")
                .show(ui, |plot_ui| {
                    plot_ui.line(
                        Line::new(fuel_points)
                            .color(Color32::from_rgb(100, 200, 100))
                            .name("Fuel Level"),
                    );

                    // Add yellow flag indicators for yellow laps
                    for x in &yellow_laps {
                        plot_ui.vline(VLine::new(*x).color(Color32::YELLOW).width(2.0));
                    }
                });

            ui.add_space(12.0);

            // Stats summary
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "Total laps recorded: {}",
                        state.lap_snapshots.len()
                    ))
                    .size(14.0)
                    .color(Color32::LIGHT_GRAY),
                );
                ui.add_space(16.0);
                let yellow_count = state
                    .lap_snapshots
                    .iter()
                    .filter(|s| s.was_yellow_lap)
                    .count();
                if yellow_count > 0 {
                    ui.label(
                        RichText::new(format!("Yellow flag laps: {}", yellow_count))
                            .size(14.0)
                            .color(Color32::YELLOW),
                    );
                }
            });
        }); // ScrollArea
}

fn render_fuel_capacity_debug(ui: &mut egui::Ui, state: &AppState) {
    // Fuel Capacity Debug Section
    ui.label(
        RichText::new("Fuel Capacity Debug (for partial fuel leagues)")
            .size(18.0)
            .strong(),
    );
    ui.add_space(8.0);

    // Current frame values
    egui::Frame::none()
        .fill(Color32::from_rgb(30, 35, 40))
        .inner_margin(12.0)
        .rounding(4.0)
        .show(ui, |ui| {
            ui.label(RichText::new("Current Frame Values:").size(14.0).strong());
            ui.add_space(4.0);

            // Use a fixed caption width. An automatic grid gets its width from
            // the widest cell in the current frame. Some rows below are not in
            // all frames, so the column width would change.
            egui::Grid::new("fuel_debug_current")
                .num_columns(2)
                .spacing([20.0, 4.0])
                .min_col_width(DEBUG_LABEL_COL_WIDTH)
                .show(ui, |ui| {
                    ui.label("FuelLevel (L):");
                    ui.label(
                        RichText::new(format!("{:.3}", state.latest_frame.fuel_level))
                            .color(Color32::WHITE)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();

                    ui.label("FuelLevelPct (raw):");
                    ui.label(
                        RichText::new(format!("{:.6}", state.latest_frame.fuel_level_pct))
                            .color(Color32::WHITE)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();

                    ui.label("FuelLevelPct (%):");
                    ui.label(
                        RichText::new(format!("{:.2}%", state.latest_frame.fuel_level_pct * 100.0))
                            .color(Color32::LIGHT_BLUE)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();

                    ui.label("DriverCarFuelMaxLtr (SDK):");
                    ui.label(
                        RichText::new(format!("{:.3} L", state.latest_frame.fuel_max_ltr))
                            .color(Color32::YELLOW)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();

                    ui.label("Calculated Capacity:");
                    let calculated = if state.latest_frame.fuel_level_pct > 0.001 {
                        state.latest_frame.fuel_level / state.latest_frame.fuel_level_pct
                    } else {
                        0.0
                    };
                    ui.label(
                        RichText::new(format!("{:.3} L", calculated))
                            .color(Color32::GREEN)
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();

                    ui.label("Used Capacity (fuel_capacity):");
                    ui.label(
                        RichText::new(format!("{:.3} L", state.latest_frame.fuel_capacity))
                            .color(Color32::from_rgb(255, 150, 255))
                            .family(egui::FontFamily::Monospace),
                    );
                    ui.end_row();

                    // Show the values if they are different.
                    let diff = (state.latest_frame.fuel_max_ltr - calculated).abs();
                    if diff > 1.0 && calculated > 0.0 {
                        ui.label(RichText::new("WARNING:").color(Color32::RED).strong());
                        ui.label(
                            RichText::new(format!(
                                "SDK max ({:.1}L) differs from calculated ({:.1}L) by {:.1}L - possible partial fuel!",
                                state.latest_frame.fuel_max_ltr,
                                calculated,
                                diff
                            ))
                            .color(Color32::RED),
                        );
                        ui.end_row();
                    }
                });
        });

    ui.add_space(12.0);

    // Historical fuel capacity values per lap
    if !state.lap_snapshots.is_empty() {
        ui.label(RichText::new("Fuel Capacity Per Lap:").size(14.0).strong());
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(200.0)
            .id_salt("fuel_capacity_history")
            .show(ui, |ui| {
                egui::Grid::new("fuel_debug_history")
                    .num_columns(5)
                    .spacing([15.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        // Header
                        ui.label(RichText::new("Lap").strong());
                        ui.label(RichText::new("FuelLevelPct").strong());
                        ui.label(RichText::new("SDK Max (L)").strong());
                        ui.label(RichText::new("Calculated (L)").strong());
                        ui.label(RichText::new("Used (L)").strong());
                        ui.end_row();

                        for snap in state.lap_snapshots.iter().rev().take(50) {
                            ui.label(format!("{}", snap.lap_number));
                            ui.label(
                                RichText::new(format!("{:.4}", snap.fuel_level_pct))
                                    .family(egui::FontFamily::Monospace),
                            );
                            ui.label(
                                RichText::new(format!("{:.1}", snap.fuel_max_ltr))
                                    .family(egui::FontFamily::Monospace)
                                    .color(Color32::YELLOW),
                            );
                            let calculated = if snap.fuel_level_pct > 0.001 {
                                snap.fuel_level_at_start / snap.fuel_level_pct
                            } else {
                                0.0
                            };
                            ui.label(
                                RichText::new(format!("{:.1}", calculated))
                                    .family(egui::FontFamily::Monospace)
                                    .color(Color32::GREEN),
                            );
                            ui.label(
                                RichText::new(format!("{:.1}", snap.fuel_capacity_calculated))
                                    .family(egui::FontFamily::Monospace)
                                    .color(Color32::from_rgb(255, 150, 255)),
                            );
                            ui.end_row();
                        }
                    });
            });
    }
}
