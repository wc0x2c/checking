use eframe::egui;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};
use egui_plot::{Line, Plot, PlotPoints, VLine};
use checking_fuel::AppState;

// Color constants
const AVG_BACKGROUND_COLOR: Color32 = Color32::from_rgb(30, 50, 60);
const PUSH_BACKGROUND_COLOR: Color32 = Color32::from_rgb(60, 30, 30);
const LOW_BACKGROUND_COLOR: Color32 = Color32::from_rgb(30, 60, 30);
const MAX_BACKGROUND_COLOR: Color32 = Color32::from_rgb(50, 20, 40);
const WARNING_BACKGROUND_COLOR: Color32 = Color32::from_rgb(80, 20, 20);
const SUCCESS_BACKGROUND_COLOR: Color32 = Color32::from_rgb(20, 60, 30);
const CAUTION_BACKGROUND_COLOR: Color32 = Color32::from_rgb(60, 50, 20);
const NEUTRAL_BACKGROUND_COLOR: Color32 = Color32::from_rgb(40, 40, 40);
const CHART_BACKGROUND_COLOR: Color32 = Color32::from_rgb(25, 25, 30);
const CHART_STROKE_COLOR: Color32 = Color32::from_rgb(60, 60, 70);

const HEADER_TEXT_COLOR: Color32 = Color32::from_rgb(200, 200, 220);
const SUCCESS_TEXT_LIGHT_COLOR: Color32 = Color32::from_rgb(150, 255, 170);
const WARNING_TEXT_LIGHT_COLOR: Color32 = Color32::from_rgb(255, 200, 200);
const CAUTION_TEXT_COLOR: Color32 = super::CAUTION_TEXT_COLOR;
const PUSH_TEXT_COLOR: Color32 = Color32::from_rgb(255, 150, 150);
const LOW_TEXT_COLOR: Color32 = Color32::from_rgb(150, 255, 150);
const MAX_TEXT_COLOR: Color32 = Color32::from_rgb(255, 100, 200);
const CAUTION_TEXT_LIGHT_COLOR: Color32 = Color32::from_rgb(255, 220, 120);

const SUCCESS_BORDER_COLOR: Color32 = Color32::from_rgb(100, 255, 120);
const CAUTION_BORDER_COLOR: Color32 = Color32::from_rgb(255, 200, 50);
const WARNING_BORDER_COLOR: Color32 = Color32::from_rgb(255, 80, 80);

const FUEL_LEVEL_BAR_COLOR: Color32 = Color32::from_rgb(100, 200, 100);

// Fixed layout heights. Each section of this tab keeps its space when it has no data. Thus, when
// telemetry arrives during a race, the text on the page changes but nothing moves. See the sizing
// notes in `mod.rs`.

/// The lapped driver warning. The space is kept also when the car is on the lead lap.
const LAPS_DOWN_WARNING_HEIGHT: f32 = 58.0;

/// The fuel use plot. Its "waiting for lap data" placeholder has the same height.
const CHART_HEIGHT: f32 = 220.0;

/// The bottom row of panels (stint timeline and lap table). The height is fixed and does not come
/// from `available_height`, so that height changes in the sections above do not change it.
const BOTTOM_PANEL_HEIGHT: f32 = 260.0;

/// Height of the header above the lap table. The table scroll height is the remaining height.
const TABLE_HEADER_ALLOWANCE: f32 = 40.0;
const LAP_PROGRESS_BAR_COLOR: Color32 = Color32::from_rgb(100, 150, 255);

const PLOT_DARK_PUSH_COLOR: Color32 = Color32::from_rgb(80, 25, 25);
const PLOT_DARK_LOW_COLOR: Color32 = Color32::from_rgb(30, 80, 35);

const CARD_BACKGROUND_COLOR: Color32 = super::CARD_BACKGROUND_COLOR;

/// One strategy row in the stint timeline.
struct StintRow {
    label: String,
    fuel_per_lap: f32,
    color: Color32,
    /// The lap of each pit stop, in sequence.
    pit_laps: Vec<i32>,
    stops: i32,
    can_finish: bool,
    /// The lap range in which the first stop can move, if there is one.
    window: Option<(i32, i32)>,
    /// True for the calculated budget row: the highest fuel rate that keeps the plan. The chart
    /// shows it as a target, not as a strategy that you use.
    is_budget: bool,
}

/// Stint timeline: the pit laps for each fuel strategy.
///
/// The driver usually wants to know how much more pace is possible before it costs a stop. The
/// answer is a comparison between rows. On a shared lap axis, the answer is the row where the
/// number of segments changes. This is easier to see than in a column of stop counts.
fn render_stint_timeline(ui: &mut egui::Ui, rows: &[StintRow], total_laps: i32, current_lap: f32) {
    use egui::{pos2, Align2, FontId, Rect, Sense, Stroke, Vec2};

    if total_laps <= 0 {
        return;
    }

    const LABEL_W: f32 = 96.0;
    const TRAILER_W: f32 = 86.0;
    const ROW_H: f32 = 26.0;
    const BAR_H: f32 = 16.0;
    const AXIS_H: f32 = 18.0;
    /// Four rates (LOW, AVG, PUSH, MAX) and the budget row. A row is not drawn when its strategy
    /// has no outcome. The height uses this constant and not `rows.len()`, so that the chart does
    /// not grow by one row at a time when data arrives.
    const MAX_ROWS: usize = 5;

    let full_w = ui.available_width().min(800.0);
    let height = MAX_ROWS as f32 * ROW_H + AXIS_H;
    let (resp, painter) = ui.allocate_painter(Vec2::new(full_w, height), Sense::hover());
    let origin = resp.rect.min;

    let track_x0 = origin.x + LABEL_W;
    let track_w = (full_w - LABEL_W - TRAILER_W).max(10.0);
    // Lap number to x. The axis is for the full race, and all rows use it. The chart is only useful
    // with one shared axis.
    let lap_x = |lap: f32| track_x0 + track_w * (lap / total_laps as f32).clamp(0.0, 1.0);

    let font = FontId::proportional(11.0);
    let dim = Color32::from_rgb(120, 120, 135);

    for (i, row) in rows.iter().enumerate() {
        let y = origin.y + i as f32 * ROW_H;
        let mid = y + ROW_H * 0.5;

        // Label column: the name and its fuel rate.
        painter.text(
            pos2(origin.x, mid),
            Align2::LEFT_CENTER,
            &row.label,
            font.clone(),
            row.color,
        );
        painter.text(
            pos2(origin.x + LABEL_W - 8.0, mid),
            Align2::RIGHT_CENTER,
            format!("{:.3}", row.fuel_per_lap),
            font.clone(),
            dim,
        );

        let bar_top = mid - BAR_H * 0.5;

        // The pit window, shaded behind the stints: the range in which the first stop can move if
        // there is a caution.
        if let Some((from, to)) = row.window {
            let (x0, x1) = (lap_x(from as f32), lap_x(to as f32));
            if x1 - x0 >= 1.0 {
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, y + 1.0), pos2(x1, y + ROW_H - 1.0)),
                    2.0,
                    row.color.gamma_multiply(0.18),
                );
            }
        }

        if !row.can_finish {
            // No stints to draw: the plan does not get to the flag.
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(track_x0, bar_top),
                    pos2(track_x0 + track_w, bar_top + BAR_H),
                ),
                3.0,
                WARNING_BACKGROUND_COLOR,
            );
            painter.text(
                pos2(track_x0 + track_w * 0.5, mid),
                Align2::CENTER_CENTER,
                "CANNOT FINISH",
                font.clone(),
                WARNING_TEXT_LIGHT_COLOR,
            );
        } else {
            // Stint segments: from the current position to each pit lap, and then to the flag.
            let mut edges = vec![current_lap];
            edges.extend(row.pit_laps.iter().map(|l| *l as f32));
            edges.push(total_laps as f32);

            for pair in edges.windows(2) {
                let (x0, x1) = (lap_x(pair[0]), lap_x(pair[1]));
                // A gap of 1 px at each pit lap shows the stints as separate segments, without a
                // divider line.
                let seg =
                    Rect::from_min_max(pos2(x0 + 1.0, bar_top), pos2(x1 - 1.0, bar_top + BAR_H));
                if seg.width() <= 0.0 {
                    continue;
                }
                let fill = if row.is_budget {
                    row.color.gamma_multiply(0.35)
                } else {
                    row.color.gamma_multiply(0.75)
                };
                painter.rect_filled(seg, 3.0, fill);

                // Stint length in laps. It shows in the segment if it fits.
                let laps = pair[1] - pair[0];
                if seg.width() > 34.0 {
                    painter.text(
                        seg.center(),
                        Align2::CENTER_CENTER,
                        format!("{:.0}", laps),
                        font.clone(),
                        Color32::from_rgb(20, 20, 25),
                    );
                }
            }

            // Pit markers at the stint boundaries.
            for lap in &row.pit_laps {
                let x = lap_x(*lap as f32);
                painter.line_segment(
                    [pos2(x, bar_top - 3.0), pos2(x, bar_top + BAR_H + 3.0)],
                    Stroke::new(2.0, CAUTION_BORDER_COLOR),
                );
            }
        }

        // At the end of the row: the stop count, which the driver compares.
        let stops_text = if !row.can_finish {
            "—".to_string()
        } else if row.stops == 0 {
            "NO STOP".to_string()
        } else if row.stops == 1 {
            "1 STOP".to_string()
        } else {
            format!("{} STOPS", row.stops)
        };
        painter.text(
            pos2(track_x0 + track_w + 8.0, mid),
            Align2::LEFT_CENTER,
            stops_text,
            font.clone(),
            row.color,
        );
    }

    // Lap axis at the bottom, and a mark for the current lap. Its position uses the fixed row count
    // and not the number of rows drawn, so that it does not move when strategies show or do not
    // show.
    let axis_y = origin.y + MAX_ROWS as f32 * ROW_H + 2.0;
    painter.line_segment(
        [pos2(track_x0, axis_y), pos2(track_x0 + track_w, axis_y)],
        Stroke::new(1.0, CHART_STROKE_COLOR),
    );
    // Approximately one tick each 10 laps. On long races the step is larger, so that the ticks do
    // not overlap.
    let step = (total_laps as f32 / 8.0).ceil().max(1.0) as i32;
    let step = [1, 2, 5, 10, 20, 25, 50, 100]
        .into_iter()
        .find(|s| *s >= step)
        .unwrap_or(step);
    let mut lap = 0;
    while lap <= total_laps {
        let x = lap_x(lap as f32);
        painter.line_segment(
            [pos2(x, axis_y), pos2(x, axis_y + 3.0)],
            Stroke::new(1.0, CHART_STROKE_COLOR),
        );
        painter.text(
            pos2(x, axis_y + 5.0),
            Align2::CENTER_TOP,
            format!("{lap}"),
            FontId::proportional(9.0),
            dim,
        );
        lap += step;
    }
    let cx = lap_x(current_lap);
    painter.line_segment(
        [pos2(cx, origin.y), pos2(cx, axis_y)],
        Stroke::new(1.0, SUCCESS_BORDER_COLOR),
    );
}

pub fn render_fuel_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    color_low: Color32,
    color_avg: Color32,
    color_push: Color32,
    color_max: Color32,
    color_pit: Color32,
    sim_results: Option<&[super::CachedSimResult; 4]>, // [LOW, AVG, PUSH, MAX]
    extra_liters: f32,
    fuel_tank_pct: Option<f32>,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // FUEL TAB CONTENT
            //
            // All fuel and pit calculations come from `strategy::analyze`. The overlays and the
            // strategy banner below use the same function. Read the values from
            // `analysis`, and do not calculate them again here.
            let laps_down = state.laps_down;
            let pit_sv_fuel = state.latest_frame.pit_sv_fuel;
            let total_laps = state.get_effective_total_laps();

            let fuel_capacity =
                super::get_effective_fuel_capacity(state.get_fuel_capacity(), fuel_tank_pct);
            let analysis = crate::strategy::analyze(state, extra_liters, fuel_capacity);

            let rates = crate::strategy::Rates::from_state(state, total_laps);
            let avg_10_green = rates.avg;
            let avg_5_push = rates.push;
            let low_5_green = rates.low;
            let max_single = rates.max;

            // Outcome for each strategy, found by the labels that `analyze` sets. `None` when the
            // estimate has no data.
            let outcome = |label: &str| analysis.outcomes.iter().find(|o| o.label == label);
            let out_low = outcome("LOW");
            let out_avg = outcome("AVG");
            let out_push = outcome("PUSH");
            let out_max = outcome("MAX");

            // Lap counts for the display.
            let avg_lap_count = ((total_laps as f32 * 0.10).ceil() as usize).max(5);
            let push_lap_count = ((total_laps as f32 * 0.05).ceil() as usize).max(3);
            let low_lap_count = ((total_laps as f32 * 0.05).ceil() as usize).max(3);

            // Calculate the remaining laps outside the closure.
            let avg_str = avg_10_green.map_or("N/A".to_string(), |v| format!("{:.3}L", v));
            let push_str = avg_5_push.map_or("N/A".to_string(), |v| format!("{:.3}L", v));
            let low_str = low_5_green.map_or("N/A".to_string(), |v| format!("{:.3}L", v));
            let max_str = max_single.map_or("N/A".to_string(), |v| format!("{:.3}L", v));

            let laps_of = |o: Option<&crate::strategy::StrategyOutcome>| {
                o.map_or(0.0, |o| o.laps_on_current_fuel)
            };
            let laps_avg = laps_of(out_avg);
            let laps_push = laps_of(out_push);
            let laps_low = laps_of(out_low);
            let laps_max = laps_of(out_max);

            let laps_to_go = analysis.laps_to_go;

            // Fuel to add at each stop, for the strategy markers on the FUEL TO ADD card.
            let fill_of = |o: Option<&crate::strategy::StrategyOutcome>| {
                o.and_then(|o| o.fuel_at_first_stop).unwrap_or(0.0)
            };
            let fuel_add_avg = fill_of(out_avg);
            let fuel_add_push = fill_of(out_push);
            let fuel_add_low = fill_of(out_low);
            let fuel_add_max = fill_of(out_max);

            // Current status cards in a grid.
            //
            // All cards are in one row that does not wrap. Here `horizontal_wrapped` does not break
            // between children with a fixed size. It makes all of them as tall as the tallest one.
            // Thus the seven cards have sizes that fit together in the default window width of 1400
            // px.
            ui.horizontal(|ui| {
                // Fuel Level card.
                let fuel_pct = if fuel_capacity > 0.0 {
                    (state.latest_frame.fuel_level / fuel_capacity).min(1.0)
                } else {
                    0.0
                };
                super::render_metric_card(
                    ui,
                    "FUEL LEVEL",
                    format!("{:.2} L", state.latest_frame.fuel_level),
                    Color32::WHITE,
                    Some((
                        fuel_pct,
                        format!(
                            "{:.1}L / {:.1}L",
                            state.latest_frame.fuel_level, fuel_capacity
                        ),
                        FUEL_LEVEL_BAR_COLOR,
                    )),
                );

                ui.add_space(8.0);

                // Laps Remaining (fractional).
                let current_lap_fractional = state.get_current_lap_fractional();
                let laps_remaining_fractional =
                    (total_laps as f32 - current_lap_fractional).max(0.0);
                let lap_progress = state.latest_frame.lap_dist_pct;
                super::render_metric_card(
                    ui,
                    "LAPS REMAINING",
                    if state.laps_down > 0 {
                        format!(
                            "{:.2} / {} (-{})",
                            laps_remaining_fractional, total_laps, state.laps_down
                        )
                    } else {
                        format!("{:.2} / {}", laps_remaining_fractional, total_laps)
                    },
                    Color32::WHITE,
                    Some((
                        lap_progress,
                        format!("Lap {:.0}%", lap_progress * 100.0),
                        LAP_PROGRESS_BAR_COLOR,
                    )),
                );

                ui.add_space(8.0);

                // Fuel to Add.
                let color = if pit_sv_fuel > fuel_add_max {
                    color_max
                } else if pit_sv_fuel > fuel_add_push {
                    color_push
                } else if pit_sv_fuel > fuel_add_avg {
                    color_avg
                } else if pit_sv_fuel > fuel_add_low {
                    color_low
                } else {
                    Color32::GRAY
                };

                super::render_fuel_to_add_card(
                    ui,
                    pit_sv_fuel,
                    state.latest_frame.fuel_level,
                    fuel_capacity,
                    fuel_add_low,
                    fuel_add_avg,
                    fuel_add_push,
                    fuel_add_max,
                    &super::StrategyColors {
                        current: color,
                        low: color_low,
                        avg: color_avg,
                        push: color_push,
                        max: color_max,
                    },
                    low_5_green.unwrap_or(0.0),
                    avg_10_green.unwrap_or(0.0),
                    avg_5_push.unwrap_or(0.0),
                    max_single.unwrap_or(0.0),
                    laps_low,
                    laps_avg,
                    laps_push,
                    laps_max,
                );

            });

            ui.add_space(8.0);

            // The four strategy cards have a separate row. When each card is wide enough for its
            // longest line, the seven cards together are wider than the default window. In one row,
            // the last card was clipped, or with `horizontal_wrapped` all cards became taller.
            ui.horizontal(|ui| {
                // One function makes the four cards. The overlay LAPS LEFT cards use a different
                // function, because they show the fuel margin and do not show the pit line.
                for (label, outcome, color, pit_color) in [
                    ("LAPS LEFT (AVG)", out_avg, color_avg, CAUTION_TEXT_COLOR),
                    ("LAPS LEFT (PUSH)", out_push, color_push, PUSH_TEXT_COLOR),
                    ("LAPS LEFT (LOW)", out_low, color_low, LOW_TEXT_COLOR),
                    ("LAPS LEFT (MAX)", out_max, color_max, MAX_TEXT_COLOR),
                ] {
                    ui.add_space(8.0);
                    egui::Frame::none()
                        .fill(CARD_BACKGROUND_COLOR)
                        .rounding(8.0)
                        .inner_margin(16.0)
                        .stroke(egui::Stroke::new(2.0, color))
                        .show(ui, |ui| {
                            super::render_laps_left_card(
                                ui,
                                label,
                                outcome,
                                color,
                                pit_color,
                                super::LAPS_LEFT_SIZE_TAB,
                                extra_liters,
                            );
                        });
                }
            });

            ui.add_space(8.0);

            // Fuel rates with lap counts.
            ui.horizontal(|ui| {
                ui.label(RichText::new("Fuel/Lap:").size(14.0).color(Color32::GRAY));
                // Each value is monospace, so that when a rate changes or a lap count gets one more
                // digit, the items on its right do not move.
                for (text, color) in [
                    (
                        format!("LOW: {low_str} ({low_lap_count} laps)"),
                        color_low,
                    ),
                    (
                        format!("AVG: {avg_str} ({avg_lap_count} laps)"),
                        color_avg,
                    ),
                    (
                        format!("PUSH: {push_str} ({push_lap_count} laps)"),
                        color_push,
                    ),
                    (format!("MAX: {max_str} (1 lap)"), color_max),
                ] {
                    ui.add_space(8.0);
                    ui.label(super::num(text, 14.0, color));
                }
            });

            ui.add_space(12.0);

            // Warning for lapped drivers. The row keeps its space also when the car is on the lead
            // lap. Thus, when the warning shows during a race, the sections below do not move.
            super::reserved_row(ui, LAPS_DOWN_WARNING_HEIGHT, |ui| {
                if laps_down > 0 {
                    egui::Frame::none()
                        .fill(WARNING_BACKGROUND_COLOR)
                        .rounding(8.0)
                        .inner_margin(12.0)
                        .stroke(egui::Stroke::new(2.0, Color32::RED))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("⚠").size(24.0).color(Color32::RED));
                                ui.label(
                                    super::num(
                                        format!(
                                    "DOWN {laps_down} LAP(S) - Race ends when LEADER finishes! ({laps_to_go:.0} laps to go)"
                                        ),
                                        16.0,
                                        WARNING_TEXT_LIGHT_COLOR,
                                    )
                                    .strong(),
                                );
                            });
                        });
                }
            });
            ui.add_space(8.0);

            // The strategy result. This is the only part of the app that tells the driver what to
            // do, and not only the numbers. The refuel values come from the outer `refuel_*`
            // variables, which include the margin, so that they agree with the cards.
            if avg_10_green.is_some() && avg_5_push.is_some() {
                let will_make_avg = laps_avg >= laps_to_go;
                let will_make_push = laps_push >= laps_to_go;

                let (icon, text, bg_color, border_color, text_color) =
                    if will_make_avg && will_make_push {
                        (
                            "",
                            "PUSH FUEL OK",
                            SUCCESS_BACKGROUND_COLOR,
                            SUCCESS_BORDER_COLOR,
                            SUCCESS_TEXT_LIGHT_COLOR,
                        )
                    } else if will_make_avg {
                        (
                            "⚠",
                            "PUSH MARGINAL",
                            CAUTION_BACKGROUND_COLOR,
                            CAUTION_BORDER_COLOR,
                            CAUTION_TEXT_LIGHT_COLOR,
                        )
                    } else {
                        (
                            "✗",
                            "REFUEL REQUIRED",
                            WARNING_BACKGROUND_COLOR,
                            WARNING_BORDER_COLOR,
                            PUSH_TEXT_COLOR,
                        )
                    };

                egui::Frame::none()
                    .fill(bg_color)
                    .rounding(8.0)
                    .inner_margin(16.0)
                    .stroke(egui::Stroke::new(3.0, border_color))
                    .show(ui, |ui| {
                        ui.set_min_height(super::STATUS_BANNER_HEIGHT);
                        ui.horizontal(|ui| {
                            // The icon is "", "⚠" or "✗", and these have different widths. A slot
                            // with a fixed width keeps the text next to it in the same position.
                            ui.allocate_ui(
                                egui::vec2(super::STATUS_ICON_SLOT, super::STATUS_BANNER_HEIGHT),
                                |ui| {
                                    ui.label(RichText::new(icon).size(28.0).color(border_color));
                                },
                            );
                            ui.label(RichText::new(text).size(20.0).strong().color(text_color));
                        });
                    });
            } else {
                egui::Frame::none()
                    .fill(NEUTRAL_BACKGROUND_COLOR)
                    .rounding(8.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        // The same height as the banner above, so that the first strategy result
                        // does not move the page.
                        ui.set_min_height(super::STATUS_BANNER_HEIGHT);
                        ui.label(
                            RichText::new("⏳ Collecting lap data...")
                                .size(20.0)
                                .color(Color32::GRAY),
                        );
                    });
            }

            ui.add_space(12.0);

            // Fuel use chart.
            let filtered_lap_data = state.get_filtered_fuel_data();
            if !filtered_lap_data.is_empty() {
                egui::Frame::none()
                    .fill(CHART_BACKGROUND_COLOR)
                    .rounding(8.0)
                    .inner_margin(12.0)
                    .stroke(egui::Stroke::new(1.0, CHART_STROKE_COLOR))
                    .show(ui, |ui| {
                        ui.set_min_height(CHART_HEIGHT);
                        ui.label(
                            RichText::new("📈 FUEL USAGE PER LAP")
                                .size(16.0)
                                .strong()
                                .color(HEADER_TEXT_COLOR),
                        );
                        ui.add_space(4.0);

                        let points: PlotPoints = filtered_lap_data
                            .iter()
                            .map(|(lap, fuel)| [*lap as f64, *fuel as f64])
                            .collect();

                        let line = Line::new(points)
                            .color(Color32::GRAY)
                            .width(2.0)
                            .name("Fuel/Lap");

                        Plot::new("fuel_plot")
                            .height(280.0)
                            .legend(
                                egui_plot::Legend::default().position(egui_plot::Corner::LeftTop),
                            )
                            .show_axes([true, true])
                            .show_grid([true, true])
                            .x_axis_formatter(|val, _range| format!("{}", val.value as i32))
                            .show(ui, |plot_ui| {
                                // Main fuel line.
                                plot_ui.line(line);

                                // Get the lap range one time, and not 4 times.
                                let (min_lap, max_lap) = if !state.lap_data.is_empty() {
                                    (
                                        state.lap_data.first().map(|l| l.lap_number).unwrap_or(1),
                                        state.lap_data.last().map(|l| l.lap_number).unwrap_or(1),
                                    )
                                } else {
                                    (1, 1)
                                };

                                // Strategy lines.
                                if let Some(low) = low_5_green {
                                    let low_points: PlotPoints = vec![
                                        [min_lap as f64, low as f64],
                                        [max_lap as f64, low as f64],
                                    ]
                                    .into();
                                    plot_ui.line(
                                        Line::new(low_points)
                                            .color(color_low)
                                            .width(2.5)
                                            .style(egui_plot::LineStyle::dashed_loose())
                                            .name(format!("LOW {:.3}L", low)),
                                    );
                                }

                                if let Some(avg) = avg_10_green {
                                    let avg_points: PlotPoints = vec![
                                        [min_lap as f64, avg as f64],
                                        [max_lap as f64, avg as f64],
                                    ]
                                    .into();
                                    plot_ui.line(
                                        Line::new(avg_points)
                                            .color(color_avg)
                                            .width(2.5)
                                            .style(egui_plot::LineStyle::dashed_loose())
                                            .name(format!("AVG {:.3}L", avg)),
                                    );
                                }

                                if let Some(push) = avg_5_push {
                                    let push_points: PlotPoints = vec![
                                        [min_lap as f64, push as f64],
                                        [max_lap as f64, push as f64],
                                    ]
                                    .into();
                                    plot_ui.line(
                                        Line::new(push_points)
                                            .color(color_push)
                                            .width(2.5)
                                            .style(egui_plot::LineStyle::dashed_loose())
                                            .name(format!("PUSH {:.3}L", push)),
                                    );
                                }

                                if let Some(max) = max_single {
                                    let max_points: PlotPoints = vec![
                                        [min_lap as f64, max as f64],
                                        [max_lap as f64, max as f64],
                                    ]
                                    .into();
                                    plot_ui.line(
                                        Line::new(max_points)
                                            .color(color_max)
                                            .width(2.5)
                                            .style(egui_plot::LineStyle::dashed_loose())
                                            .name(format!("MAX {:.3}L", max)),
                                    );
                                }

                                // Pit stop vertical lines.
                                let pit_stops = state.get_pit_stop_laps();
                                for pit_lap in pit_stops {
                                    plot_ui.vline(
                                        VLine::new(pit_lap as f64)
                                            .color(color_pit)
                                            .width(3.0)
                                            .name("⚑ PIT"),
                                    );
                                }

                                // Colored markers on the bottom axis for the strategy laps.
                                let (_avg_laps, push_laps, low_laps) =
                                    state.get_strategy_lap_assignments(total_laps);

                                // PUSH lap markers on the bottom axis.
                                for lap in &push_laps {
                                    let lap_x = *lap as f64;
                                    // A much darker version of the PUSH color.
                                    let dark_push = PLOT_DARK_PUSH_COLOR; // Dark red
                                    plot_ui.vline(
                                        VLine::new(lap_x).color(dark_push).width(3.0).name(""),
                                    );
                                }

                                // The chart does not show the AVG laps, because there are too many
                                // markers.

                                // LOW lap markers.
                                for lap in &low_laps {
                                    let lap_x = *lap as f64;
                                    // A much darker version of the LOW color.
                                    let dark_low = PLOT_DARK_LOW_COLOR; // Dark green
                                    plot_ui.vline(
                                        VLine::new(lap_x).color(dark_low).width(3.0).name(""),
                                    );
                                }
                            });
                    });
            } else {
                egui::Frame::none()
                    .fill(CHART_BACKGROUND_COLOR)
                    .rounding(8.0)
                    .inner_margin(40.0)
                    .show(ui, |ui| {
                        // The same height as the plot, so that the panels below do not move when
                        // data arrives.
                        ui.set_min_height(CHART_HEIGHT);
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new("⏳ Waiting for lap data...")
                                    .size(18.0)
                                    .color(Color32::GRAY),
                            );
                        });
                    });
            }

            ui.add_space(12.0);

            // Bottom panel: stint timeline (left) and lap table (right). The height is fixed and
            // does not use `available_height`. `available_height` is measured after all sections
            // above, so a height change above would change these columns.
            ui.horizontal(|ui| {
                // Left panel: stint timeline, with the pit laps for each strategy.
                ui.vertical(|ui| {
                    ui.set_width(800.0);
                    ui.set_height(BOTTOM_PANEL_HEIGHT);
                    if let (true, Some(_)) = (state.current_lap > 0, avg_10_green) {
                        ui.label(
                            RichText::new(format!("STINT TIMELINE ({:.1}L tank)", fuel_capacity))
                                .size(16.0)
                                .strong()
                                .color(HEADER_TEXT_COLOR),
                        );
                        ui.add_space(4.0);

                        let current_fuel = state.latest_frame.fuel_level;
                        let current_lap_fractional = state.get_current_lap_fractional();

                        // One row for each fuel rate, and the calculated budget row. All values
                        // come from `analysis.outcomes`, so that the chart agrees with the cards
                        // above it.
                        let mut rows: Vec<StintRow> = Vec::new();
                        let colored = [
                            ("LOW", out_low, color_low, 0),
                            ("AVG", out_avg, color_avg, 1),
                            ("PUSH", out_push, color_push, 2),
                            ("MAX", out_max, color_max, 3),
                        ];

                        for (name, outcome, color, cache_idx) in colored {
                            let Some(o) = outcome else { continue };

                            // Pit laps for the full race, not only the first stop. A plan with more
                            // than one stop is the case where the timeline is most useful. Use the
                            // cached simulation of the frame if there is one, because it is for the
                            // same four rates.
                            let sim = match sim_results {
                                Some(cached) => cached[cache_idx].clone(),
                                None => checking_fuel::simulate_race_strategy(
                                    current_fuel,
                                    current_lap_fractional,
                                    total_laps,
                                    o.fuel_per_lap,
                                    fuel_capacity,
                                ),
                            };
                            let window = crate::strategy::compute_pit_window(
                                state,
                                o.fuel_per_lap,
                                fuel_capacity,
                                extra_liters,
                            )
                            .map(|w| (w.earliest_lap, w.latest_lap));

                            rows.push(StintRow {
                                label: name.to_string(),
                                fuel_per_lap: o.fuel_per_lap,
                                color,
                                pit_laps: sim.pit_details.iter().map(|(lap, _)| *lap).collect(),
                                stops: sim.num_stops,
                                can_finish: sim.can_finish,
                                window,
                                is_budget: false,
                            });
                        }

                        // The budget row: the highest fuel rate that keeps the reference stop
                        // count. The driver wants to know this rate. All pace between AVG and this
                        // rate costs no extra stop.
                        if let Some(budget) = analysis.max_sustainable.as_ref() {
                            let sim = checking_fuel::simulate_race_strategy(
                                current_fuel,
                                current_lap_fractional,
                                total_laps,
                                budget.fuel_per_lap,
                                fuel_capacity,
                            );
                            rows.push(StintRow {
                                // The name shows what the value is: a real limit, or the highest
                                // rate that a driver can hold when the real limit is too high.
                                label: if budget.unconstrained {
                                    "MAX SAFE".to_string()
                                } else {
                                    "BUDGET".to_string()
                                },
                                fuel_per_lap: budget.fuel_per_lap,
                                color: CAUTION_TEXT_LIGHT_COLOR,
                                pit_laps: sim.pit_details.iter().map(|(lap, _)| *lap).collect(),
                                stops: sim.num_stops,
                                can_finish: sim.can_finish,
                                window: None,
                                is_budget: true,
                            });
                        }

                        render_stint_timeline(ui, &rows, total_laps, current_lap_fractional);
                    }
                });

                ui.add_space(12.0);

                // Right panel: lap usage table.
                ui.vertical(|ui| {
                    ui.set_width(400.0);
                    ui.set_height(BOTTOM_PANEL_HEIGHT);
                    let (avg_laps, push_laps, low_laps) =
                        state.get_strategy_lap_assignments(total_laps);
                    if !state.lap_data.is_empty() {
                        ui.label(
                            RichText::new("📋 RECENT FUEL USAGE (Last 20 Strategy Laps)")
                                .size(16.0)
                                .strong()
                                .color(HEADER_TEXT_COLOR),
                        );
                        ui.add_space(4.0);

                        let mut used_lap_numbers: Vec<i32> = Vec::new();
                        used_lap_numbers.extend(&avg_laps);
                        used_lap_numbers.extend(&push_laps);
                        used_lap_numbers.extend(&low_laps);
                        used_lap_numbers.sort();
                        used_lap_numbers.dedup();

                        let laps_to_show: Vec<_> = state
                            .lap_data
                            .iter()
                            .filter(|lap| used_lap_numbers.contains(&lap.lap_number))
                            .rev()
                            .take(20)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect();

                        // The height comes from the fixed panel height, so that the table does not
                        // change size when rows arrive.
                        let table_height = BOTTOM_PANEL_HEIGHT - TABLE_HEADER_ALLOWANCE;

                        TableBuilder::new(ui)
                            .striped(true)
                            .vscroll(true)
                            .max_scroll_height(table_height)
                            .column(Column::exact(60.0))
                            .column(Column::exact(80.0))
                            .column(Column::exact(80.0))
                            .column(Column::remainder())
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.strong("Lap");
                                });
                                header.col(|ui| {
                                    ui.strong("Fuel (L)");
                                });
                                header.col(|ui| {
                                    ui.strong("Flags");
                                });
                                header.col(|ui| {
                                    ui.strong("Use");
                                });
                            })
                            .body(|mut body| {
                                for lap in laps_to_show {
                                    let flag_str = if lap.green_flag_count > lap.yellow_flag_count {
                                        "Green"
                                    } else {
                                        "Yellow"
                                    };

                                    let is_avg = avg_laps.contains(&lap.lap_number);
                                    let is_push = push_laps.contains(&lap.lap_number);
                                    let is_low = low_laps.contains(&lap.lap_number);

                                    let use_marker = match (is_avg, is_push, is_low) {
                                        (true, true, false) => "AVG+PUSH",
                                        (true, false, true) => "AVG+LOW",
                                        (true, false, false) => "AVG",
                                        (false, true, false) => "PUSH",
                                        (false, false, true) => "LOW",
                                        _ => "",
                                    };

                                    let color = if is_push {
                                        color_push
                                    } else if is_avg {
                                        color_avg
                                    } else if is_low {
                                        color_low
                                    } else {
                                        Color32::WHITE
                                    };

                                    body.row(20.0, |mut row| {
                                        row.col(|ui| {
                                            ui.colored_label(color, lap.lap_number.to_string());
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(
                                                color,
                                                format!("{:.3}", lap.fuel_used),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(color, flag_str);
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(color, use_marker);
                                        });
                                    });
                                }
                            });
                    }
                });
            });
        }); // End ScrollArea
}
