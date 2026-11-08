use checking_fuel::AppState;
use checking_protocol::DriverInfo;
use eframe::egui;
use egui::{Color32, RichText};

use crate::config::{MinimapMode, RadarOverlayConfig, RelativeOverlayConfig};
use crate::pages;
use crate::pages::{AVG_COLOR, LOW_COLOR, MAX_COLOR, PUSH_COLOR};
use crate::radar::{RadarTarget, analyze_radar, bar_bounds_px};
use crate::relative::relative_gap;

const LAP_PROGRESS_BAR_COLOR: Color32 = Color32::from_rgb(100, 150, 255);
const CARD_BACKGROUND_COLOR: Color32 = pages::CARD_BACKGROUND_COLOR;
// The card colors that all pages use are in `pages`.
const SUCCESS_TEXT_COLOR: Color32 = pages::SUCCESS_TEXT_COLOR;
const WARNING_TEXT_COLOR: Color32 = pages::WARNING_TEXT_COLOR;

pub fn render_laps_remaining_overlay(ui: &mut egui::Ui, state: &AppState) -> egui::Response {
    let total_laps = state.get_effective_total_laps();
    let current_lap_fractional = state.get_current_lap_fractional();
    let laps_remaining_fractional = (total_laps as f32 - current_lap_fractional).max(0.0);
    let lap_progress = state.latest_frame.lap_dist_pct;

    // Render the card and get its rect.
    let frame_response = egui::Frame::none()
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .show(ui, |ui| {
            pages::render_metric_card(
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
        });

    // Make an interactive response over all of the card area.
    ui.interact(
        frame_response.response.rect,
        ui.id().with("drag_laps"),
        egui::Sense::drag(),
    )
}

pub fn render_fuel_to_add_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    extra_liters: f32,
    fuel_tank_pct: Option<f32>,
) -> egui::Response {
    let total_laps = state.get_effective_total_laps();
    let avg_10_green = state.avg_fuel_pct_of_total(total_laps);
    let avg_5_push = state.avg_fuel_push_pct_of_total(total_laps);
    let low_5_green = state.avg_fuel_low_pct_of_total(total_laps);
    let max_single = state.max_fuel_usage();

    let fuel_add_avg = if let Some(avg) = avg_10_green {
        state.fuel_to_finish_avg(total_laps, avg) + extra_liters
    } else {
        0.0
    };
    let fuel_add_push = if let Some(push) = avg_5_push {
        state.fuel_to_finish_push(total_laps, push) + extra_liters
    } else {
        0.0
    };
    let fuel_add_low = if let Some(low) = low_5_green {
        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_to_go = (total_laps as f32 - current_lap_fractional).max(0.0);
        let fuel_needed = laps_to_go * low + extra_liters;
        (fuel_needed - state.latest_frame.fuel_level).max(0.0)
    } else {
        0.0
    };
    let fuel_add_max = if let Some(max) = max_single {
        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_to_go = (total_laps as f32 - current_lap_fractional).max(0.0);
        let fuel_needed = laps_to_go * max + extra_liters;
        (fuel_needed - state.latest_frame.fuel_level).max(0.0)
    } else {
        0.0
    };

    let pit_sv_fuel = state.latest_frame.pit_sv_fuel;
    let fuel_capacity =
        pages::get_effective_fuel_capacity(state.get_fuel_capacity(), fuel_tank_pct);

    let card_rate = |fuel_to_add: f32, fuel_per_lap: Option<f32>| pages::CardRate {
        fuel_to_add,
        fuel_per_lap: fuel_per_lap.unwrap_or(0.0),
        laps_on_fuel: fuel_per_lap.map_or(0.0, |rate| state.laps_remaining(rate)),
    };
    let rates = [
        card_rate(fuel_add_low, low_5_green),
        card_rate(fuel_add_avg, avg_10_green),
        card_rate(fuel_add_push, avg_5_push),
        card_rate(fuel_add_max, max_single),
    ];

    let frame_response = egui::Frame::none()
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .show(ui, |ui| {
            pages::render_fuel_to_add_card(
                ui,
                pit_sv_fuel,
                state.latest_frame.fuel_level,
                fuel_capacity,
                &rates,
            );
        });

    // Make an interactive response over all of the card area.
    ui.interact(
        frame_response.response.rect,
        ui.id().with("drag_fuel"),
        egui::Sense::drag(),
    )
}

/// One LAPS LEFT overlay card. The four strategies use it.
///
/// This card is not the same as the LAPS LEFT card on the Fuel tab. The overlay shows the fuel
/// margin and does not show the pit line. In the car, the driver wants to know if the fuel is
/// sufficient. The Fuel tab tells when to pit.
fn render_laps_left_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    label: &str,
    fuel_per_lap: Option<f32>,
    color: Color32,
    extra_liters: f32,
    drag_id: &str,
) -> egui::Response {
    let laps = fuel_per_lap.map_or(0.0, |rate| state.laps_remaining(rate));
    let (can_finish, fuel_margin) = match fuel_per_lap {
        Some(rate) => state.calculate_fuel_margin(rate, extra_liters),
        None => (false, 0.0),
    };

    // Total extra fuel: the usable margin and the reserve from `extra_liters`.
    let total_extra_fuel = fuel_margin + extra_liters;

    let frame_response = egui::Frame::none()
        .fill(CARD_BACKGROUND_COLOR)
        .inner_margin(12.0)
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .stroke(egui::Stroke::new(2.0_f32, color))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(label).size(12.0).color(Color32::GRAY));
                ui.label(
                    RichText::new(format!("{laps:>5.1}"))
                        .size(pages::LAPS_LEFT_SIZE_OVERLAY)
                        .strong()
                        .color(color),
                );
                ui.add_space(2.0);

                let status_text = match (can_finish, extra_liters > 0.0) {
                    (true, true) => {
                        format!("✔ +{total_extra_fuel:.1}L (+{fuel_margin:.1}L margin)")
                    }
                    (true, false) => format!("✔ +{fuel_margin:.1}L"),
                    (false, true) => format!(
                        "Short {:.1}L ({extra_liters:.1}L margin)",
                        fuel_margin.abs(),
                    ),
                    (false, false) => format!("Short {:.1}L", fuel_margin.abs()),
                };
                let status_color = if can_finish {
                    SUCCESS_TEXT_COLOR
                } else {
                    WARNING_TEXT_COLOR
                };
                ui.label(RichText::new(status_text).size(13.0).color(status_color));
            });
        });

    ui.interact(
        frame_response.response.rect,
        ui.id().with(drag_id),
        egui::Sense::drag(),
    )
}

pub fn render_avg_laps_left_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    extra_liters: f32,
    _fuel_tank_pct: Option<f32>,
) -> egui::Response {
    let total_laps = state.get_effective_total_laps();
    render_laps_left_overlay(
        ui,
        state,
        "LAPS LEFT (AVG)",
        state.avg_fuel_pct_of_total(total_laps),
        AVG_COLOR,
        extra_liters,
        "drag",
    )
}

pub fn render_push_laps_left_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    extra_liters: f32,
    _fuel_tank_pct: Option<f32>,
) -> egui::Response {
    let total_laps = state.get_effective_total_laps();
    render_laps_left_overlay(
        ui,
        state,
        "LAPS LEFT (PUSH)",
        state.avg_fuel_push_pct_of_total(total_laps),
        PUSH_COLOR,
        extra_liters,
        "drag_push",
    )
}

pub fn render_low_laps_left_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    extra_liters: f32,
    _fuel_tank_pct: Option<f32>,
) -> egui::Response {
    let total_laps = state.get_effective_total_laps();
    render_laps_left_overlay(
        ui,
        state,
        "LAPS LEFT (LOW)",
        state.avg_fuel_low_pct_of_total(total_laps),
        LOW_COLOR,
        extra_liters,
        "drag_low",
    )
}

pub fn render_max_laps_left_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    extra_liters: f32,
    _fuel_tank_pct: Option<f32>,
) -> egui::Response {
    render_laps_left_overlay(
        ui,
        state,
        "LAPS LEFT (MAX)",
        state.max_fuel_usage(),
        MAX_COLOR,
        extra_liters,
        "drag_max",
    )
}

pub fn render_relative_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    config: &RelativeOverlayConfig,
) -> egui::Response {
    let frame = &state.latest_frame;
    let player_idx = frame.player_car_idx;

    // Drivers sorted by track position, for the minimap.
    let mut sorted_drivers: Vec<_> = frame.drivers.iter().collect();
    sorted_drivers.sort_by(|a, b| {
        // Sort by lap (descending), then by lap_dist_pct (descending).
        match b.lap.cmp(&a.lap) {
            std::cmp::Ordering::Equal => b
                .lap_dist_pct
                .partial_cmp(&a.lap_dist_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            other => other,
        }
    });

    // Find the player and calculate the relative track positions.
    let player = match frame.drivers.iter().find(|d| d.car_idx == player_idx) {
        Some(p) => p,
        None => {
            // The player is not in the list. Show all drivers sorted by position.
            let relative_drivers: Vec<_> = sorted_drivers.to_vec();

            let outer_frame_response = egui::Frame::none()
                .outer_margin(egui::Margin::symmetric(1.0, 1.0))
                .show(ui, |ui| {
                    egui::Frame::none()
                        .fill(CARD_BACKGROUND_COLOR)
                        .inner_margin(12.0)
                        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("RELATIVE")
                                        .size(16.0)
                                        .strong()
                                        .color(Color32::LIGHT_GRAY),
                                );
                                ui.add_space(8.0);

                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for &driver in &relative_drivers {
                                            render_driver_row(
                                                ui, driver, player_idx, config, None,
                                                0, // No lap difference without the player.
                                                state,
                                            );
                                        }
                                    });
                            });
                        });
                });

            return ui.interact(
                outer_frame_response.response.rect,
                ui.id().with("drag_relative"),
                egui::Sense::drag(),
            );
        }
    };

    // Track distance and time gap of each driver relative to the player. The relative position uses
    // only lap_dist_pct and not the lap number, because the relative view shows the cars that are
    // near on the track, on all laps. Returns: (driver, track_delta, time_delta, lap_diff)
    let mut relative_positions: Vec<_> = frame
        .drivers
        .iter()
        .map(|driver| {
            let gap = relative_gap(player, driver);
            (driver, gap.track_delta, gap.time_delta, gap.lap_diff)
        })
        .collect();

    // Sort by time delta (negative is behind, positive is in front). The sort uses the time gap and
    // not the lap number.
    relative_positions.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    // Find the player position in the sorted list.
    let player_position = relative_positions
        .iter()
        .position(|(d, _, _, _)| d.car_idx == player_idx)
        .unwrap_or(0);

    // Number of cars to show in front and behind.
    let num_ahead = (config.num_positions / 2).max(1);
    let num_behind = config.num_positions - num_ahead - 1;

    // Drivers near the player on the track. Tuple: (driver, track_delta, time_delta, lap_diff)
    let start_idx = player_position.saturating_sub(num_behind);
    let end_idx = (player_position + num_ahead + 1).min(relative_positions.len());
    let mut relative_drivers: Vec<_> = relative_positions[start_idx..end_idx]
        .iter()
        .map(|(driver, track_delta, time_delta, lap_diff)| {
            (*driver, *track_delta, *time_delta, *lap_diff)
        })
        .collect();

    // Reverse the order, so that the cars in front are at the top.
    relative_drivers.reverse();

    let outer_frame_response = egui::Frame::none()
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .show(ui, |ui| {
            egui::Frame::none()
                .fill(CARD_BACKGROUND_COLOR)
                .inner_margin(12.0)
                .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        // Header
                        ui.label(
                            RichText::new("RELATIVE")
                                .size(16.0)
                                .strong()
                                .color(Color32::LIGHT_GRAY),
                        );

                        ui.add_space(8.0);

                        // Minimap (if enabled)
                        if config.show_minimap {
                            render_minimap(ui, &sorted_drivers, player_idx, &config.minimap_mode);
                            ui.add_space(12.0);
                        }

                        // Driver list
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for &(driver, _track_delta, time_delta, lap_diff) in
                                    &relative_drivers
                                {
                                    render_driver_row(
                                        ui,
                                        driver,
                                        player_idx,
                                        config,
                                        Some(time_delta),
                                        lap_diff,
                                        state,
                                    );
                                }
                            });
                    });
                });
        });

    ui.interact(
        outer_frame_response.response.rect,
        ui.id().with("drag_relative"),
        egui::Sense::drag(),
    )
}

fn render_minimap(
    ui: &mut egui::Ui,
    sorted_drivers: &[&DriverInfo],
    player_idx: i32,
    mode: &MinimapMode,
) {
    let minimap_height = 20.0;
    let minimap_width = ui.available_width();

    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(minimap_width, minimap_height),
        egui::Sense::hover(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // Draw background
        painter.rect_filled(rect, 4.0, Color32::from_rgb(15, 20, 25));

        // Draw track line
        let track_y = rect.center().y;
        let line_start = egui::pos2(rect.left() + 10.0, track_y);
        let line_end = egui::pos2(rect.right() - 10.0, track_y);
        painter.line_segment(
            [line_start, line_end],
            egui::Stroke::new(3.0_f32, Color32::from_rgb(60, 60, 60)),
        );

        // Find the leader: the first item in sorted_drivers, which is sorted by track position.
        let leader_idx = sorted_drivers.first().map(|d| d.car_idx);

        // Draw cars
        for driver in sorted_drivers {
            let normalized_pos = driver.lap_dist_pct % 1.0; // 0.0 to 1.0

            let car_x = match mode {
                MinimapMode::Relative => {
                    // Relative mode: player centered
                    let player = sorted_drivers
                        .iter()
                        .find(|d| d.car_idx == player_idx)
                        .map(|d| d.lap_dist_pct % 1.0)
                        .unwrap_or(0.0);

                    let mut relative_pos = normalized_pos - player;
                    // Wrap the position.
                    if relative_pos > 0.5 {
                        relative_pos -= 1.0;
                    } else if relative_pos < -0.5 {
                        relative_pos += 1.0;
                    }

                    // Map to the screen, with the player in the center.
                    rect.center().x + relative_pos * (line_end.x - line_start.x)
                }
                MinimapMode::Absolute => {
                    // Absolute mode: S/F line at center
                    let centered_pos = normalized_pos - 0.5; // -0.5 to 0.5
                    rect.center().x + centered_pos * (line_end.x - line_start.x)
                }
            };

            // Clamp to the visible area.
            if car_x < line_start.x || car_x > line_end.x {
                continue;
            }

            let is_player = driver.car_idx == player_idx;
            let is_leader = Some(driver.car_idx) == leader_idx;

            let color = if is_player {
                Color32::from_rgb(100, 200, 255) // Cyan for player
            } else if is_leader {
                Color32::from_rgb(50, 255, 50) // Green for leader
            } else if driver.on_pit_road == 1 {
                Color32::from_rgb(255, 200, 50) // Yellow for pit
            } else {
                Color32::from_rgb(200, 200, 200) // White for others
            };

            let car_size = if is_player || is_leader { 8.0 } else { 6.0 };

            painter.circle_filled(egui::pos2(car_x, track_y), car_size, color);
        }
    }
}

fn render_driver_row(
    ui: &mut egui::Ui,
    driver: &DriverInfo,
    player_idx: i32,
    config: &RelativeOverlayConfig,
    delta: Option<f32>,
    lap_diff: i32,
    state: &AppState,
) {
    let is_player = driver.car_idx == player_idx;
    let is_buddy = config.buddy_list.contains(&driver.get_user_name());
    let is_warning = config.warning_list.contains(&driver.get_user_name());

    // Lap status for the color, from the real lap difference.
    let is_lapped_down = lap_diff <= -1; // 1+ laps behind player
    let is_lapping = lap_diff >= 1; // 1+ laps ahead of player

    let bg_color = if is_player {
        Color32::from_rgb(40, 60, 80) // Blue tint for player
    } else if is_warning {
        Color32::from_rgb(80, 30, 30) // Dark red for warning drivers
    } else if is_buddy {
        Color32::from_rgb(60, 80, 40) // Green tint for buddy
    } else if is_lapped_down {
        Color32::from_rgb(50, 30, 30) // Red tint for lapped cars
    } else if is_lapping {
        Color32::from_rgb(30, 40, 50) // Blue tint for cars a lap ahead
    } else {
        Color32::from_rgb(30, 35, 40) // Default dark
    };

    egui::Frame::none()
        .fill(bg_color)
        .inner_margin(4.0)
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .stroke(egui::Stroke::new(
            if is_warning { 2.0_f32 } else { 1.0_f32 },
            if is_player {
                Color32::from_rgb(100, 150, 200)
            } else if is_warning {
                Color32::from_rgb(255, 80, 80) // Red border for a warning driver
            } else if is_buddy {
                Color32::from_rgb(100, 200, 100)
            } else if is_lapped_down {
                Color32::from_rgb(150, 60, 60)
            } else if is_lapping {
                Color32::from_rgb(60, 100, 150)
            } else {
                Color32::from_rgb(50, 55, 60)
            },
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Position - fixed width
                ui.add_sized(
                    [30.0, 20.0],
                    egui::Label::new(
                        RichText::new(format!("P{}", driver.position))
                            .size(14.0)
                            .strong()
                            .color(Color32::WHITE),
                    ),
                );

                // Car number - fixed width
                ui.add_sized(
                    [35.0, 20.0],
                    egui::Label::new(
                        RichText::new(format!("#{}", driver.get_car_number()))
                            .size(10.0)
                            .strong()
                            .color(Color32::WHITE),
                    ),
                );

                // Warning indicator
                if is_warning {
                    ui.label(
                        RichText::new("⚠")
                            .size(14.0)
                            .color(Color32::from_rgb(255, 200, 0)),
                    );
                }

                // Driver name - expanding
                ui.label(
                    RichText::new(driver.get_user_name())
                        .size(10.0)
                        .color(if is_player {
                            Color32::from_rgb(150, 200, 255)
                        } else if is_warning {
                            Color32::from_rgb(255, 150, 150)
                        } else {
                            Color32::LIGHT_GRAY
                        }),
                );

                // Spacer that moves the items on the right side to the right edge.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Time gap indicator: fixed width, at the right edge.
                    let time_text = if let Some(time_delta) = delta {
                        if !is_player {
                            if time_delta > 0.01 {
                                format!("+{:.3}", time_delta.abs())
                            } else if time_delta < -0.01 {
                                format!("-{:.3}", time_delta.abs())
                            } else {
                                "0.000".to_string()
                            }
                        } else {
                            "     ".to_string() // Empty space for player
                        }
                    } else {
                        "     ".to_string()
                    };

                    let time_color = if let Some(time_delta) = delta {
                        if time_delta > 0.01 {
                            Color32::from_rgb(100, 255, 100)
                        } else if time_delta < -0.01 {
                            Color32::from_rgb(255, 100, 100)
                        } else {
                            Color32::GRAY
                        }
                    } else {
                        Color32::GRAY
                    };

                    ui.add_sized(
                        [50.0, 20.0],
                        egui::Label::new(
                            RichText::new(time_text)
                                .size(12.0)
                                .strong()
                                .color(time_color)
                                .family(egui::FontFamily::Monospace),
                        ),
                    );

                    // Pit stop count - fixed width
                    let pit_count = state.get_pit_stop_count(driver.car_idx);
                    let pit_text = if pit_count > 0 {
                        format!("P{}", pit_count)
                    } else {
                        "  ".to_string()
                    };
                    ui.add_sized(
                        [25.0, 20.0],
                        egui::Label::new(
                            RichText::new(pit_text)
                                .size(12.0)
                                .strong()
                                .color(Color32::from_rgb(150, 150, 255)),
                        ),
                    );

                    // Laps since pit - fixed width
                    let laps_text = if driver.laps_since_pit > 0 {
                        format!("L{}", driver.laps_since_pit)
                    } else {
                        "  ".to_string()
                    };
                    ui.add_sized(
                        [25.0, 20.0],
                        egui::Label::new(RichText::new(laps_text).size(12.0).color(Color32::GRAY)),
                    );

                    // Pit status - fixed width
                    let pit_status_text = if driver.on_pit_road == 1 {
                        "PIT"
                    } else {
                        "   "
                    };
                    ui.add_sized(
                        [30.0, 20.0],
                        egui::Label::new(
                            RichText::new(pit_status_text)
                                .size(12.0)
                                .strong()
                                .color(Color32::from_rgb(255, 200, 50)),
                        ),
                    );

                    // iRating - fixed width
                    ui.add_sized(
                        [45.0, 20.0],
                        egui::Label::new(
                            RichText::new(format!("{}", driver.irating))
                                .size(12.0)
                                .color(Color32::from_rgb(200, 200, 100)),
                        ),
                    );

                    // License (Safety rating) - fixed width
                    ui.add_sized(
                        [50.0, 20.0],
                        egui::Label::new(
                            RichText::new(driver.get_license_string())
                                .size(12.0)
                                .color(Color32::from_rgb(150, 200, 150)),
                        ),
                    );
                });
            });
        });

    ui.add_space(4.0);
}

pub fn render_pedal_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    history_seconds: f32,
) -> egui::Response {
    let frame_response = egui::Frame::none()
        .fill(CARD_BACKGROUND_COLOR)
        .inner_margin(12.0)
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                // History graph
                render_input_history_graph(ui, state, history_seconds);
            });
        });

    ui.interact(
        frame_response.response.rect,
        ui.id().with("drag_pedal"),
        egui::Sense::drag(),
    )
}

fn render_input_history_graph(ui: &mut egui::Ui, state: &AppState, history_seconds: f32) {
    let graph_height = 140.0;
    let graph_width = ui.available_width();

    let (rect, _response) =
        ui.allocate_exact_size(egui::vec2(graph_width, graph_height), egui::Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();

    // Background
    painter.rect_filled(rect, 4.0, Color32::from_rgb(15, 20, 25));

    // Grid lines (percentage markers)
    for i in 0..=4 {
        let y = rect.bottom() - (rect.height() * i as f32 / 4.0);
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, Color32::from_rgb(40, 40, 40)),
        );

        // Percentage labels
        painter.text(
            egui::pos2(rect.left() + 2.0, y - 6.0),
            egui::Align2::LEFT_CENTER,
            format!("{}%", i * 25),
            egui::FontId::proportional(10.0),
            Color32::GRAY,
        );
    }

    if state.input_history.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No input data",
            egui::FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    }

    // Draw time labels
    painter.text(
        egui::pos2(rect.left() + 20.0, rect.bottom() - 10.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{:.0}s ago", history_seconds),
        egui::FontId::proportional(10.0),
        Color32::GRAY,
    );
    painter.text(
        egui::pos2(rect.right() - 5.0, rect.bottom() - 10.0),
        egui::Align2::RIGHT_BOTTOM,
        "now",
        egui::FontId::proportional(10.0),
        Color32::GRAY,
    );

    // Plot input history
    let history = &state.input_history;
    let num_samples = history.len();

    if num_samples < 2 {
        return;
    }

    let now = std::time::Instant::now();

    // Convert input samples to screen coordinates and draw
    let mut throttle_points = Vec::new();
    let mut brake_points = Vec::new();
    let mut clutch_points = Vec::new();

    for sample in history.iter() {
        // Calculate the x position from the time. Older samples are on the left.
        let age = now.duration_since(sample.timestamp).as_secs_f32();
        let x_normalized = 1.0 - (age / history_seconds).min(1.0); // 0.0 is history_seconds ago, 1.0 is now
        let x = rect.left() + rect.width() * x_normalized;

        // Y positions are inverted: 0% at the bottom, 100% at the top.
        let throttle_y = rect.bottom() - rect.height() * sample.throttle;
        let brake_y = rect.bottom() - rect.height() * sample.brake;
        let clutch_y = rect.bottom() - rect.height() * sample.clutch;

        throttle_points.push(egui::pos2(x, throttle_y));
        brake_points.push(egui::pos2(x, brake_y));
        clutch_points.push(egui::pos2(x, clutch_y));
    }

    // Draw lines connecting points
    if throttle_points.len() > 1 {
        painter.add(egui::Shape::line(
            throttle_points,
            egui::Stroke::new(2.0_f32, Color32::from_rgb(100, 255, 100)),
        ));
    }

    if brake_points.len() > 1 {
        painter.add(egui::Shape::line(
            brake_points,
            egui::Stroke::new(2.0_f32, Color32::from_rgb(255, 100, 100)),
        ));
    }

    if clutch_points.len() > 1 {
        painter.add(egui::Shape::line(
            clutch_points,
            egui::Stroke::new(2.0_f32, Color32::from_rgb(100, 150, 255)),
        ));
    }

    // Legend
    let legend_y = rect.top() + 10.0;
    painter.text(
        egui::pos2(rect.left() + 10.0, legend_y),
        egui::Align2::LEFT_TOP,
        "Throttle",
        egui::FontId::proportional(11.0),
        Color32::from_rgb(100, 255, 100),
    );
    painter.text(
        egui::pos2(rect.left() + 70.0, legend_y),
        egui::Align2::LEFT_TOP,
        "Brake",
        egui::FontId::proportional(11.0),
        Color32::from_rgb(255, 100, 100),
    );
    painter.text(
        egui::pos2(rect.left() + 120.0, legend_y),
        egui::Align2::LEFT_TOP,
        "Clutch",
        egui::FontId::proportional(11.0),
        Color32::from_rgb(100, 150, 255),
    );
}

pub fn render_radar_left_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    cfg: &RadarOverlayConfig,
) -> egui::Response {
    let (left, _) = analyze_radar(state, cfg.fallback_car_length_m);

    let frame_response = egui::Frame::none()
        .fill(CARD_BACKGROUND_COLOR)
        .inner_margin(12.0)
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
        .show(ui, |ui| {
            render_radar_side_indicator(ui, left, cfg.range_m);
        });

    ui.interact(
        frame_response.response.rect,
        ui.id().with("drag_radar_left"),
        egui::Sense::drag(),
    )
}

pub fn render_radar_right_overlay(
    ui: &mut egui::Ui,
    state: &AppState,
    cfg: &RadarOverlayConfig,
) -> egui::Response {
    let (_, right) = analyze_radar(state, cfg.fallback_car_length_m);

    let frame_response = egui::Frame::none()
        .fill(CARD_BACKGROUND_COLOR)
        .inner_margin(12.0)
        .outer_margin(egui::Margin::symmetric(1.0, 1.0))
        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
        .show(ui, |ui| {
            render_radar_side_indicator(ui, right, cfg.range_m);
        });

    ui.interact(
        frame_response.response.rect,
        ui.id().with("drag_radar_right"),
        egui::Sense::drag(),
    )
}

/// Draws the indicator for one side: a vertical strip for `range_m` in front and behind, with a bar
/// at the relative position of the car. The bar length is the car length. The strip is empty when
/// no car is alongside.
fn render_radar_side_indicator(ui: &mut egui::Ui, target: Option<RadarTarget>, range_m: f32) {
    ui.vertical(|ui| {
        let size = egui::vec2(24.0, 120.0);
        let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
        let painter = ui.painter();

        // Strip background
        painter.rect_filled(rect, 4.0, Color32::from_rgb(30, 30, 30));

        // Player position: the center line, for reference.
        let mid_y = rect.center().y;
        painter.line_segment(
            [
                egui::pos2(rect.left(), mid_y),
                egui::pos2(rect.right(), mid_y),
            ],
            egui::Stroke::new(1.0_f32, Color32::from_rgb(90, 90, 90)),
        );

        if let Some((target, (top, bottom))) =
            target.and_then(|t| bar_bounds_px(&t, range_m, rect.height()).map(|b| (t, b)))
        {
            let bar = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 2.0, rect.top() + top),
                egui::pos2(rect.right() - 2.0, rect.top() + bottom),
            );

            let color = if target.count > 1 {
                Color32::from_rgb(255, 120, 0) // Orange: two cars on this side
            } else {
                Color32::from_rgb(255, 220, 0) // Yellow: one car
            };
            painter.rect_filled(bar, 3.0, color);
        }

        // Border
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(2.0_f32, Color32::from_rgb(100, 100, 100)),
        );
    });
}
