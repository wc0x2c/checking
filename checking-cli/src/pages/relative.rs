use checking_fuel::AppState;
use eframe::egui;
use egui::{Color32, RichText};
use egui_extras::{Column, TableBuilder};

use crate::config::RelativeOverlayConfig;

const CARD_BACKGROUND_COLOR: Color32 = Color32::from_rgb(40, 40, 50);
const HEADER_TEXT_COLOR: Color32 = Color32::from_rgb(200, 200, 220);

/// Width of the caption column in the info cards. The width is fixed, so that
/// all values start at the same x position.
const INFO_LABEL_WIDTH: f32 = 140.0;

/// Height of a buddy list or a warning list. These lists get one row for each
/// driver when cars join and leave the session. Thus they scroll in a box with
/// a fixed size, and the card around them does not change size.
const LIST_HEIGHT: f32 = 160.0;

/// Height of the driver section. The section shows the driver table or the "no
/// driver data" text, and these have very different sizes. Both use this
/// height, so that the page does not move when telemetry starts.
const DRIVER_SECTION_HEIGHT: f32 = 320.0;

/// One `caption: value` row in an info card.
///
/// The value is monospace and the caption column has a fixed width. Thus, when
/// a live value gets one more digit, the row text changes but nothing moves.
fn info_row(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.allocate_ui(
            egui::vec2(INFO_LABEL_WIDTH, super::CARD_LINE_HEIGHT),
            |ui| {
                ui.label(RichText::new(label).color(Color32::LIGHT_GRAY));
            },
        );
        ui.label(super::num(value, 14.0, Color32::WHITE).strong());
    });
}

pub fn render_relative_tab(
    ui: &mut egui::Ui,
    state: &AppState,
    config: &mut RelativeOverlayConfig,
) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.heading(
                RichText::new("Relative & Lap Map Debug")
                    .size(24.0)
                    .strong()
                    .color(HEADER_TEXT_COLOR),
            );

            ui.add_space(12.0);

            // Top-level telemetry info
            egui::Frame::none()
                .fill(CARD_BACKGROUND_COLOR)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Telemetry Frame Info")
                            .size(18.0)
                            .strong()
                            .color(HEADER_TEXT_COLOR),
                    );
                    ui.add_space(8.0);

                    let frame = &state.latest_frame;

                    for (label, value) in [
                        ("Player Car Index:", format!("{}", frame.player_car_idx)),
                        ("Total Drivers:", format!("{}", frame.drivers.len())),
                        ("Session State:", format!("{}", frame.session_state)),
                        ("Current Lap:", format!("{}", frame.lap_current)),
                        ("Lap Distance %:", format!("{:.3}", frame.lap_dist_pct)),
                        (
                            "Position:",
                            format!("{} (P{} in class)", frame.position, frame.class_position),
                        ),
                    ] {
                        info_row(ui, label, value);
                    }
                });

            ui.add_space(12.0);

            // Relative Config Info
            egui::Frame::none()
                .fill(CARD_BACKGROUND_COLOR)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Relative Overlay Config")
                            .size(18.0)
                            .strong()
                            .color(HEADER_TEXT_COLOR),
                    );
                    ui.add_space(8.0);

                    for (label, value) in [
                        ("Enabled:", format!("{}", config.window.enabled)),
                        ("Show Minimap:", format!("{}", config.show_minimap)),
                        ("Minimap Mode:", format!("{:?}", config.minimap_mode)),
                        ("Num Positions:", format!("{}", config.num_positions)),
                        ("Buddy List:", format!("{}", config.buddy_list.len())),
                        ("Warning List:", format!("{}", config.warning_list.len())),
                        (
                            "Window Position:",
                            format!("({:.0}, {:.0})", config.window.x, config.window.y),
                        ),
                        (
                            "Window Size:",
                            format!("{:.0}x{:.0}", config.window.width, config.window.height),
                        ),
                    ] {
                        info_row(ui, label, value);
                    }
                });

            ui.add_space(12.0);

            // Buddy List Management
            egui::Frame::none()
                .fill(CARD_BACKGROUND_COLOR)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Buddy List (Friends)")
                            .size(18.0)
                            .strong()
                            .color(Color32::from_rgb(100, 200, 100)),
                    );
                    ui.add_space(8.0);

                    // Both lists below have one row for each driver. A scroll
                    // box with a fixed size keeps the card size the same when
                    // cars join and leave the session.
                    let mut to_remove = Vec::new();
                    egui::ScrollArea::vertical()
                        .id_salt("buddy_list")
                        .max_height(LIST_HEIGHT)
                        .min_scrolled_height(LIST_HEIGHT)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // Current buddies, with remove buttons.
                            for (idx, name) in config.buddy_list.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("✓").color(Color32::from_rgb(100, 200, 100)),
                                    );
                                    ui.label(RichText::new(name).color(Color32::LIGHT_GRAY));
                                    if ui.small_button("Remove").clicked() {
                                        to_remove.push(idx);
                                    }
                                });
                            }

                            // Add a buddy.
                            ui.add_space(4.0);
                            ui.label(RichText::new("Add from current drivers:").size(12.0));

                            // Drivers that you can add.
                            for driver in &state.latest_frame.drivers {
                                let name = driver.get_user_name();
                                if !config.buddy_list.contains(&name)
                                    && !config.warning_list.contains(&name)
                                {
                                    ui.horizontal(|ui| {
                                        if ui.small_button("+ Add").clicked() {
                                            config.buddy_list.push(name.clone());
                                        }
                                        ui.label(
                                            RichText::new(&name)
                                                .size(11.0)
                                                .color(Color32::LIGHT_GRAY),
                                        );
                                    });
                                }
                            }
                        });

                    // Remove marked items
                    for &idx in to_remove.iter().rev() {
                        config.buddy_list.remove(idx);
                    }
                });

            ui.add_space(12.0);

            // Warning List Management
            egui::Frame::none()
                .fill(CARD_BACKGROUND_COLOR)
                .rounding(8.0)
                .inner_margin(16.0)
                .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(80, 40, 40)))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("⚠ Warning List (Dangerous Drivers)")
                            .size(18.0)
                            .strong()
                            .color(Color32::from_rgb(255, 150, 150)),
                    );
                    ui.add_space(8.0);

                    let mut to_remove = Vec::new();
                    egui::ScrollArea::vertical()
                        .id_salt("warning_list")
                        .max_height(LIST_HEIGHT)
                        .min_scrolled_height(LIST_HEIGHT)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // Current warnings, with remove buttons.
                            for (idx, name) in config.warning_list.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("⚠").color(Color32::from_rgb(255, 200, 0)),
                                    );
                                    ui.label(
                                        RichText::new(name).color(Color32::from_rgb(255, 150, 150)),
                                    );
                                    if ui.small_button("Remove").clicked() {
                                        to_remove.push(idx);
                                    }
                                });
                            }

                            // Add a warning.
                            ui.add_space(4.0);
                            ui.label(RichText::new("Add from current drivers:").size(12.0));

                            // Drivers that you can add.
                            for driver in &state.latest_frame.drivers {
                                let name = driver.get_user_name();
                                if !config.warning_list.contains(&name)
                                    && !config.buddy_list.contains(&name)
                                {
                                    ui.horizontal(|ui| {
                                        if ui.small_button("+ Warn").clicked() {
                                            config.warning_list.push(name.clone());
                                        }
                                        ui.label(
                                            RichText::new(&name)
                                                .size(11.0)
                                                .color(Color32::LIGHT_GRAY),
                                        );
                                    });
                                }
                            }
                        });

                    // Remove marked items
                    for &idx in to_remove.iter().rev() {
                        config.warning_list.remove(idx);
                    }
                });

            ui.add_space(12.0);

            // Debug data that shows why the driver list can be empty.
            if state.latest_frame.drivers.is_empty() {
                egui::Frame::none()
                    .fill(Color32::from_rgb(60, 40, 30))
                    .rounding(8.0)
                    .inner_margin(16.0)
                    .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(200, 100, 50)))
                    .show(ui, |ui| {
                        // This height is the same as the driver table. When
                        // telemetry starts, the table replaces this frame.
                        // Without the same height, this change makes the
                        // largest movement on the page.
                        ui.set_min_height(DRIVER_SECTION_HEIGHT);
                        ui.label(
                            RichText::new("⚠ No Driver Data Available")
                                .size(18.0)
                                .strong()
                                .color(Color32::from_rgb(255, 180, 100)),
                        );
                        ui.add_space(8.0);

                        ui.label(
                            RichText::new("Possible reasons:")
                                .size(14.0)
                                .color(Color32::LIGHT_GRAY),
                        );
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("•").size(14.0).color(Color32::GRAY));
                            ui.label(
                                RichText::new("No telemetry data is being received on port 5555")
                                    .size(13.0)
                                    .color(Color32::LIGHT_GRAY),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("•").size(14.0).color(Color32::GRAY));
                            ui.label(
                                RichText::new(
                                    "Running on non-Windows platform (iRacing SDK not available)",
                                )
                                .size(13.0)
                                .color(Color32::LIGHT_GRAY),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("•").size(14.0).color(Color32::GRAY));
                            ui.label(
                                RichText::new(
                                    "Session YAML parsing may have failed (check console logs)",
                                )
                                .size(13.0)
                                .color(Color32::LIGHT_GRAY),
                            );
                        });

                        ui.add_space(8.0);

                        ui.label(
                            RichText::new("Expected setup:")
                                .size(14.0)
                                .strong()
                                .color(Color32::from_rgb(150, 200, 255)),
                        );
                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("→").size(14.0).color(Color32::GRAY));
                            ui.label(
                                RichText::new("Run telemetry sender on Windows with iRacing")
                                    .size(13.0)
                                    .color(Color32::LIGHT_GRAY),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("→").size(14.0).color(Color32::GRAY));
                            ui.label(
                                RichText::new(
                                    "Configure sender to connect to this machine's IP:5555",
                                )
                                .size(13.0)
                                .color(Color32::LIGHT_GRAY),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("→").size(14.0).color(Color32::GRAY));
                            ui.label(
                                RichText::new("Or run this app on Windows with iRacing running")
                                    .size(13.0)
                                    .color(Color32::LIGHT_GRAY),
                            );
                        });

                        ui.add_space(8.0);

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Current Platform:").color(Color32::LIGHT_GRAY));
                            ui.label(RichText::new(std::env::consts::OS).strong().color(
                                if cfg!(windows) {
                                    Color32::GREEN
                                } else {
                                    Color32::YELLOW
                                },
                            ));
                        });

                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Driver Count:").color(Color32::LIGHT_GRAY));
                            ui.label(
                                RichText::new("0")
                                    .strong()
                                    .color(Color32::from_rgb(255, 150, 150)),
                            );
                        });
                    });
            }

            // Driver Data Table
            if !state.latest_frame.drivers.is_empty() {
                egui::Frame::none()
                    .fill(CARD_BACKGROUND_COLOR)
                    .rounding(8.0)
                    .inner_margin(16.0)
                    .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                    .show(ui, |ui| {
                        ui.set_min_height(DRIVER_SECTION_HEIGHT);
                        ui.label(
                            super::num(
                                format!(
                                    "Driver Data ({} drivers)",
                                    state.latest_frame.drivers.len()
                                ),
                                18.0,
                                HEADER_TEXT_COLOR,
                            )
                            .strong(),
                        );
                        ui.add_space(8.0);

                        let player_idx = state.latest_frame.player_car_idx;

                        TableBuilder::new(ui)
                            .striped(true)
                            .vscroll(true)
                            .max_scroll_height(400.0)
                            .column(Column::exact(50.0)) // Idx
                            .column(Column::exact(150.0)) // Name
                            .column(Column::exact(60.0)) // Car #
                            .column(Column::exact(50.0)) // Pos
                            .column(Column::exact(50.0)) // Lap
                            .column(Column::exact(80.0)) // Lap Dist %
                            .column(Column::exact(80.0)) // Est Time
                            .column(Column::exact(70.0)) // Speed
                            .column(Column::exact(80.0)) // Best Lap
                            .column(Column::exact(80.0)) // Last Lap
                            .column(Column::exact(80.0)) // On Pit Road
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.strong("Idx");
                                });
                                header.col(|ui| {
                                    ui.strong("Name");
                                });
                                header.col(|ui| {
                                    ui.strong("Car #");
                                });
                                header.col(|ui| {
                                    ui.strong("Pos");
                                });
                                header.col(|ui| {
                                    ui.strong("Lap");
                                });
                                header.col(|ui| {
                                    ui.strong("Lap Dist %");
                                });
                                header.col(|ui| {
                                    ui.strong("Est Time");
                                });
                                header.col(|ui| {
                                    ui.strong("Speed");
                                });
                                header.col(|ui| {
                                    ui.strong("Best Lap");
                                });
                                header.col(|ui| {
                                    ui.strong("Last Lap");
                                });
                                header.col(|ui| {
                                    ui.strong("Pit Road");
                                });
                            })
                            .body(|mut body| {
                                // Sort the drivers by position.
                                let mut drivers: Vec<_> =
                                    state.latest_frame.drivers.iter().collect();
                                drivers.sort_by_key(|d| d.position);

                                for driver in drivers {
                                    let is_player = driver.car_idx == player_idx;
                                    let row_color = if is_player {
                                        Color32::from_rgb(80, 120, 80)
                                    } else {
                                        Color32::LIGHT_GRAY
                                    };

                                    body.row(20.0, |mut row| {
                                        row.col(|ui| {
                                            ui.colored_label(
                                                row_color,
                                                format!("{}", driver.car_idx),
                                            );
                                        });
                                        row.col(|ui| {
                                            let name = driver.get_user_name();
                                            let display_name = if name.len() > 20 {
                                                format!("{}...", &name[..17])
                                            } else {
                                                name
                                            };
                                            ui.colored_label(row_color, display_name);
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(row_color, driver.get_car_number());
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(
                                                row_color,
                                                format!("{}", driver.position),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(row_color, format!("{}", driver.lap));
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(
                                                row_color,
                                                format!("{:.3}", driver.lap_dist_pct),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(
                                                row_color,
                                                format!("{:.2}s", driver.est_time),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.colored_label(
                                                row_color,
                                                format!("{:.1}", driver.speed),
                                            );
                                        });
                                        row.col(|ui| {
                                            if driver.lap_best_time > 0.0 {
                                                ui.colored_label(
                                                    row_color,
                                                    format!("{:.2}s", driver.lap_best_time),
                                                );
                                            } else {
                                                ui.colored_label(Color32::DARK_GRAY, "N/A");
                                            }
                                        });
                                        row.col(|ui| {
                                            if driver.lap_last_time > 0.0 {
                                                ui.colored_label(
                                                    row_color,
                                                    format!("{:.2}s", driver.lap_last_time),
                                                );
                                            } else {
                                                ui.colored_label(Color32::DARK_GRAY, "N/A");
                                            }
                                        });
                                        row.col(|ui| {
                                            let pit_text =
                                                if driver.on_pit_road == 1 { "YES" } else { "NO" };
                                            let pit_color = if driver.on_pit_road == 1 {
                                                Color32::from_rgb(255, 200, 100)
                                            } else {
                                                row_color
                                            };
                                            ui.colored_label(pit_color, pit_text);
                                        });
                                    });
                                }
                            });
                    });
            } else {
                egui::Frame::none()
                    .fill(CARD_BACKGROUND_COLOR)
                    .rounding(8.0)
                    .inner_margin(40.0)
                    .show(ui, |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new("No driver data available")
                                    .size(18.0)
                                    .color(Color32::GRAY),
                            );
                        });
                    });
            }

            ui.add_space(12.0);

            // Relative Calculations Debug
            if !state.latest_frame.drivers.is_empty() {
                let frame = &state.latest_frame;
                let player_idx = frame.player_car_idx;

                if let Some(player) = frame.drivers.iter().find(|d| d.car_idx == player_idx) {
                    egui::Frame::none()
                        .fill(CARD_BACKGROUND_COLOR)
                        .rounding(8.0)
                        .inner_margin(16.0)
                        .stroke(egui::Stroke::new(2.0_f32, Color32::from_rgb(60, 70, 80)))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Relative Calculations (from player perspective)")
                                    .size(18.0)
                                    .strong()
                                    .color(HEADER_TEXT_COLOR),
                            );
                            ui.add_space(8.0);

                            let player_track_pos = player.lap as f32 + player.lap_dist_pct;
                            let player_est_time = player.est_time;

                            info_row(
                                ui,
                                "Player Track Position:",
                                format!("{player_track_pos:.3}"),
                            );
                            info_row(ui, "Player Est Time:", format!("{player_est_time:.2}s"));

                            ui.add_space(12.0);

                            // Calculate and show the relative positions.
                            let mut relative_positions: Vec<_> = frame
                                .drivers
                                .iter()
                                .map(|driver| {
                                    let driver_track_pos = driver.lap as f32 + driver.lap_dist_pct;
                                    let lap_delta = driver_track_pos - player_track_pos;

                                    let time_delta =
                                        if driver.est_time > 0.0 && player_est_time > 0.0 {
                                            driver.est_time - player_est_time
                                        } else {
                                            let reference_lap_time = if driver.lap_last_time > 0.0 {
                                                driver.lap_last_time
                                            } else if driver.lap_best_time > 0.0 {
                                                driver.lap_best_time
                                            } else {
                                                0.0
                                            };
                                            lap_delta * reference_lap_time
                                        };

                                    (driver, lap_delta, time_delta)
                                })
                                .collect();

                            // Sort by time delta.
                            relative_positions.sort_by(|a, b| {
                                a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal)
                            });

                            TableBuilder::new(ui)
                                .striped(true)
                                .vscroll(true)
                                .max_scroll_height(400.0)
                                .column(Column::exact(50.0)) // Idx
                                .column(Column::exact(150.0)) // Name
                                .column(Column::exact(60.0)) // Car #
                                .column(Column::exact(100.0)) // Lap Delta
                                .column(Column::exact(100.0)) // Time Delta
                                .column(Column::exact(100.0)) // Track Pos
                                .header(20.0, |mut header| {
                                    header.col(|ui| {
                                        ui.strong("Idx");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Name");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Car #");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Lap Delta");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Time Delta");
                                    });
                                    header.col(|ui| {
                                        ui.strong("Track Pos");
                                    });
                                })
                                .body(|mut body| {
                                    for (driver, lap_delta, time_delta) in relative_positions {
                                        let is_player = driver.car_idx == player_idx;
                                        let row_color = if is_player {
                                            Color32::from_rgb(100, 255, 100)
                                        } else if time_delta < 0.0 {
                                            Color32::from_rgb(255, 150, 150) // Behind
                                        } else {
                                            Color32::from_rgb(150, 150, 255) // Ahead
                                        };

                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.colored_label(
                                                    row_color,
                                                    format!("{}", driver.car_idx),
                                                );
                                            });
                                            row.col(|ui| {
                                                let name = driver.get_user_name();
                                                let display_name = if name.len() > 20 {
                                                    format!("{}...", &name[..17])
                                                } else {
                                                    name
                                                };
                                                ui.colored_label(row_color, display_name);
                                            });
                                            row.col(|ui| {
                                                ui.colored_label(
                                                    row_color,
                                                    driver.get_car_number(),
                                                );
                                            });
                                            row.col(|ui| {
                                                let sign = if lap_delta >= 0.0 { "+" } else { "" };
                                                ui.colored_label(
                                                    row_color,
                                                    format!("{}{:.3} laps", sign, lap_delta),
                                                );
                                            });
                                            row.col(|ui| {
                                                let sign = if time_delta >= 0.0 { "+" } else { "" };
                                                ui.colored_label(
                                                    row_color,
                                                    format!("{}{:.2}s", sign, time_delta),
                                                );
                                            });
                                            row.col(|ui| {
                                                ui.colored_label(
                                                    row_color,
                                                    format!(
                                                        "{:.3}",
                                                        driver.lap as f32 + driver.lap_dist_pct
                                                    ),
                                                );
                                            });
                                        });
                                    }
                                });
                        });
                } else {
                    egui::Frame::none()
                        .fill(CARD_BACKGROUND_COLOR)
                        .rounding(8.0)
                        .inner_margin(40.0)
                        .show(ui, |ui| {
                            ui.centered_and_justified(|ui| {
                                ui.label(
                                    RichText::new("Player not found in driver data")
                                        .size(18.0)
                                        .color(Color32::from_rgb(255, 100, 100)),
                                );
                            });
                        });
                }
            }
        }); // ScrollArea
}
