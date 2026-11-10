// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use checking_fuel::{AppState, SenderState};
use checking_protocol::TelemetryFrame;
use eframe::egui;
use egui::{Color32, RichText};
use std::sync::{Arc, Mutex};

mod config;
mod lap_fuel;
mod overlay;
mod pages;
mod radar;
mod relative;
/// Synthetic race data for the `strategy` unit tests.
#[cfg(test)]
mod scenario;
mod strategy;
// Only Windows has the joystick and keyboard backend that starts the worker. The other targets
// show and save the settings only.
#[cfg_attr(not(windows), allow(dead_code))]
mod weight_jacker;

use config::OverlayConfig;

/// Opens a file or directory in the system file manager.
fn open_file_location(path: &std::path::Path) -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }

    Ok(())
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 1000.0])
            .with_title("Checking")
            .with_min_inner_size([1000.0, 700.0]),
        ..Default::default()
    };

    let app_state = Arc::new(Mutex::new(AppState::new()));
    let sender_state = Arc::new(Mutex::new(SenderState::new("127.0.0.1:5555".to_string())));

    let state_clone = app_state.clone();
    let sender_state_clone = sender_state.clone();

    let overlay_config = OverlayConfig::load();
    let weight_jacker_config = Arc::new(Mutex::new(overlay_config.weight_jacker));
    #[cfg(windows)]
    weight_jacker::spawn(weight_jacker_config.clone());

    // Start the network listener and the sender on a background thread.
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Start the sender loop.
            let sender_future = checking_iracingio::run_sender_loop(sender_state_clone);

            // Start the receiver loop.
            let receiver_future = run_network_listener(state_clone);

            // Run both at the same time.
            tokio::select! {
                result = receiver_future => {
                    if let Err(e) = result {
                        eprintln!("Network receiver error: {}", e);
                    }
                }
                _ = sender_future => {
                    eprintln!("Sender loop ended unexpectedly");
                }
            }
        });
    });

    eframe::run_native(
        "Checking",
        options,
        Box::new(|cc| {
            // Set dark mode and the custom style.
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.window_margin = egui::Margin::same(12.0);
            cc.egui_ctx.set_style(style);

            Ok(Box::new(FuelApp {
                state: app_state,
                sender_state,
                weight_jacker_config,
                current_tab: Tab::Main,
                overlay_config,
                frame_count: 0,
                buddy_input: String::new(),
                warning_input: String::new(),
                simulation_cache: None,
                fuel_estimates_cache: None,
            }))
        }),
    )
}

// Cache key for the race simulation results.
#[derive(Debug, Clone, PartialEq)]
struct SimulationCacheKey {
    current_lap: i32,
    fuel_level_rounded: i32, // Round to avoid cache misses on tiny fuel changes
    total_laps: i32,
}

// Fuel estimates cache. The app calculates it one time in each frame, and all pages use it.
#[derive(Debug, Clone)]
struct FuelEstimates {
    low_fuel_per_lap: Option<f32>,
    avg_fuel_per_lap: Option<f32>,
    push_fuel_per_lap: Option<f32>,
    max_fuel_per_lap: Option<f32>,
}

struct FuelApp {
    state: Arc<Mutex<AppState>>,
    sender_state: Arc<Mutex<SenderState>>,
    /// The weight jacker settings that the input thread reads. The Options tab writes them.
    weight_jacker_config: Arc<Mutex<weight_jacker::WeightJackerConfig>>,
    current_tab: Tab,
    overlay_config: OverlayConfig,
    frame_count: u64,
    buddy_input: String,
    warning_input: String,
    // Cache for the race simulations (4 strategies). They take a long time to calculate.
    simulation_cache: Option<(SimulationCacheKey, [pages::CachedSimResult; 4])>,
    // Cache for the fuel estimates. Calculated one time in each frame, and cleared when the lap
    // data changes.
    fuel_estimates_cache: Option<(usize, FuelEstimates)>, // (lap_data.len(), estimates)
}

#[derive(PartialEq)]
enum Tab {
    Main,
    Debug,
    Sender,
    Options,
    Relative,
    Errors,
}

// Renders the header.
fn render_header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.heading(
            RichText::new("Checking")
                .size(38.0)
                .strong()
                .color(Color32::from_rgb(220, 30, 30)),
        );
    });
}

// Renders the connection status.
fn render_connection_status(ui: &mut egui::Ui, state: &AppState) {
    egui::Frame::none()
        .fill(if state.connected {
            Color32::from_rgb(20, 60, 30)
        } else {
            Color32::from_rgb(60, 50, 20)
        })
        .rounding(6.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            // Both branches use one row with a fixed height. Thus a connect or disconnect does not
            // move the tab bar and the page below.
            ui.set_min_height(pages::CARD_LINE_HEIGHT);
            ui.horizontal(|ui| {
                if state.connected {
                    // Hz and latency change in each frame. Monospace text keeps the label width the
                    // same when the number of digits changes.
                    ui.label(pages::num(
                        format!(
                            "Connected - {} Hz | Latency: {}ms",
                            state.current_hz, state.latency_ms
                        ),
                        14.0,
                        Color32::GREEN,
                    ));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(pages::num(
                            format!(
                                "Lap {} | Leader: L{} | Gap: {} lap(s)",
                                state.current_lap, state.latest_frame.leader_lap, state.laps_down
                            ),
                            14.0,
                            Color32::LIGHT_GRAY,
                        ));
                    });
                } else {
                    ui.label(
                        RichText::new("Waiting for connection on 0.0.0.0:5555...")
                            .size(14.0)
                            .color(Color32::YELLOW),
                    );
                }
            });
        });
}

impl eframe::App for FuelApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Repaint at 30 FPS (33 ms) for telemetry updates. This uses less CPU than a higher rate
        // and is fast enough for race data.
        ctx.request_repaint_after(std::time::Duration::from_millis(33));

        self.frame_count += 1;

        // Calculate the fuel estimates one time in each frame, before the state is locked for
        // rendering.
        let fuel_estimates = {
            let state = self.state.lock().unwrap();
            let lap_data_len = state.lap_data.len();

            // Use the cache if it is still valid.
            let needs_recompute = self
                .fuel_estimates_cache
                .as_ref()
                .map(|(cached_len, _)| *cached_len != lap_data_len)
                .unwrap_or(true);

            if needs_recompute {
                let total_laps = state.get_effective_total_laps();
                let estimates = FuelEstimates {
                    low_fuel_per_lap: state.avg_fuel_low_pct_of_total(total_laps),
                    avg_fuel_per_lap: state.avg_fuel_pct_of_total(total_laps),
                    push_fuel_per_lap: state.avg_fuel_push_pct_of_total(total_laps),
                    max_fuel_per_lap: state.max_fuel_usage(),
                };
                self.fuel_estimates_cache = Some((lap_data_len, estimates.clone()));
                estimates
            } else {
                self.fuel_estimates_cache.as_ref().unwrap().1.clone()
            }
        };

        {
            let mut state = self.state.lock().unwrap();

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_space(8.0);

                // Header.
                render_header(ui);

                ui.add_space(4.0);

                // Connection status bar.
                render_connection_status(ui, &state);

                ui.add_space(8.0);

                // Tab selector.
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.current_tab, Tab::Main, "Fuel");
                    ui.selectable_value(&mut self.current_tab, Tab::Debug, "Debug");
                    ui.selectable_value(&mut self.current_tab, Tab::Sender, "Sender");
                    ui.selectable_value(&mut self.current_tab, Tab::Options, "Options");
                    ui.selectable_value(&mut self.current_tab, Tab::Relative, "Relative");

                    // Always show the count, also when it is zero. If the label changes between
                    // "Errors" and "Errors (3)", the button width changes and all tabs in the row
                    // move. The tab bar is on all pages, so this occurs on all tabs.
                    let errors_label = format!("Errors ({})", state.parse_errors.len());
                    ui.selectable_value(&mut self.current_tab, Tab::Errors, errors_label);
                });

                ui.add_space(12.0);

                // Show the content of the selected tab.
                match self.current_tab {
                    Tab::Main => {
                        // Get the simulations for the fuel tab from the cache, or calculate them.
                        let total_laps = state.get_effective_total_laps();
                        let current_lap = state.latest_frame.lap_current;
                        let fuel_level = state.latest_frame.fuel_level;
                        let fuel_rounded = (fuel_level * 10.0) as i32; // Round to 0.1L precision

                        let cache_key = SimulationCacheKey {
                            current_lap,
                            fuel_level_rounded: fuel_rounded,
                            total_laps,
                        };

                        // Use the cache if it is still valid.
                        let needs_recompute = self
                            .simulation_cache
                            .as_ref()
                            .map(|(key, _)| key != &cache_key)
                            .unwrap_or(true);

                        if needs_recompute {
                            // Use the fuel estimates that the app calculated before.
                            let low = fuel_estimates.low_fuel_per_lap;
                            let avg = fuel_estimates.avg_fuel_per_lap;
                            let push = fuel_estimates.push_fuel_per_lap;
                            let max = fuel_estimates.max_fuel_per_lap;

                            if let (Some(low), Some(avg), Some(push), Some(max)) =
                                (low, avg, push, max)
                            {
                                let current_lap_fractional = state.get_current_lap_fractional();
                                let fuel_capacity = state.get_fuel_capacity();

                                let results = pages::compute_all_simulations(
                                    fuel_level,
                                    current_lap_fractional,
                                    total_laps,
                                    fuel_capacity,
                                    [low, avg, push, max],
                                );

                                self.simulation_cache = Some((cache_key, results));
                            } else {
                                self.simulation_cache = None;
                            }
                        }

                        let sim_results = self.simulation_cache.as_ref().map(|(_, r)| r);
                        pages::render_fuel_tab(
                            ui,
                            &state,
                            sim_results,
                            self.overlay_config.extra_liters,
                            self.overlay_config.fuel_tank_pct,
                        );
                    }
                    Tab::Debug => {
                        pages::render_debug_tab(ui, &state);
                    }
                    Tab::Sender => {
                        let sender_state = self.sender_state.lock().unwrap();
                        pages::render_sender_tab(ui, &sender_state);
                    }
                    Tab::Options => {
                        render_options_tab(
                            ui,
                            &mut self.overlay_config,
                            &mut self.buddy_input,
                            &mut self.warning_input,
                        );
                        *self.weight_jacker_config.lock().unwrap() =
                            self.overlay_config.weight_jacker;
                    }
                    Tab::Relative => {
                        pages::render_relative_tab(ui, &state, &mut self.overlay_config.relative);
                    }
                    Tab::Errors => {
                        pages::render_errors_tab(ui, &mut state);
                    }
                } // End of match
            });
        } // Drop state lock here

        // Render the overlay windows. They lock the state again.
        self.render_overlay_windows(ctx, &fuel_estimates);
    }
}

impl FuelApp {
    fn render_overlay_windows(&mut self, ctx: &egui::Context, _fuel_estimates: &FuelEstimates) {
        let state = self.state.lock().unwrap();

        // Laps Remaining overlay: a separate viewport (external window).
        if self.overlay_config.laps_remaining.enabled {
            let cfg = &self.overlay_config.laps_remaining;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("Laps Remaining")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([235.0, 110.0]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("laps_remaining_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    // Close the window when the user pushes ESC.
                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.laps_remaining.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_laps_remaining_overlay(ui, &state);

                            panel_response = Some(response);
                        });

                    // Drag the window, only if the overlays are unlocked.
                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                // Move the window by the drag delta. Write the config directly, so
                                // that the viewport position does not feed back.
                                let cfg = &mut self.overlay_config.laps_remaining;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked {
                            // Read the position from the viewport only when the user does not drag.
                            if let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect) {
                                let cfg = &mut self.overlay_config.laps_remaining;
                                cfg.x = outer_rect.left();
                                cfg.y = outer_rect.top();
                            }
                        }
                    }

                    // The size is fixed, so do not read the dimensions.

                    // Find out if the user closed the window.
                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.laps_remaining.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // Fuel to Add overlay: a separate viewport with a fixed size. The lap fuel card is below the
        // fuel to add card.
        if self.overlay_config.fuel_to_add.enabled {
            let cfg = &self.overlay_config.fuel_to_add;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("Fuel to Add")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size(overlay::FUEL_TO_ADD_OVERLAY_SIZE);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("fuel_to_add_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.fuel_to_add.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_fuel_to_add_overlay(
                                ui,
                                &state,
                                self.overlay_config.extra_liters,
                                self.overlay_config.fuel_tank_pct,
                            );

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.fuel_to_add;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.fuel_to_add;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.fuel_to_add.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // AVG Laps Left overlay: a separate viewport with a fixed size.
        if self.overlay_config.avg_laps_left.enabled {
            let cfg = &self.overlay_config.avg_laps_left;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("AVG Laps Left")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([140.0, 120.0]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("avg_laps_left_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.avg_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = egui::Rgba::TRANSPARENT.into();
                            ui.visuals_mut().panel_fill = egui::Rgba::TRANSPARENT.into();

                            let response = overlay::render_avg_laps_left_overlay(
                                ui,
                                &state,
                                self.overlay_config.extra_liters,
                                self.overlay_config.fuel_tank_pct,
                            );

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.avg_laps_left;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.avg_laps_left;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    // The size is fixed, so do not read the dimensions.

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.avg_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // PUSH Laps Left overlay: a separate viewport with a fixed size.
        if self.overlay_config.push_laps_left.enabled {
            let cfg = &self.overlay_config.push_laps_left;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("PUSH Laps Left")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([140.0, 120.0]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("push_laps_left_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.push_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_push_laps_left_overlay(
                                ui,
                                &state,
                                self.overlay_config.extra_liters,
                                self.overlay_config.fuel_tank_pct,
                            );

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.push_laps_left;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.push_laps_left;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    // The size is fixed, so do not read the dimensions.

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.push_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // LOW Laps Left overlay: a separate viewport with a fixed size.
        if self.overlay_config.low_laps_left.enabled {
            let cfg = &self.overlay_config.low_laps_left;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("LOW Laps Left")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([140.0, 120.0]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("low_laps_left_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.low_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_low_laps_left_overlay(
                                ui,
                                &state,
                                self.overlay_config.extra_liters,
                                self.overlay_config.fuel_tank_pct,
                            );

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.low_laps_left;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.low_laps_left;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    // The size is fixed, so do not read the dimensions.

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.low_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // MAX Laps Left overlay: a separate viewport with a fixed size.
        if self.overlay_config.max_laps_left.enabled {
            let cfg = &self.overlay_config.max_laps_left;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("MAX Laps Left")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(false)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([140.0, 120.0]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("max_laps_left_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.max_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_max_laps_left_overlay(
                                ui,
                                &state,
                                self.overlay_config.extra_liters,
                                self.overlay_config.fuel_tank_pct,
                            );

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.max_laps_left;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.max_laps_left;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    // The size is fixed, so do not read the dimensions.

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.max_laps_left.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // Relative Standings overlay: a separate viewport.
        if self.overlay_config.relative.window.enabled {
            let cfg = &self.overlay_config.relative.window;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_title("Relative Standings")
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(!self.overlay_config.locked)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([cfg.width, cfg.height]);

            let relative_config = self.overlay_config.relative.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("relative_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.relative.window.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response =
                                overlay::render_relative_overlay(ui, &state, &relative_config);

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.relative.window;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.relative.window;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    if !self.overlay_config.locked
                        && let Some(inner_rect) = ctx.input(|i| i.viewport().inner_rect)
                    {
                        let cfg = &mut self.overlay_config.relative.window;
                        cfg.width = inner_rect.width();
                        cfg.height = inner_rect.height();
                    }

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.relative.window.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // Pedal Inputs overlay: a separate viewport.
        if self.overlay_config.pedal.window.enabled {
            let cfg = &self.overlay_config.pedal.window;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(!self.overlay_config.locked)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([cfg.width, cfg.height]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("pedal_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.pedal.window.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;

                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_pedal_overlay(
                                ui,
                                &state,
                                self.overlay_config.pedal.history_seconds,
                            );

                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked && response.dragged() {
                            if let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            }) {
                                let cfg = &mut self.overlay_config.pedal.window;
                                cfg.x += pos_delta.x;
                                cfg.y += pos_delta.y;
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    egui::pos2(cfg.x, cfg.y),
                                ));
                            }
                        } else if !self.overlay_config.locked
                            && let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect)
                        {
                            let cfg = &mut self.overlay_config.pedal.window;
                            cfg.x = outer_rect.left();
                            cfg.y = outer_rect.top();
                        }
                    }

                    if !self.overlay_config.locked
                        && let Some(inner_rect) = ctx.input(|i| i.viewport().inner_rect)
                    {
                        let cfg = &mut self.overlay_config.pedal.window;
                        cfg.width = inner_rect.width();
                        cfg.height = inner_rect.height();
                    }

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.pedal.window.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // Radar Left overlay: a separate viewport.
        if self.overlay_config.radar_left.enabled {
            let cfg = &self.overlay_config.radar_left;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(!self.overlay_config.locked)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([cfg.width, cfg.height]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("radar_left_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.radar_left.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_radar_left_overlay(
                                ui,
                                &state,
                                &self.overlay_config.radar,
                            );
                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked
                            && response.dragged()
                            && let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            })
                        {
                            let cfg = &mut self.overlay_config.radar_left;
                            cfg.x += pos_delta.x;
                            cfg.y += pos_delta.y;
                        }

                        if !self.overlay_config.locked && response.drag_stopped() {
                            let _ = self.overlay_config.save();
                        }

                        let inner_rect = response.rect;
                        if inner_rect.width() > 0.0 && inner_rect.height() > 0.0 {
                            let cfg = &mut self.overlay_config.radar_left;
                            cfg.width = inner_rect.width();
                            cfg.height = inner_rect.height();
                        }
                    }

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.radar_left.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }

        // Radar Right overlay: a separate viewport.
        if self.overlay_config.radar_right.enabled {
            let cfg = &self.overlay_config.radar_right;

            let viewport_builder = egui::ViewportBuilder::default()
                .with_position([cfg.x, cfg.y])
                .with_decorations(false)
                .with_resizable(!self.overlay_config.locked)
                .with_always_on_top()
                .with_transparent(true)
                .with_inner_size([cfg.width, cfg.height]);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("radar_right_overlay"),
                viewport_builder,
                |ctx, class| {
                    if class == egui::ViewportClass::Embedded {
                        return;
                    }

                    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                        self.overlay_config.radar_right.enabled = false;
                        let _ = self.overlay_config.save();
                        return;
                    }

                    let mut panel_response = None;
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            ui.spacing_mut().window_margin = egui::Margin::ZERO;
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.visuals_mut().window_fill = Color32::TRANSPARENT;
                            ui.visuals_mut().panel_fill = Color32::TRANSPARENT;

                            let response = overlay::render_radar_right_overlay(
                                ui,
                                &state,
                                &self.overlay_config.radar,
                            );
                            panel_response = Some(response);
                        });

                    if let Some(response) = panel_response {
                        if !self.overlay_config.locked
                            && response.dragged()
                            && let Some(pos_delta) = ctx.input(|i| {
                                if i.pointer.primary_down() {
                                    Some(i.pointer.delta())
                                } else {
                                    None
                                }
                            })
                        {
                            let cfg = &mut self.overlay_config.radar_right;
                            cfg.x += pos_delta.x;
                            cfg.y += pos_delta.y;
                        }

                        if !self.overlay_config.locked && response.drag_stopped() {
                            let _ = self.overlay_config.save();
                        }

                        let inner_rect = response.rect;
                        if inner_rect.width() > 0.0 && inner_rect.height() > 0.0 {
                            let cfg = &mut self.overlay_config.radar_right;
                            cfg.width = inner_rect.width();
                            cfg.height = inner_rect.height();
                        }
                    }

                    if ctx.input(|i| i.viewport().close_requested()) {
                        self.overlay_config.radar_right.enabled = false;
                        let _ = self.overlay_config.save();
                    }
                },
            );
        }
    }
}

/// Settings of the weight jacker buttons. See `weight_jacker`.
fn render_weight_jacker_settings(ui: &mut egui::Ui, config: &mut OverlayConfig) {
    egui::Frame::none()
        .fill(Color32::from_rgb(25, 30, 35))
        .rounding(8.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new("Weight Jacker")
                    .size(18.0)
                    .strong()
                    .color(Color32::LIGHT_BLUE),
            );
            ui.add_space(12.0);

            let wj = &mut config.weight_jacker;
            let mut changed = false;
            egui::Grid::new("weight_jacker_grid")
                .num_columns(2)
                .spacing([20.0, 12.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Enabled:").size(14.0));
                    changed |= ui.checkbox(&mut wj.enabled, "").changed();
                    ui.end_row();

                    ui.label(RichText::new("Joystick:").size(14.0));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut wj.joystick)
                                .range(1..=weight_jacker::MAX_JOYSTICK),
                        )
                        .changed();
                    ui.end_row();

                    ui.label(RichText::new("Up button:").size(14.0));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut wj.up_button)
                                .range(1..=weight_jacker::MAX_BUTTON),
                        )
                        .changed();
                    ui.end_row();

                    ui.label(RichText::new("Down button:").size(14.0));
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut wj.down_button)
                                .range(1..=weight_jacker::MAX_BUTTON),
                        )
                        .changed();
                    ui.end_row();

                    ui.label(RichText::new("Key presses:").size(14.0));
                    changed |= ui.add(egui::Slider::new(&mut wj.presses, 1..=20)).changed();
                    ui.end_row();

                    ui.label(RichText::new("Delay:").size(14.0));
                    changed |= ui
                        .add(egui::Slider::new(&mut wj.delay_ms, 0..=500).suffix(" ms"))
                        .changed();
                    ui.end_row();
                });
            if changed {
                let _ = config.save();
            }

            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "One press of a wheel button sends the up or down arrow key the set number \
                     of times. The joystick and button numbers are the same as in AutoHotkey and \
                     Pulover Macro Creator: 2Joy8 is joystick 2, button 8. The keys go to the \
                     window that has the focus. This works only on Windows.",
                )
                .size(12.0)
                .italics()
                .color(Color32::GRAY),
            );
        });
}

fn render_options_tab(
    ui: &mut egui::Ui,
    config: &mut OverlayConfig,
    buddy_input: &mut String,
    warning_input: &mut String,
) {
    ui.heading("Options");
    ui.add_space(12.0);

    ui.label(
        RichText::new("Configure overlay windows and display settings.")
            .size(14.0)
            .color(Color32::GRAY),
    );

    ui.add_space(16.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        // Overlay Enable/Disable Section
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 30, 35))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Overlay Windows")
                        .size(18.0)
                        .strong()
                        .color(Color32::LIGHT_BLUE),
                );
                ui.add_space(8.0);

                ui.label(
                    RichText::new("Enable or disable overlay windows:")
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );
                ui.add_space(12.0);

                // Lock/Unlock toggle
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Overlay Position:").size(15.0).strong());
                    ui.add_space(8.0);

                    let lock_text = if config.locked { "Locked" } else { "Unlocked" };
                    let lock_color = if config.locked {
                        Color32::from_rgb(255, 150, 150)
                    } else {
                        Color32::from_rgb(150, 255, 150)
                    };

                    if ui
                        .button(RichText::new(lock_text).size(15.0).color(lock_color))
                        .clicked()
                    {
                        let was_unlocked = !config.locked;
                        config.locked = !config.locked;

                        // Save only when the overlays change from unlocked to locked.
                        if config.locked && was_unlocked {
                            let _ = config.save();
                        }
                    }

                    ui.add_space(8.0);
                    if config.locked {
                        ui.label(
                            RichText::new("(Overlays cannot be moved or resized)")
                                .size(12.0)
                                .color(Color32::GRAY)
                                .italics(),
                        );
                    } else {
                        ui.label(
                            RichText::new("(Overlays can be moved and resized)")
                                .size(12.0)
                                .color(Color32::from_rgb(150, 255, 150))
                                .italics(),
                        );
                    }
                });

                ui.add_space(12.0);

                egui::Grid::new("overlay_enable_grid")
                    .num_columns(2)
                    .spacing([40.0, 12.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(RichText::new("Overlay").size(15.0).strong());
                        ui.label(RichText::new("Enabled").size(15.0).strong());
                        ui.end_row();

                        ui.label("Laps Remaining");
                        let mut enabled = config.laps_remaining.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.laps_remaining.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("Fuel to Add");
                        let mut enabled = config.fuel_to_add.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.fuel_to_add.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("AVG Laps Left");
                        let mut enabled = config.avg_laps_left.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.avg_laps_left.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("PUSH Laps Left");
                        let mut enabled = config.push_laps_left.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.push_laps_left.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("LOW Laps Left");
                        let mut enabled = config.low_laps_left.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.low_laps_left.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("MAX Laps Left");
                        let mut enabled = config.max_laps_left.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.max_laps_left.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("Relative Standings");
                        let mut enabled = config.relative.window.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.relative.window.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("Pedal Inputs");
                        let mut enabled = config.pedal.window.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.pedal.window.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("Radar Left");
                        let mut enabled = config.radar_left.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.radar_left.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label("Radar Right");
                        let mut enabled = config.radar_right.enabled;
                        if ui.checkbox(&mut enabled, "").changed() {
                            config.radar_right.enabled = enabled;
                            let _ = config.save();
                        }
                        ui.end_row();
                    });

                ui.add_space(12.0);

                if ui.button("Reset All Positions to Default").clicked() {
                    // Keep the current enabled states.
                    let laps_remaining_enabled = config.laps_remaining.enabled;
                    let fuel_to_add_enabled = config.fuel_to_add.enabled;
                    let avg_laps_left_enabled = config.avg_laps_left.enabled;
                    let push_laps_left_enabled = config.push_laps_left.enabled;
                    let low_laps_left_enabled = config.low_laps_left.enabled;
                    let max_laps_left_enabled = config.max_laps_left.enabled;
                    let relative_enabled = config.relative.window.enabled;
                    let pedal_enabled = config.pedal.window.enabled;
                    let radar_left_enabled = config.radar_left.enabled;
                    let radar_right_enabled = config.radar_right.enabled;

                    *config = OverlayConfig::default();

                    // Restore the enabled states.
                    config.laps_remaining.enabled = laps_remaining_enabled;
                    config.fuel_to_add.enabled = fuel_to_add_enabled;
                    config.avg_laps_left.enabled = avg_laps_left_enabled;
                    config.push_laps_left.enabled = push_laps_left_enabled;
                    config.low_laps_left.enabled = low_laps_left_enabled;
                    config.max_laps_left.enabled = max_laps_left_enabled;
                    config.relative.window.enabled = relative_enabled;
                    config.pedal.window.enabled = pedal_enabled;
                    config.radar_left.enabled = radar_left_enabled;
                    config.radar_right.enabled = radar_right_enabled;

                    let _ = config.save();
                }
            });

        ui.add_space(16.0);

        // Fuel Strategy Settings Section
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 30, 35))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Fuel Strategy")
                        .size(18.0)
                        .strong()
                        .color(Color32::LIGHT_BLUE),
                );
                ui.add_space(8.0);

                ui.label(
                    RichText::new("Configure fuel calculation settings:")
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );
                ui.add_space(12.0);

                egui::Grid::new("fuel_strategy_grid")
                    .num_columns(2)
                    .spacing([20.0, 12.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Fuel Reserve:").size(14.0))
                            .on_hover_text(
                                "Fuel kept in hand on top of what the race needs. Every \
                                 fuel-to-add and pit figure in the app includes this, so \
                                 raising it makes the app ask for more fuel and call you \
                                 in earlier.",
                            );
                        let mut extra_liters = config.extra_liters;
                        if ui
                            .add(
                                egui::Slider::new(&mut extra_liters, 0.0..=5.0)
                                    .suffix(" L")
                                    .step_by(0.1),
                            )
                            .on_hover_text(
                                "A buffer against a bad out-lap, a late caution, or the \
                                 fuel estimate running slightly optimistic. 0.5 L is about \
                                 a fifth of a lap in most cars.",
                            )
                            .changed()
                        {
                            config.extra_liters = extra_liters;
                            let _ = config.save();
                        }
                        ui.end_row();

                        ui.label(RichText::new("Fuel Tank %:").size(14.0));
                        let mut fuel_tank_pct = config.fuel_tank_pct.unwrap_or(100.0);
                        if ui
                            .add(
                                egui::Slider::new(&mut fuel_tank_pct, 50.0..=100.0)
                                    .suffix("%")
                                    .step_by(1.0),
                            )
                            .changed()
                        {
                            config.fuel_tank_pct = if (fuel_tank_pct - 100.0).abs() < 0.1 {
                                None
                            } else {
                                Some(fuel_tank_pct)
                            };
                            let _ = config.save();
                        }
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.label(
                    RichText::new("Add extra fuel to all strategy calculations to avoid running the tank too low, which can cause engine issues in some cars.")
                        .size(12.0)
                        .italics()
                        .color(Color32::GRAY),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Set the fuel tank size as a percentage of the maximum. Leave at 100% (or empty) to use the actual tank capacity from telemetry.")
                        .size(12.0)
                        .italics()
                        .color(Color32::GRAY),
                );
            });

        ui.add_space(16.0);

        // Relative Overlay Settings Section
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 30, 35))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Relative Overlay")
                        .size(18.0)
                        .strong()
                        .color(Color32::LIGHT_BLUE),
                );
                ui.add_space(12.0);

                egui::Grid::new("relative_options_grid")
                    .num_columns(2)
                    .spacing([20.0, 12.0])
                    .show(ui, |ui| {
                        // Show Minimap toggle
                        ui.label(RichText::new("Show Minimap:").size(14.0));
                        let mut show_minimap = config.relative.show_minimap;
                        if ui.checkbox(&mut show_minimap, "").changed() {
                            config.relative.show_minimap = show_minimap;
                            let _ = config.save();
                        }
                        ui.end_row();

                        // Minimap Mode
                        ui.label(RichText::new("Minimap Mode:").size(14.0));
                        ui.vertical(|ui| {
                            let old_mode = config.relative.minimap_mode.clone();

                            ui.radio_value(
                                &mut config.relative.minimap_mode,
                                crate::config::MinimapMode::Relative,
                                "Relative (Player Centered)",
                            );
                            ui.radio_value(
                                &mut config.relative.minimap_mode,
                                crate::config::MinimapMode::Absolute,
                                "Absolute (S/F Centered)",
                            );

                            if old_mode != config.relative.minimap_mode {
                                let _ = config.save();
                            }
                        });
                        ui.end_row();

                        // Number of positions
                        ui.label(RichText::new("Number of cars to show:").size(14.0));
                        let mut num_pos = config.relative.num_positions as f32;
                        if ui
                            .add(egui::Slider::new(&mut num_pos, 3.0..=15.0).step_by(2.0))
                            .changed()
                        {
                            config.relative.num_positions = num_pos as usize;
                            let _ = config.save();
                        }
                        ui.end_row();
                    });

                ui.add_space(16.0);

                // Buddy list section
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("Buddy List")
                        .size(15.0)
                        .strong()
                        .color(Color32::from_rgb(150, 255, 150)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Highlight friends in the relative overlay by adding their names below:",
                    )
                    .size(13.0)
                    .color(Color32::LIGHT_GRAY),
                );
                ui.add_space(8.0);

                // Show current buddy list
                if config.relative.buddy_list.is_empty() {
                    ui.label(
                        RichText::new("No buddies added yet")
                            .size(13.0)
                            .italics()
                            .color(Color32::GRAY),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(150.0)
                        .show(ui, |ui| {
                            let mut to_remove = None;
                            for (idx, buddy) in config.relative.buddy_list.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("• {}", buddy))
                                            .size(13.0)
                                            .color(Color32::from_rgb(150, 255, 150)),
                                    );
                                    if ui.button("Remove").clicked() {
                                        to_remove = Some(idx);
                                    }
                                });
                            }
                            if let Some(idx) = to_remove {
                                config.relative.buddy_list.remove(idx);
                                let _ = config.save();
                            }
                        });
                }

                ui.add_space(8.0);

                // Add buddy text input
                ui.horizontal(|ui| {
                    ui.label("Add driver name:");
                    let response = ui.text_edit_singleline(buddy_input);
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let trimmed = buddy_input.trim().to_string();
                        if !trimmed.is_empty() && !config.relative.buddy_list.contains(&trimmed) {
                            config.relative.buddy_list.push(trimmed);
                            let _ = config.save();
                            buddy_input.clear();
                        }
                    }
                    if ui.button("Add").clicked() && !buddy_input.trim().is_empty() {
                        let trimmed = buddy_input.trim().to_string();
                        if !config.relative.buddy_list.contains(&trimmed) {
                            config.relative.buddy_list.push(trimmed);
                            let _ = config.save();
                            buddy_input.clear();
                        }
                    }
                });

                ui.add_space(4.0);
                ui.label(
                    RichText::new("(You can also edit overlay_config.toml directly)")
                        .size(11.0)
                        .italics()
                        .color(Color32::GRAY),
                );

                ui.add_space(16.0);

                // Warning list section
                ui.separator();
                ui.add_space(8.0);

                ui.label(
                    RichText::new("⚠ Warning List")
                        .size(15.0)
                        .strong()
                        .color(Color32::from_rgb(255, 150, 150)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "Highlight dangerous drivers in the relative overlay by adding their names below:",
                    )
                    .size(13.0)
                    .color(Color32::LIGHT_GRAY),
                );
                ui.add_space(8.0);

                // Show current warning list
                if config.relative.warning_list.is_empty() {
                    ui.label(
                        RichText::new("No warnings added yet")
                            .size(13.0)
                            .italics()
                            .color(Color32::GRAY),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("warning_list_scroll")
                        .max_height(150.0)
                        .show(ui, |ui| {
                            let mut to_remove = None;
                            for (idx, name) in config.relative.warning_list.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("⚠")
                                            .size(13.0)
                                            .color(Color32::from_rgb(255, 200, 0)),
                                    );
                                    ui.label(
                                        RichText::new(name.to_string())
                                            .size(13.0)
                                            .color(Color32::from_rgb(255, 150, 150)),
                                    );
                                    if ui.button("Remove").clicked() {
                                        to_remove = Some(idx);
                                    }
                                });
                            }
                            if let Some(idx) = to_remove {
                                config.relative.warning_list.remove(idx);
                                let _ = config.save();
                            }
                        });
                }

                ui.add_space(8.0);

                // Add warning text input
                ui.horizontal(|ui| {
                    ui.label("Add driver name:");
                    let response = ui.text_edit_singleline(warning_input);
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let trimmed = warning_input.trim().to_string();
                        if !trimmed.is_empty() && !config.relative.warning_list.contains(&trimmed) {
                            config.relative.warning_list.push(trimmed);
                            let _ = config.save();
                            warning_input.clear();
                        }
                    }
                    if ui.button("Add").clicked() && !warning_input.trim().is_empty() {
                        let trimmed = warning_input.trim().to_string();
                        if !config.relative.warning_list.contains(&trimmed) {
                            config.relative.warning_list.push(trimmed);
                            let _ = config.save();
                            warning_input.clear();
                        }
                    }
                });
            });

        ui.add_space(16.0);

        // Pedal Overlay Settings Section
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 30, 35))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Pedal Overlay")
                        .size(18.0)
                        .strong()
                        .color(Color32::LIGHT_BLUE),
                );
                ui.add_space(12.0);

                egui::Grid::new("pedal_options_grid")
                    .num_columns(2)
                    .spacing([20.0, 12.0])
                    .show(ui, |ui| {
                        // History duration
                        ui.label(RichText::new("History Duration:").size(14.0));
                        let mut history_secs = config.pedal.history_seconds;
                        if ui
                            .add(
                                egui::Slider::new(&mut history_secs, 5.0..=60.0)
                                    .suffix(" seconds")
                                    .step_by(5.0),
                            )
                            .changed()
                        {
                            config.pedal.history_seconds = history_secs;
                            let _ = config.save();
                        }
                        ui.end_row();
                    });

                ui.add_space(8.0);
                ui.label(
                    RichText::new("Configure how many seconds of pedal input history to display")
                        .size(12.0)
                        .italics()
                        .color(Color32::GRAY),
                );
            });

        ui.add_space(16.0);

        render_weight_jacker_settings(ui, config);

        ui.add_space(16.0);

        // Overlay Dimensions Section
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 30, 35))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Overlay Dimensions")
                        .size(18.0)
                        .strong()
                        .color(Color32::LIGHT_BLUE),
                );
                ui.add_space(8.0);

                if config.locked {
                    ui.label(
                        RichText::new("⚠ Unlock overlays above to adjust dimensions")
                            .size(13.0)
                            .color(Color32::from_rgb(255, 200, 100)),
                    );
                } else {
                    ui.label(
                        RichText::new("Adjust the width and height of each overlay window:")
                            .size(13.0)
                            .color(Color32::LIGHT_GRAY),
                    );
                }
                ui.add_space(12.0);

                ui.add_enabled_ui(!config.locked, |ui| {
                    // Macro that renders the dimension sliders for an overlay.
                    macro_rules! render_overlay_dimensions {
                        ($name:expr, $config:expr, $default_width:expr, $default_height:expr) => {
                            ui.separator();
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new($name).size(15.0).strong().color(
                                    if config.locked {
                                        Color32::GRAY
                                    } else {
                                        Color32::WHITE
                                    },
                                ));
                                ui.add_space(8.0);
                                if ui.button("Reset Size").clicked() {
                                    $config.width = $default_width;
                                    $config.height = $default_height;
                                    let _ = config.save();
                                }
                            });
                            ui.add_space(8.0);

                            egui::Grid::new(format!("{}_dimensions_grid", $name))
                                .num_columns(2)
                                .spacing([20.0, 8.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Width:").size(14.0));
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut $config.width, 100.0..=800.0)
                                                .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        let _ = config.save();
                                    }
                                    ui.end_row();

                                    ui.label(RichText::new("Height:").size(14.0));
                                    if ui
                                        .add(
                                            egui::Slider::new(&mut $config.height, 100.0..=800.0)
                                                .suffix(" px"),
                                        )
                                        .changed()
                                    {
                                        let _ = config.save();
                                    }
                                    ui.end_row();
                                });

                            ui.add_space(8.0);
                        };
                    }

                    // The Laps Remaining overlay has a fixed size of 235x110 px.
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Note: Laps Remaining overlay has a fixed size of 235x110px")
                            .size(13.0)
                            .italics()
                            .color(Color32::GRAY),
                    );
                    ui.add_space(8.0);
                    // The Fuel to Add and Laps Left overlays have fixed sizes.
                    ui.separator();
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!(
                            "Note: Fuel to Add overlay has a fixed size of {:.0}x{:.0}px",
                            overlay::FUEL_TO_ADD_OVERLAY_SIZE[0],
                            overlay::FUEL_TO_ADD_OVERLAY_SIZE[1]
                        ))
                            .size(13.0)
                            .italics()
                            .color(Color32::GRAY),
                    );
                    ui.label(
                        RichText::new("Note: AVG/PUSH/LOW/MAX Laps Left overlays have a fixed size of 140x120px")
                            .size(13.0)
                            .italics()
                            .color(Color32::GRAY),
                    );
                    ui.add_space(8.0);

                    render_overlay_dimensions!("Relative", config.relative.window, 400.0, 600.0);
                    render_overlay_dimensions!("Pedal Input", config.pedal.window, 350.0, 350.0);
                });

                ui.add_space(8.0);
                if !config.locked {
                    ui.label(
                        RichText::new(
                            "Tip: You can also drag overlay corners to resize them directly",
                        )
                        .size(11.0)
                        .italics()
                        .color(Color32::GRAY),
                    );
                }
            });

        ui.add_space(16.0);

        // Config Location Section
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 30, 35))
            .rounding(8.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Configuration Location")
                        .size(18.0)
                        .strong()
                        .color(Color32::LIGHT_BLUE),
                );
                ui.add_space(12.0);

                let config_path = OverlayConfig::config_path();
                let config_dir = config_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."));

                ui.label(
                    RichText::new(format!("Config file: {}", config_path.display()))
                        .size(13.0)
                        .color(Color32::LIGHT_GRAY),
                );

                ui.add_space(8.0);

                if ui.button("Open Config Folder").clicked()
                    && let Err(e) = open_file_location(config_dir) {
                        eprintln!("Failed to open config location: {}", e);
                    }
            });
    });
}

async fn run_network_listener(state: Arc<Mutex<AppState>>) -> anyhow::Result<()> {
    use checking_fuel::ParseError;
    use deku::DekuContainerRead;
    use futures::StreamExt;
    use std::time::Instant;
    use tokio::net::TcpListener;
    use tokio_util::codec::{FramedRead, LengthDelimitedCodec};

    let listener = TcpListener::bind("0.0.0.0:5555").await?;

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                println!("Client connected: {}", addr);
                let state_clone = state.clone();

                tokio::spawn(async move {
                    let (read, _) = socket.into_split();
                    let mut framed = FramedRead::new(read, LengthDelimitedCodec::new());
                    let mut last_report = Instant::now();
                    let mut frame_count = 0u64;

                    while let Some(frame_result) = framed.next().await {
                        match frame_result {
                            Ok(bytes) => {
                                match TelemetryFrame::from_bytes((&bytes, 0)) {
                                    Ok((_, telem)) => {
                                        let latency = checking_protocol::current_time_ms()
                                            .saturating_sub(telem.timestamp_ms);

                                        frame_count += 1;

                                        let mut app_state = state_clone.lock().unwrap();
                                        app_state.update(telem, latency);

                                        // Update the Hz value each second.
                                        if last_report.elapsed().as_secs() >= 1 {
                                            app_state.update_hz(frame_count);
                                            frame_count = 0;
                                            last_report = Instant::now();
                                        }
                                    }
                                    Err(e) => {
                                        let mut app_state = state_clone.lock().unwrap();
                                        let error = ParseError::new(
                                            "TelemetryFrame",
                                            "Deku",
                                            format!("Failed to parse telemetry frame: {:?}", e),
                                        )
                                        .with_data_preview(&bytes, 32);
                                        app_state.add_parse_error(error);
                                    }
                                }
                            }
                            Err(e) => {
                                let mut app_state = state_clone.lock().unwrap();
                                let error = ParseError::new(
                                    "Network",
                                    "IO",
                                    format!("Failed to read frame from network: {}", e),
                                );
                                app_state.add_parse_error(error);
                            }
                        }
                    }
                });
            }
            Err(e) => {
                eprintln!("Connection error: {}", e);
            }
        }
    }
}
