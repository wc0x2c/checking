use eframe::egui;
use egui::{Color32, RichText};
use checking_fuel::{SenderState, SenderStatus};

pub fn render_sender_tab(ui: &mut egui::Ui, sender_state: &SenderState) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
    ui.heading(RichText::new("iRacing Sender").size(24.0));
    ui.add_space(8.0);

    // Connection status card
    egui::Frame::none()
        .fill(match &sender_state.status {
            SenderStatus::Streaming { .. } => Color32::from_rgb(20, 60, 30),
            SenderStatus::Error(_) => Color32::from_rgb(80, 20, 20),
            _ => Color32::from_rgb(60, 50, 20),
        })
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(
            2.0,
            match &sender_state.status {
                SenderStatus::Streaming { .. } => Color32::GREEN,
                SenderStatus::Error(_) => Color32::RED,
                _ => Color32::YELLOW,
            },
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon, text, color) = match &sender_state.status {
                    SenderStatus::Disconnected => (
                        "⭕",
                        "Disconnected - Not running".to_string(),
                        Color32::GRAY,
                    ),
                    SenderStatus::ConnectingToIRacing => (
                        "🔄",
                        "Connecting to iRacing...".to_string(),
                        Color32::YELLOW,
                    ),
                    SenderStatus::WaitingForSession => (
                        "⏳",
                        "Waiting for iRacing session...".to_string(),
                        Color32::YELLOW,
                    ),
                    SenderStatus::ConnectingToReceiver => (
                        "🔄",
                        format!(
                            "Connecting to receiver at {}...",
                            sender_state.target_address
                        ),
                        Color32::YELLOW,
                    ),
                    SenderStatus::Streaming { hz, frames_sent } => (
                        "",
                        format!("Streaming at {} Hz ({} frames sent)", hz, frames_sent),
                        Color32::GREEN,
                    ),
                    SenderStatus::Error(msg) => ("", format!("Error: {}", msg), Color32::RED),
                };

                ui.label(RichText::new(icon).size(28.0).color(color));
                ui.label(RichText::new(text).size(20.0).strong().color(color));
            });
        });

    ui.add_space(16.0);

    // Configuration section
    egui::Frame::none()
        .fill(Color32::from_rgb(40, 40, 50))
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(80, 80, 100)))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Configuration")
                    .size(18.0)
                    .strong()
                    .color(Color32::from_rgb(200, 200, 220)),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Target Address:")
                        .size(14.0)
                        .color(Color32::GRAY),
                );
                ui.label(
                    RichText::new(&sender_state.target_address)
                        .size(14.0)
                        .color(Color32::WHITE)
                        .monospace(),
                );
            });
        });

    ui.add_space(16.0);

    // Status details section
    egui::Frame::none()
        .fill(Color32::from_rgb(40, 40, 50))
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(80, 80, 100)))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Status Details")
                    .size(18.0)
                    .strong()
                    .color(Color32::from_rgb(200, 200, 220)),
            );
            ui.add_space(8.0);

            match &sender_state.status {
                SenderStatus::Disconnected => {
                    ui.label(
                        RichText::new("The sender is not currently running.")
                            .size(14.0)
                            .color(Color32::GRAY),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("The application will automatically attempt to connect to iRacing when it's running.")
                            .size(14.0)
                            .color(Color32::GRAY),
                    );
                }
                SenderStatus::ConnectingToIRacing => {
                    ui.label(
                        RichText::new("Attempting to connect to iRacing shared memory...")
                            .size(14.0)
                            .color(Color32::YELLOW),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Make sure iRacing is running and you're in a session.")
                            .size(14.0)
                            .color(Color32::GRAY),
                    );
                }
                SenderStatus::WaitingForSession => {
                    ui.label(
                        RichText::new("Connected to iRacing, waiting for active session...")
                            .size(14.0)
                            .color(Color32::YELLOW),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Join a session or start a test drive to begin streaming telemetry.")
                            .size(14.0)
                            .color(Color32::GRAY),
                    );
                }
                SenderStatus::ConnectingToReceiver => {
                    ui.label(
                        RichText::new(format!("Connecting to receiver at {}...", sender_state.target_address))
                            .size(14.0)
                            .color(Color32::YELLOW),
                    );
                }
                SenderStatus::Streaming { hz, frames_sent } => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("●").size(20.0).color(Color32::GREEN));
                        ui.label(
                            RichText::new("Actively streaming telemetry data")
                                .size(14.0)
                                .color(Color32::GREEN),
                        );
                    });
                    ui.add_space(8.0);

                    use egui_extras::{Column, TableBuilder};
                    TableBuilder::new(ui)
                        .striped(false)
                        .column(Column::exact(150.0))
                        .column(Column::remainder())
                        .body(|mut body| {
                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(RichText::new("Update Rate:").size(14.0).color(Color32::GRAY));
                                });
                                row.col(|ui| {
                                    ui.label(RichText::new(format!("{} Hz", hz)).size(14.0).color(Color32::WHITE));
                                });
                            });
                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(RichText::new("Frames Sent:").size(14.0).color(Color32::GRAY));
                                });
                                row.col(|ui| {
                                    ui.label(RichText::new(format!("{}", frames_sent)).size(14.0).color(Color32::WHITE));
                                });
                            });
                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(RichText::new("Target:").size(14.0).color(Color32::GRAY));
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(&sender_state.target_address)
                                            .size(14.0)
                                            .color(Color32::WHITE)
                                            .monospace(),
                                    );
                                });
                            });
                        });
                }
                SenderStatus::Error(msg) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("✗").size(20.0).color(Color32::RED));
                        ui.label(
                            RichText::new("An error occurred:")
                                .size(14.0)
                                .color(Color32::RED),
                        );
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(msg)
                            .size(14.0)
                            .color(Color32::from_rgb(255, 150, 150))
                            .monospace(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("The application will automatically retry...")
                            .size(14.0)
                            .color(Color32::GRAY),
                    );
                }
            }
        });

    ui.add_space(16.0);

    // Information section
    egui::Frame::none()
        .fill(Color32::from_rgb(30, 35, 45))
        .rounding(8.0)
        .inner_margin(16.0)
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(60, 70, 90)))
        .show(ui, |ui| {
            ui.label(
                RichText::new("ℹ About the Sender")
                    .size(16.0)
                    .strong()
                    .color(Color32::from_rgb(150, 180, 220)),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new("The sender connects to iRacing's shared memory and streams telemetry data to this application.")
                    .size(13.0)
                    .color(Color32::LIGHT_GRAY),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("It will automatically:")
                    .size(13.0)
                    .color(Color32::LIGHT_GRAY),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("  •").size(13.0).color(Color32::LIGHT_GRAY));
                ui.label(
                    RichText::new("Detect when iRacing is running")
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("  •").size(13.0).color(Color32::LIGHT_GRAY));
                ui.label(
                    RichText::new("Wait for an active session")
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("  •").size(13.0).color(Color32::LIGHT_GRAY));
                ui.label(
                    RichText::new("Stream telemetry at ~60 Hz")
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("  •").size(13.0).color(Color32::LIGHT_GRAY));
                ui.label(
                    RichText::new("Retry on connection errors")
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );
            });
        });

    if let Some(error) = &sender_state.last_error {
        ui.add_space(16.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(60, 30, 30))
            .rounding(8.0)
            .inner_margin(12.0)
            .stroke(egui::Stroke::new(2.0, Color32::from_rgb(200, 80, 80)))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("⚠ Last Error")
                        .size(16.0)
                        .strong()
                        .color(Color32::from_rgb(255, 150, 150)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(error)
                        .size(13.0)
                        .color(Color32::from_rgb(255, 180, 180))
                        .monospace(),
                );
            });
    }
    }); // ScrollArea
}
