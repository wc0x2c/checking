mod debug;
mod errors;
mod fuel;
mod relative;
mod sender;

pub use debug::render_debug_tab;
pub use errors::render_errors_tab;
pub use fuel::render_fuel_tab;
pub use relative::render_relative_tab;
pub use sender::render_sender_tab;

use eframe::egui;
use egui::{Color32, RichText};

// Re-export checking-fuel functions for this crate.
pub use checking_fuel::{SimulationResult, compute_all_simulations, get_effective_fuel_capacity};

// Card colors for all pages. The Fuel tab and the overlay both render the LAPS LEFT card, so they
// must use the same colors.
pub const CARD_BACKGROUND_COLOR: Color32 = Color32::from_rgb(40, 40, 50);
pub const SUCCESS_TEXT_COLOR: Color32 = Color32::from_rgb(100, 255, 100);
pub const WARNING_TEXT_COLOR: Color32 = Color32::from_rgb(255, 100, 100);
pub const CAUTION_TEXT_COLOR: Color32 = Color32::from_rgb(255, 200, 100);

// The color of each fuel rate, and of the pit stop, on all pages and overlays.
pub const LOW_COLOR: Color32 = Color32::from_rgb(100, 255, 120);
pub const AVG_COLOR: Color32 = Color32::from_rgb(80, 200, 255);
pub const PUSH_COLOR: Color32 = Color32::from_rgb(255, 80, 80);
pub const MAX_COLOR: Color32 = Color32::from_rgb(255, 80, 180);
pub const PIT_COLOR: Color32 = Color32::from_rgb(255, 200, 50);

/// Font size for the large number on a LAPS LEFT card. The Fuel tab has many cards in one row. An
/// overlay has only one card, so its number can be larger.
pub const LAPS_LEFT_SIZE_TAB: f32 = 32.0;
pub const LAPS_LEFT_SIZE_OVERLAY: f32 = 42.0;

// ---------------------------------------------------------------------------
// Fixed sizing
//
// egui is immediate mode: each widget measures itself from its content in each frame. Telemetry
// repaints at 30 FPS. Thus a label changes size when its value gets one more digit, and a container
// changes height when an optional row shows or does not show. Then the layout moves. These helpers
// enforce one rule: the geometry comes from the layout, and never from the data.
//
// There are two causes, and the code must prevent both:
//   - Vertical: optional rows show and do not show. The fix is to always keep
//     their space (see `reserved_row`), and leave it blank when there is no data.
//   - Horizontal: the default font is proportional, so `1` is narrower than `0`.
//     A digit change alone changes the label size and moves the labels next to
//     it. The fix is to render live numbers in monospace (see `num`).
// ---------------------------------------------------------------------------

/// Height of one text row in a card. It is larger than the 14 pt body text of the cards, so an
/// empty reserved row has the same height as a full row.
pub const CARD_LINE_HEIGHT: f32 = 22.0;

/// Width of a LAPS LEFT card. It holds [`LAPS_LEFT_DETAIL_CHARS`] monospace characters at
/// [`LAPS_LEFT_DETAIL_SIZE`] without wrapping, with some extra space for font rounding. Four of
/// these cards are in their own row, which fits in the default window width of 1400 px.
pub const LAPS_LEFT_CARD_WIDTH: f32 = 260.0;

/// Font size for the pit line and the spare or short line.
///
/// It is smaller than the body text, because monospace is wider than the proportional font and
/// these lines do not wrap. The longest line has [`LAPS_LEFT_DETAIL_CHARS`] characters, and
/// [`LAPS_LEFT_CARD_WIDTH`] is set to hold it. If you increase this value and do not make the card
/// wider, the card size changes with the data. The test
/// `laps_left_card_is_the_same_size_in_every_data_state` finds this error.
pub const LAPS_LEFT_DETAIL_SIZE: f32 = 11.0;

/// Total inner height of a LAPS LEFT card: the title, the large value, the pit row and the spare or
/// short row. The space for the last two rows is always kept.
pub const LAPS_LEFT_CARD_HEIGHT: f32 = 108.0;

/// Width of a metric card. Monospace digits are wider than proportional digits. At 200 px, the
/// "LAPS REMAINING" value wrapped onto a second line and went outside the frame.
pub const METRIC_CARD_WIDTH: f32 = 250.0;

/// Inner height of a metric card: the title, the value and the progress bar row. The space for the
/// progress bar row is always kept, also when the caller gives no bar.
pub const METRIC_CARD_HEIGHT: f32 = 96.0;

/// Height of the progress bar row in a metric card. A `ProgressBar` with text is taller than a
/// plain text row, so this is not `CARD_LINE_HEIGHT`.
pub const PROGRESS_BAR_ROW_HEIGHT: f32 = 24.0;

/// Inner height of the Fuel tab status banner. The banner with data and the "collecting data"
/// banner both use this height, so that the content below does not move when one replaces the
/// other.
pub const STATUS_BANNER_HEIGHT: f32 = 36.0;

/// Square slot for a status icon. The icon is "", "⚠" or "✗", and these have different widths. A
/// slot with a fixed size keeps the text after it in the same position.
pub const STATUS_ICON_SLOT: f32 = 34.0;

/// Column width for the strategy grid on the FUEL TO ADD card. It fits the `{:.3}` L/Lap values,
/// which are the longest cells.
const STRATEGY_GRID_COL_WIDTH: f32 = 42.0;

/// A live numeric value in monospace, so that its width does not change when its digits change. Use
/// it for all values from telemetry. Use plain `RichText` for static labels, because their width
/// does not change.
pub fn num(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .size(size)
        .color(color)
        .family(egui::FontFamily::Monospace)
}

/// Draws `content` into a row of exactly `height`, or leaves the row blank.
///
/// The caller uses the same space in both cases. Thus a row that has no data in this frame does not
/// collapse and move all content below it up. Use this function and not `if let Some(..)` around a
/// `ui.label`.
///
/// The height is a maximum and a minimum. With only `set_min_size`, a long string can wrap onto a
/// second line and make the row taller, which also moves the layout. Content that is too long is
/// clipped, so callers must make the rows large enough for their longest string.
pub fn reserved_row(ui: &mut egui::Ui, height: f32, content: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    let size = egui::vec2(width, height);
    ui.allocate_ui(size, |ui| {
        ui.set_min_size(size);
        ui.set_max_size(size);
        ui.set_clip_rect(ui.max_rect());
        content(ui);
    });
}

/// `reserved_row` for a single optional label.
///
/// Wrapping is off, because a label that wraps changes its height, and this row must prevent that.
/// The caller must make the row wide enough for the label.
pub fn reserved_label(ui: &mut egui::Ui, height: f32, text: Option<RichText>) {
    reserved_row(ui, height, |ui| {
        if let Some(text) = text {
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
        }
    });
}

/// Number of characters in a LAPS LEFT detail line.
///
/// Each detail string is padded to this number of characters. In a monospace font, all strings then
/// have the same pixel width. Thus the card has one fixed size for all states. If the card width
/// came from the longest string, a new or changed message would change the card width without a
/// warning.
///
/// The value is the length of the longest line that the card can show with the largest possible
/// values: `"Spare: +999.9L (+999.9L over reserve)"`.
const LAPS_LEFT_DETAIL_CHARS: usize = 37;

/// Pads a detail line to [`LAPS_LEFT_DETAIL_CHARS`], so that all variants have the same width.
/// Longer strings are not truncated, because that can hide an important value. The layout test
/// fails instead.
fn pad_detail(text: String) -> String {
    format!("{text:<LAPS_LEFT_DETAIL_CHARS$}")
}

/// The value if it is available, or a dash if it is not. A table column with a value in each cell
/// does not change size when the values arrive.
fn dash_or(available: bool, value: impl FnOnce() -> String) -> String {
    if available { value() } else { "-".to_string() }
}

/// One LAPS LEFT card: the number of laps for the fuel of this strategy, when to pit and if the car
/// gets to the flag.
///
/// The four strategy cards on the Fuel tab use it. The overlay uses a different function
/// (`overlay::render_laps_left_overlay`). The overlay shows the fuel margin and no pit line,
/// because the strategy overlay tells the driver when to pit. This function takes a full
/// [`StrategyOutcome`], so that a new line on the card needs a change in one location only.
pub fn render_laps_left_card(
    ui: &mut egui::Ui,
    label: &str,
    outcome: Option<&crate::strategy::StrategyOutcome>,
    color: Color32,
    pit_text_color: Color32,
    value_size: f32,
    // The safety reserve from the config, so that the card can tell if its values include it. Use
    // 0.0 to not show this detail.
    reserve: f32,
) {
    // Fixed geometry. The Fuel tab draws this card four times next to each other, and the overlay
    // draws it again. All lines in it come from the data. Without a fixed width, the row of cards
    // changes its layout in almost each frame.
    ui.vertical(|ui| {
        // The height has a minimum and a maximum. The width has only a set value. The detail rows
        // fit without wrapping, so no content must be wider. A maximum width would truncate an
        // important value without a warning, and the test above would not find the overflow.
        ui.set_width(LAPS_LEFT_CARD_WIDTH);
        ui.set_min_height(LAPS_LEFT_CARD_HEIGHT);
        ui.set_max_height(LAPS_LEFT_CARD_HEIGHT);

        ui.label(RichText::new(label).size(12.0).color(Color32::GRAY));

        let laps = outcome.map_or(0.0, |o| o.laps_on_current_fuel);
        ui.label(num(format!("{laps:.1}"), value_size, color).strong());

        // The space for both remaining rows is always kept. Thus the card height is the same when
        // an outcome is not available.
        let (pit_text, margin_text) = match outcome {
            Some(outcome) => (
                laps_left_pit_text(outcome, pit_text_color),
                Some(laps_left_margin_text(outcome, reserve)),
            ),
            None => (None, None),
        };

        reserved_label(ui, CARD_LINE_HEIGHT, pit_text);
        reserved_label(ui, CARD_LINE_HEIGHT, margin_text);
    });
}

/// When to pit. If no stop is necessary but the fuel is still short, how much fuel to add.
///
/// `None` means that this strategy has nothing to show. The caller still keeps the row.
fn laps_left_pit_text(
    outcome: &crate::strategy::StrategyOutcome,
    pit_text_color: Color32,
) -> Option<RichText> {
    match outcome.pit_lap {
        Some(pit_lap) => Some(num(
            pad_detail(format!(
                "Pit L{pit_lap} | +{:.1}L",
                outcome.fuel_at_first_stop.unwrap_or(0.0),
            )),
            LAPS_LEFT_DETAIL_SIZE,
            pit_text_color,
        )),
        None if outcome.fuel_to_add_now > 0.0 => Some(num(
            pad_detail(format!("Fuel Add: +{:.1}L", outcome.fuel_to_add_now)),
            LAPS_LEFT_DETAIL_SIZE,
            pit_text_color,
        )),
        None => None,
    }
}

/// The surplus or shortfall line. It is a separate function, so that the card body is easy to read.
fn laps_left_margin_text(outcome: &crate::strategy::StrategyOutcome, reserve: f32) -> RichText {
    // Surplus or shortfall on the fuel in the car: can the car get to the flag without a stop. This
    // is not `fuel_margin`, which is the value at the end of the race after the stops and is
    // approximately 0 when there is a planned stop.
    //
    // When a reserve is set, the line shows both values, as the overlay does. "+4.7L spare" alone
    // does not tell if the reserve is already subtracted, and this changes what the driver does on
    // the last lap. `margin_without_stopping` does not include the reserve, so the gross value is
    // that value plus the reserve.
    if outcome.finishes_without_stopping {
        let text = if reserve > 0.0 {
            format!(
                "Spare: +{:.1}L (+{:.1}L over reserve)",
                outcome.margin_without_stopping + reserve,
                outcome.margin_without_stopping,
            )
        } else {
            format!("Spare: +{:.1}L", outcome.margin_without_stopping)
        };
        num(pad_detail(text), LAPS_LEFT_DETAIL_SIZE, SUCCESS_TEXT_COLOR)
    } else {
        let short = outcome.margin_without_stopping.abs();
        // A shortfall to the target does not mean that the fuel runs out. When the shortfall is
        // smaller than the reserve, the car still gets to the flag and uses part of the reserve. A
        // red "Short" would be a false alarm for a car that finishes.
        let (text, color) = if reserve > 0.0 && short <= reserve {
            (
                format!("Into reserve: {:.1}L left at flag", reserve - short),
                CAUTION_TEXT_COLOR,
            )
        } else if reserve > 0.0 {
            (
                format!("Short: {short:.1}L ({:.1}L to dry)", short - reserve),
                WARNING_TEXT_COLOR,
            )
        } else {
            (format!("Short: {short:.1}L"), WARNING_TEXT_COLOR)
        };
        num(pad_detail(text), LAPS_LEFT_DETAIL_SIZE, color)
    }
}

/// The name and color of each fuel rate, from the lowest fuel use to the highest.
const CARD_RATES: [(&str, Color32); 4] = [
    ("LOW", LOW_COLOR),
    ("AVG", AVG_COLOR),
    ("PUSH", PUSH_COLOR),
    ("MAX", MAX_COLOR),
];

/// The values of one fuel rate on the fuel to add card. A value of 0 means that the rate is not
/// known, and the card shows no marker and a dash.
#[derive(Debug, Clone, Copy)]
pub struct CardRate {
    /// Liters to add now to get to the finish at this rate.
    pub fuel_to_add: f32,
    pub fuel_per_lap: f32,
    /// Number of laps that the current fuel is sufficient for at this rate.
    pub laps_on_fuel: f32,
}

// Alias for backward compatibility.
pub type CachedSimResult = SimulationResult;

/// Renders the fuel to add card with a marker for each rate. `rates` are in the order LOW, AVG,
/// PUSH, MAX. The card has the color of the highest rate whose fuel to add is below the fuel that
/// the pit menu adds.
pub fn render_fuel_to_add_card(
    ui: &mut egui::Ui,
    pit_sv_fuel: f32,
    fuel_level: f32,
    fuel_capacity: f32,
    rates: &[CardRate; 4],
) {
    let current_color = rates
        .iter()
        .zip(CARD_RATES)
        .rev()
        .find(|(rate, _)| pit_sv_fuel > rate.fuel_to_add)
        .map_or(Color32::GRAY, |(_, (_, color))| color);

    egui::Frame::none()
        .fill(CARD_BACKGROUND_COLOR)
        .inner_margin(egui::Margin {
            left: 16.0,
            right: 16.0,
            top: 16.0,
            bottom: 24.0, // Extra bottom margin to accommodate strategy markers below progress bar
        })
        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(80, 80, 100)))
        .show(ui, |ui| {
            ui.set_width(250.0); // Fixed width for card in horizontal layout
            ui.vertical(|ui| {
                ui.label(num(
                    format!("FUEL TO ADD ({:.2} L)", fuel_capacity),
                    12.0,
                    Color32::GRAY,
                ));
                ui.label(num(format!("{:.2} L", pit_sv_fuel), 32.0, current_color).strong());

                // Current fuel level progress bar
                ui.add_space(4.0);
                let fuel_level_pct = if fuel_capacity > 0.0 {
                    (fuel_level / fuel_capacity).min(1.0)
                } else {
                    0.0
                };

                // Draw current fuel level progress bar
                let available_width = ui.available_width();
                let (rect, _response) =
                    ui.allocate_exact_size(egui::vec2(available_width, 16.0), egui::Sense::hover());

                if ui.is_rect_visible(rect) {
                    // Background
                    ui.painter().rect_filled(rect, 2.0, Color32::BLACK);

                    // Progress fill for current fuel level
                    let fill_width = rect.width() * fuel_level_pct;
                    let fill_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(fill_width, rect.height()));
                    ui.painter().rect_filled(fill_rect, 2.0, current_color);

                    // Label on the progress bar
                    let text = format!("{:.1} L / {:.1} L", fuel_level, fuel_capacity);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::proportional(12.0),
                        Color32::WHITE,
                    );
                }

                // Fuel to add progress bar, relative to the tank capacity, with strategy markers.
                ui.add_space(4.0);
                let fuel_add_pct = if fuel_capacity > 0.0 {
                    (pit_sv_fuel / fuel_capacity).min(1.0)
                } else {
                    0.0
                };

                // Draw the progress bar with custom painting for the reference lines.
                let available_width = ui.available_width();
                let bar_height = 16.0;
                let marker_extend = 8.0; // Increased space for strategy markers below the bar
                let total_height = bar_height + marker_extend;
                let (rect, _response) = ui.allocate_exact_size(
                    egui::vec2(available_width, total_height),
                    egui::Sense::hover(),
                );

                if ui.is_rect_visible(rect) {
                    // Make the clip rect larger, so that the markers can be painted.
                    let painter = ui.painter();

                    // The bar uses only the top part.
                    let bar_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), bar_height));

                    // Background
                    painter.rect_filled(bar_rect, 2.0, Color32::BLACK);

                    // Gray out the area that cannot be filled, because the tank already contains
                    // fuel_level.
                    if fuel_capacity > 0.0 {
                        let max_fillable_pct =
                            ((fuel_capacity - fuel_level) / fuel_capacity).clamp(0.0, 1.0);
                        let unfillable_x = bar_rect.min.x + max_fillable_pct * bar_rect.width();
                        let unfillable_rect = egui::Rect::from_min_max(
                            egui::pos2(unfillable_x, bar_rect.min.y),
                            bar_rect.max,
                        );
                        painter.rect_filled(unfillable_rect, 2.0, Color32::from_rgb(50, 50, 50));
                    }

                    // Progress fill
                    let fill_width = bar_rect.width() * fuel_add_pct;
                    let fill_rect = egui::Rect::from_min_size(
                        bar_rect.min,
                        egui::vec2(fill_width, bar_rect.height()),
                    );
                    painter.rect_filled(fill_rect, 2.0, current_color);

                    // Reference lines for LOW, AVG, PUSH and MAX. The markers go below the progress
                    // bar, so use the full rect for them.
                    let marker_y_range =
                        egui::Rangef::new(bar_rect.bottom(), rect.min.y + total_height);

                    if fuel_capacity > 0.0 {
                        for (rate, (_, color)) in rates.iter().zip(CARD_RATES) {
                            if rate.fuel_to_add > 0.0 {
                                let x = bar_rect.min.x
                                    + (rate.fuel_to_add / fuel_capacity).min(1.0)
                                        * bar_rect.width();
                                painter.vline(x, marker_y_range, egui::Stroke::new(4.0_f32, color));
                            }
                        }
                    }

                    // Text on the bar, centered on the bar part only.
                    let text = format!("{:.1}L / {:.1}L", pit_sv_fuel, fuel_capacity);
                    painter.text(
                        bar_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::proportional(12.0),
                        Color32::WHITE,
                    );
                }

                // Strategy table: Strategy | Laps | L/Lap | Full. Add space for the markers below
                // the progress bar.
                ui.add_space(12.0);
                egui::Frame::none().inner_margin(4.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Fixed column width. An automatic grid measures its widest cell again in each
                    // frame, so the columns move when their numbers change.
                    egui::Grid::new("strategy_grid")
                        .num_columns(4)
                        .spacing([6.0, 2.0])
                        .min_col_width(STRATEGY_GRID_COL_WIDTH)
                        .max_col_width(STRATEGY_GRID_COL_WIDTH)
                        .show(ui, |ui| {
                            // Header row
                            ui.label(RichText::new("").size(10.0).color(Color32::GRAY));
                            ui.label(RichText::new("Laps").size(10.0).color(Color32::GRAY));
                            ui.label(RichText::new("L/Lap").size(10.0).color(Color32::GRAY));
                            ui.label(RichText::new("Full").size(10.0).color(Color32::GRAY));
                            ui.end_row();

                            // One row for each rate. All values are live telemetry, so they use
                            // `num`, so that the grid columns do not change size when the digits
                            // change.
                            for (rate, (name, color)) in rates.iter().zip(CARD_RATES) {
                                let laps_can_go = rate.laps_on_fuel;
                                let fuel_lap = rate.fuel_per_lap;
                                ui.label(RichText::new(name).size(10.0).strong().color(color));
                                ui.label(num(
                                    dash_or(laps_can_go > 0.0, || format!("{laps_can_go:.1}")),
                                    10.0,
                                    Color32::WHITE,
                                ));
                                ui.label(num(
                                    dash_or(fuel_lap > 0.0, || format!("{fuel_lap:.3}")),
                                    10.0,
                                    color,
                                ));
                                ui.label(num(
                                    dash_or(fuel_lap > 0.0, || {
                                        format!("{:.1}", fuel_capacity / fuel_lap)
                                    }),
                                    10.0,
                                    color,
                                ));
                                ui.end_row();
                            }
                        });
                });
            });
            ui.allocate_space(egui::vec2(40.0, 0.0));
        });
}

// Renders a metric card with an optional progress bar.
pub fn render_metric_card(
    ui: &mut egui::Ui,
    title: &str,
    value: String,
    color: Color32,
    progress_bar: Option<(f32, String, Color32)>, // (progress, text, color)
) {
    egui::Frame::none()
        .fill(CARD_BACKGROUND_COLOR)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(80, 80, 100)))
        .show(ui, |ui| {
            ui.set_width(METRIC_CARD_WIDTH);
            ui.set_min_height(METRIC_CARD_HEIGHT);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(title).size(12.0).color(Color32::GRAY));
                // Monospace: this is the main telemetry value of the card. A proportional font
                // changes its size each time a digit changes.
                ui.label(num(value, 32.0, color).strong());

                // The space for the bar row is always kept, so a card without a bar has the same
                // height as a card with a bar. The height fits the bar and not text, because
                // `CARD_LINE_HEIGHT` clipped the bar label.
                ui.add_space(4.0);
                reserved_row(ui, PROGRESS_BAR_ROW_HEIGHT, |ui| {
                    if let Some((progress, text, bar_color)) = progress_bar {
                        let available_width = ui.available_width();
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(available_width)
                                .text(text)
                                .fill(bar_color),
                        );
                    }
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use checking_fuel::{
        compute_all_simulations, get_effective_fuel_capacity, simulate_race_strategy,
    };

    // ============================================
    // Layout stability
    //
    // The geometry of a card must come from the layout and not from its data. These tests render
    // the real widgets in a headless egui context. They make sure that the measured size is the
    // same in all data states of the card, also in the empty state.
    // ============================================

    fn outcome(
        pit_lap: Option<i32>,
        fuel_to_add_now: f32,
        finishes: bool,
        margin: f32,
    ) -> crate::strategy::StrategyOutcome {
        crate::strategy::StrategyOutcome {
            label: "AVG".to_string(),
            fuel_per_lap: 2.5,
            margin_without_stopping: margin,
            finishes_without_stopping: finishes,
            laps_on_current_fuel: 12.3,
            fuel_to_add_now,
            fuel_at_first_stop: pit_lap.map(|_| 34.5),
            pit_lap,
        }
    }

    /// Lays out one LAPS LEFT card and returns the space that it used.
    fn measure_card(
        ctx: &egui::Context,
        outcome: Option<&crate::strategy::StrategyOutcome>,
    ) -> egui::Vec2 {
        let mut size = egui::Vec2::ZERO;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = ui.scope(|ui| {
                    render_laps_left_card(
                        ui,
                        "LAPS LEFT (AVG)",
                        outcome,
                        Color32::WHITE,
                        Color32::WHITE,
                        LAPS_LEFT_SIZE_TAB,
                        2.0,
                    );
                });
                size = r.response.rect.size();
            });
        });
        size
    }

    #[test]
    fn laps_left_card_is_the_same_size_in_every_data_state() {
        let ctx = egui::Context::default();

        // Each of these uses a different combination of the two optional rows of the card.
        let states = [
            (
                "pit planned, short",
                Some(outcome(Some(18), 0.0, false, -4.0)),
            ),
            (
                "pit planned, spare",
                Some(outcome(Some(18), 0.0, true, 4.0)),
            ),
            (
                "no pit, splash needed",
                Some(outcome(None, 6.0, false, -6.0)),
            ),
            ("no pit, comfortable", Some(outcome(None, 0.0, true, 12.0))),
            // The outcome is not available, so both optional rows are empty.
            ("no data at all", None),
        ];

        // Warm up: egui needs one frame to get the font metrics. Before that, the first measurement
        // is not correct.
        let _ = measure_card(&ctx, states[0].1.as_ref());

        let mut measured: Vec<(&str, egui::Vec2)> = Vec::new();
        for (name, o) in &states {
            measured.push((name, measure_card(&ctx, o.as_ref())));
        }

        let (_, first) = measured[0];
        for (name, size) in &measured {
            assert_eq!(
                *size, first,
                "card size for state {name:?} is {size:?}, but {:?} for {:?} — \
                 the card must not resize with its data",
                first, measured[0].0
            );
        }
    }

    #[test]
    fn metric_card_is_the_same_size_with_and_without_a_progress_bar() {
        let ctx = egui::Context::default();

        let measure = |bar: Option<(f32, String, Color32)>| {
            let mut size = egui::Vec2::ZERO;
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = ui.scope(|ui| {
                        render_metric_card(
                            ui,
                            "FUEL LEVEL",
                            "19.91 L".into(),
                            Color32::WHITE,
                            bar.clone(),
                        );
                    });
                    size = r.response.rect.size();
                });
            });
            size
        };

        let _ = measure(None); // warm-up frame
        let without = measure(None);
        let with = measure(Some((0.5, "19.9L / 20.0L".into(), Color32::GREEN)));

        assert_eq!(
            with, without,
            "metric card resizes when the progress bar appears: {with:?} vs {without:?}"
        );
    }

    #[test]
    fn numbers_render_monospace_so_digit_changes_do_not_reflow() {
        let ctx = egui::Context::default();

        // "111.1" and "000.0" have the same number of characters but different widths in a
        // proportional font. In monospace they have the same width. This is why live values use
        // `num`.
        let measure = |text: &str| {
            let mut w = 0.0;
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = ui.label(num(text, 32.0, Color32::WHITE));
                    w = r.rect.width();
                });
            });
            w
        };

        let _ = measure("111.1"); // warm-up frame
        let narrow = measure("111.1");
        let wide = measure("000.0");

        assert!(
            (narrow - wide).abs() < 0.5,
            "monospace digits should have equal width, got {narrow} vs {wide}"
        );
    }

    // ============================================
    // Tests for get_effective_fuel_capacity
    // ============================================

    #[test]
    fn test_effective_fuel_capacity_no_restriction() {
        // When no fuel tank percentage is set, return the base capacity.
        let result = get_effective_fuel_capacity(120.0, None);
        assert_eq!(result, 120.0);
    }

    #[test]
    fn test_effective_fuel_capacity_100_percent() {
        // 100% returns the full capacity.
        let result = get_effective_fuel_capacity(120.0, Some(100.0));
        assert_eq!(result, 120.0);
    }

    #[test]
    fn test_effective_fuel_capacity_50_percent() {
        // 50% restriction
        let result = get_effective_fuel_capacity(120.0, Some(50.0));
        assert_eq!(result, 60.0);
    }

    #[test]
    fn test_effective_fuel_capacity_indycar_example() {
        // IndyCar 41.8% restriction (50.2 L from a 120 L tank).
        let result = get_effective_fuel_capacity(120.0, Some(41.833));
        assert!((result - 50.2).abs() < 0.1);
    }

    // ============================================
    // Tests for simulate_race_strategy - No pit needed
    // ============================================

    #[test]
    fn test_simulate_no_pit_needed_exact_fuel() {
        // Exactly enough fuel to finish
        // 10 laps to go, 2.5L/lap = 25L needed, have 25L
        let result = simulate_race_strategy(25.0, 0.0, 10, 2.5, 100.0);

        assert_eq!(result.num_stops, 0);
        assert!((result.fuel_margin - 0.0).abs() < 0.01);
        assert!(result.can_finish);
        assert!(result.pit_details.is_empty());
    }

    #[test]
    fn test_simulate_no_pit_needed_extra_fuel() {
        // More than enough fuel to finish
        // 10 laps to go, 2.5L/lap = 25L needed, have 30L
        let result = simulate_race_strategy(30.0, 0.0, 10, 2.5, 100.0);

        assert_eq!(result.num_stops, 0);
        assert!((result.fuel_margin - 5.0).abs() < 0.01);
        assert!(result.can_finish);
        assert!(result.pit_details.is_empty());
    }

    #[test]
    fn test_simulate_no_pit_mid_lap() {
        // Start mid-lap with enough fuel
        // On lap 5 at 50%, so 4.5 laps to go in a 10 lap race
        // 4.5 laps * 2.0L/lap = 9L needed, have 10L
        let result = simulate_race_strategy(10.0, 5.5, 10, 2.0, 100.0);

        assert_eq!(result.num_stops, 0);
        assert!((result.fuel_margin - 1.0).abs() < 0.01);
        assert!(result.can_finish);
        assert!(result.pit_details.is_empty());
    }

    // ============================================
    // Tests for simulate_race_strategy - One pit stop
    // ============================================

    #[test]
    fn test_simulate_one_pit_stop() {
        // Need one pit stop
        // 20 laps to go, 3.0L/lap = 60L needed, have 30L, tank is 50L
        // Can do 10 laps on 30L, then need to pit
        let result = simulate_race_strategy(30.0, 0.0, 20, 3.0, 50.0);

        assert_eq!(result.num_stops, 1);
        assert!(result.can_finish);
        assert_eq!(result.pit_details.len(), 1);
        // First pit should be around lap 10
        assert_eq!(result.pit_details[0].0, 10);
    }

    #[test]
    fn test_simulate_one_pit_partial_fill() {
        // Need one pit with partial fill (not full tank)
        // 15 laps to go, 2.0L/lap = 30L needed, have 20L, tank is 50L
        // Can do 10 laps on 20L, need 5 more laps = 10L
        let result = simulate_race_strategy(20.0, 0.0, 15, 2.0, 50.0);

        assert_eq!(result.num_stops, 1);
        assert!(result.can_finish);
        assert_eq!(result.pit_details.len(), 1);
        // Should add 10L (for 5 remaining laps)
        assert!((result.pit_details[0].1 - 10.0).abs() < 0.1);
    }

    // ============================================
    // Tests for simulate_race_strategy - Multiple pit stops
    // ============================================

    #[test]
    fn test_simulate_two_pit_stops() {
        // Need two pit stops
        // 40 laps to go, 4.0L/lap = 160L needed, tank is 60L
        // Can do 15 laps per tank, so need 2 stops (15 + 15 + 10 = 40)
        let result = simulate_race_strategy(60.0, 0.0, 40, 4.0, 60.0);

        assert_eq!(result.num_stops, 2);
        assert!(result.can_finish);
        assert_eq!(result.pit_details.len(), 2);
    }

    #[test]
    fn test_simulate_three_pit_stops() {
        // Long race needing 3 stops
        // 80 laps, 4.0L/lap = 320L needed, tank is 100L
        // Can do 25 laps per tank, so need 3 stops
        let result = simulate_race_strategy(100.0, 0.0, 80, 4.0, 100.0);

        assert!(result.num_stops >= 3);
        assert!(result.can_finish);
        assert_eq!(result.pit_details.len(), result.num_stops as usize);
    }

    // ============================================
    // Tests for simulate_race_strategy - Edge cases
    // ============================================

    #[test]
    fn test_simulate_last_lap() {
        // On the last lap
        // 0.5 laps to go, need 1L, have 2L
        let result = simulate_race_strategy(2.0, 9.5, 10, 2.0, 50.0);

        assert_eq!(result.num_stops, 0);
        assert!((result.fuel_margin - 1.0).abs() < 0.01);
        assert!(result.can_finish);
        assert!(result.pit_details.is_empty());
    }

    #[test]
    fn test_simulate_race_already_finished() {
        // Race is over (at or past total laps)
        let result = simulate_race_strategy(10.0, 10.0, 10, 2.0, 50.0);

        assert_eq!(result.num_stops, 0);
        assert!(result.can_finish);
        assert!(result.pit_details.is_empty());
        // Should have all fuel remaining
        assert!((result.fuel_margin - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_simulate_impossible_tank_too_small() {
        // Tank is smaller than fuel needed for 1 lap - impossible
        let result = simulate_race_strategy(1.0, 0.0, 10, 5.0, 4.0);

        assert!(!result.can_finish);
        assert!(!result.pit_details.is_empty()); // Will have attempted pit stops
    }

    #[test]
    fn test_simulate_zero_fuel_consumption() {
        // Edge case: 0 fuel per lap (electric car or a bug). Without a check, this causes a
        // division by zero. The function returns can_finish with all fuel remaining.
        let result = simulate_race_strategy(50.0, 0.0, 10, 0.001, 50.0);

        assert_eq!(result.num_stops, 0);
        assert!(result.can_finish);
        assert!(result.fuel_margin > 49.0); // Almost all fuel remaining
    }

    // ============================================
    // Tests for compute_all_simulations
    // ============================================

    #[test]
    fn test_compute_all_simulations_returns_four_results() {
        let results = compute_all_simulations(
            50.0,  // current_fuel
            0.0,   // current_lap_fractional
            20,    // total_laps
            100.0, // fuel_capacity
            [
                2.0, // LOW
                2.5, // AVG
                3.0, // PUSH
                3.5, // MAX
            ],
        );

        assert_eq!(results.len(), 4);
    }

    #[test]
    fn test_compute_all_simulations_order() {
        // The lowest fuel use gives the best results: the most margin and the fewest stops.
        let results = compute_all_simulations(
            40.0, // current_fuel
            0.0,  // current_lap_fractional
            20,   // total_laps
            50.0, // fuel_capacity
            [
                2.0, // LOW - can do 20 laps on 40L, 0 stops
                2.5, // AVG - can do 16 laps, 1 stop
                3.0, // PUSH - can do 13 laps, 1 stop
                4.0, // MAX - can do 10 laps, 1 stop
            ],
        );

        // LOW (index 0) should have 0 stops (40L / 2.0L = 20 laps exactly)
        assert_eq!(results[0].num_stops, 0);
        assert!(results[0].can_finish);

        // AVG (index 1) should need a pit stop (40L / 2.5L = 16 laps < 20)
        assert!(results[1].num_stops >= 1);
        assert!(results[1].can_finish);

        // PUSH (index 2) should need a pit stop
        assert!(results[2].num_stops >= 1);
        assert!(results[2].can_finish);

        // MAX (index 3) should need a pit stop
        assert!(results[3].num_stops >= 1);
        assert!(results[3].can_finish);
    }

    #[test]
    fn test_compute_all_simulations_all_no_stop() {
        // Scenario where no strategy needs a pit stop
        let results = compute_all_simulations(
            100.0, // current_fuel - plenty
            0.0,   // current_lap_fractional
            10,    // short race
            100.0, // fuel_capacity
            [
                2.0, // LOW
                2.5, // AVG
                3.0, // PUSH
                3.5, // MAX
            ],
        );

        // All strategies should finish without stops
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.num_stops, 0, "Strategy {} should need 0 stops", i);
            assert!(result.can_finish, "Strategy {} should be able to finish", i);
        }
    }

    // ============================================
    // Real-world scenario tests
    // ============================================

    #[test]
    fn test_realistic_oval_race() {
        // Oval race. 100 lap race, 2.8 L/lap average, 60 L tank, start with 55 L on lap 20.
        let result = simulate_race_strategy(55.0, 20.0, 100, 2.8, 60.0);

        println!("Oval race simulation:");
        println!("  Laps remaining: 80");
        println!("  Fuel needed: {} L", 80.0 * 2.8);
        println!("  Pit stops: {}", result.num_stops);
        println!("  Can finish: {}", result.can_finish);
        println!("  Fuel margin: {:.2} L", result.fuel_margin);
        for (i, (lap, fuel)) in result.pit_details.iter().enumerate() {
            println!("  Pit {}: Lap {}, add {:.1}L", i + 1, lap, fuel);
        }

        assert!(result.can_finish);
        // With 80 laps * 2.8L = 224L needed, 60L tank, need ~4 stops
        assert!(result.num_stops >= 3);
    }

    #[test]
    fn test_realistic_road_course_race() {
        // Road course race. 45 lap race, 3.2 L/lap average, 110 L tank, start with a full tank.
        let result = simulate_race_strategy(110.0, 0.0, 45, 3.2, 110.0);

        println!("\nRoad course simulation:");
        println!("  Fuel needed: {} L", 45.0 * 3.2);
        println!("  Pit stops: {}", result.num_stops);
        println!("  Can finish: {}", result.can_finish);
        println!("  Fuel margin: {:.2} L", result.fuel_margin);
        for (i, (lap, fuel)) in result.pit_details.iter().enumerate() {
            println!("  Pit {}: Lap {}, add {:.1}L", i + 1, lap, fuel);
        }

        assert!(result.can_finish);
        // 45 * 3.2 = 144L needed, 110L tank = need 1 stop
        assert!(result.num_stops >= 1);
    }

    #[test]
    fn test_realistic_sprint_race_no_pit() {
        // Short sprint race. It finishes without a stop. 15 lap race, 2.5 L/lap, 50 L tank, start
        // with a full tank.
        let result = simulate_race_strategy(50.0, 0.0, 15, 2.5, 50.0);

        println!("\nSprint race simulation:");
        println!("  Fuel needed: {} L", 15.0 * 2.5);
        println!("  Pit stops: {}", result.num_stops);
        println!("  Can finish: {}", result.can_finish);
        println!("  Fuel margin: {:.2} L", result.fuel_margin);

        assert_eq!(result.num_stops, 0);
        assert!(result.can_finish);
        // 50L - (15 * 2.5) = 50 - 37.5 = 12.5L margin
        assert!((result.fuel_margin - 12.5).abs() < 0.1);
    }

    #[test]
    fn test_indycar_restricted_fuel() {
        // IndyCar with restricted fuel tank (50.2L from 120L base)
        // 60 lap race, 1.8L/lap (fuel saving), restricted tank
        let fuel_capacity = get_effective_fuel_capacity(120.0, Some(41.833));
        let result = simulate_race_strategy(fuel_capacity, 0.0, 60, 1.8, fuel_capacity);

        println!("\nIndyCar restricted fuel simulation:");
        println!("  Tank capacity: {:.1} L", fuel_capacity);
        println!("  Fuel needed: {} L", 60.0 * 1.8);
        println!("  Pit stops: {}", result.num_stops);
        println!("  Can finish: {}", result.can_finish);
        println!("  Fuel margin: {:.2} L", result.fuel_margin);
        for (i, (lap, fuel)) in result.pit_details.iter().enumerate() {
            println!("  Pit {}: Lap {}, add {:.1}L", i + 1, lap, fuel);
        }

        assert!(result.can_finish);
        // ~50L tank, 108L needed = 2+ stops
        assert!(result.num_stops >= 2);
    }
}
