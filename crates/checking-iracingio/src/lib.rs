#[cfg(windows)]
mod irsdk;

#[cfg(windows)]
use anyhow::Result;
#[cfg(windows)]
use bytes::Bytes;
#[cfg(windows)]
use deku::DekuContainerWrite;
#[cfg(windows)]
use checking_fuel::{SenderState, SenderStatus};
#[cfg(windows)]
use futures::SinkExt;
#[cfg(windows)]
use irsdk::IrSdkClient;
#[cfg(windows)]
use serde::Deserialize;
#[cfg(windows)]
use std::collections::HashMap;
#[cfg(windows)]
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use checking_protocol::{current_time_ms, DriverInfo, TelemetryFrame, PROTOCOL_VERSION};
#[cfg(windows)]
use tokio::net::TcpStream;
#[cfg(windows)]
use tokio::time::{interval, sleep, Duration};
#[cfg(windows)]
use tokio_util::codec::{FramedWrite, LengthDelimitedCodec};

#[cfg(windows)]
#[derive(Debug, Default)]
struct PitTracker {
    // car_idx -> (lap of the last pit stop, was_in_pit)
    car_states: HashMap<i32, (i32, bool)>,
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
struct SessionInfo {
    #[serde(rename = "DriverInfo")]
    driver_info: DriverInfoSection,
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
struct DriverInfoSection {
    #[serde(rename = "DriverCarFuelMaxLtr", default)]
    driver_car_fuel_max_ltr: f32,
    #[serde(rename = "Drivers")]
    drivers: Vec<SessionDriver>,
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
struct SessionDriver {
    #[serde(rename = "CarIdx")]
    car_idx: i32,
    #[serde(rename = "UserName")]
    user_name: String,
    #[serde(rename = "CarNumber", default)]
    car_number: String,
    #[serde(rename = "CarClassID", default)]
    car_class_id: i32,
    #[serde(rename = "IRating", default)]
    irating: i32,
    #[serde(rename = "LicString", default)]
    lic_string: String,
    #[serde(rename = "CarClassEstLapTime", default)]
    car_class_est_lap_time: f32,
}

/// Connects to iRacing and sends the telemetry to the receiver. After an error,
/// the loop tries again. It does not return.
#[cfg(windows)]
pub async fn run_sender_loop(sender_state: Arc<Mutex<SenderState>>) {
    loop {
        {
            let mut state = sender_state.lock().unwrap();
            state.status = SenderStatus::ConnectingToIRacing;
        }

        let irsdk = loop {
            match IrSdkClient::new() {
                Ok(client) => {
                    break client;
                }
                Err(e) => {
                    let mut state = sender_state.lock().unwrap();
                    state.status = SenderStatus::Error(format!("iRacing not running: {}", e));
                    state.last_error = Some(format!("iRacing connection failed: {}", e));
                    drop(state);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        };

        {
            let mut state = sender_state.lock().unwrap();
            state.status = SenderStatus::WaitingForSession;
        }

        while !irsdk.is_connected() {
            sleep(Duration::from_millis(500)).await;
        }

        let target = {
            let state = sender_state.lock().unwrap();
            state.target_address.clone()
        };

        {
            let mut state = sender_state.lock().unwrap();
            state.status = SenderStatus::ConnectingToReceiver;
        }

        let stream = match TcpStream::connect(&target).await {
            Ok(s) => s,
            Err(e) => {
                let mut state = sender_state.lock().unwrap();
                state.status = SenderStatus::Error(format!("Failed to connect to receiver: {}", e));
                state.last_error = Some(format!("Receiver connection failed: {}", e));
                drop(state);
                sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mut framed = FramedWrite::new(stream, LengthDelimitedCodec::new());

        // A 16 ms period gives 62.5 Hz, which is near the 60 Hz iRacing rate.
        let mut ticker = interval(Duration::from_millis(16));
        let mut frame_count = 0u64;
        let mut total_frames_sent = 0u64;
        let mut last_report = tokio::time::Instant::now();
        let mut pit_tracker = PitTracker::default();

        // Keep the session info values, so that the YAML is not parsed for each frame.
        let mut cached_fuel_max_ltr: f32 = 0.0;
        let mut cached_track_length_meters: f32 = 0.0;
        let mut last_session_info_update: i32 = -1;

        loop {
            ticker.tick().await;

            if !irsdk.is_connected() {
                let mut state = sender_state.lock().unwrap();
                state.status = SenderStatus::Error("iRacing disconnected".to_string());
                state.last_error = Some("iRacing session ended".to_string());
                break;
            }

            let data = match irsdk.get_latest_data() {
                Ok(d) => d,
                Err(e) => {
                    let mut state = sender_state.lock().unwrap();
                    state.last_error = Some(format!("Error reading data: {}", e));
                    continue;
                }
            };

            // iRacing increments this counter when the session info changes.
            let session_info_update = irsdk.get_session_info_update();
            if session_info_update != last_session_info_update {
                last_session_info_update = session_info_update;
                if let Ok(yaml) = irsdk.get_session_info() {
                    if let Ok(info) = serde_yaml::from_str::<SessionInfo>(&yaml) {
                        cached_fuel_max_ltr = info.driver_info.driver_car_fuel_max_ltr;
                    }
                    // Returns 0.0 if the YAML has no track length.
                    let track_length = parse_track_length_from_yaml(&yaml);
                    if track_length > 0.0 {
                        cached_track_length_meters = track_length;
                    }
                }
            }

            let frame = match extract_telemetry(
                &irsdk,
                &data,
                &mut pit_tracker,
                cached_fuel_max_ltr,
                cached_track_length_meters,
            ) {
                Ok(f) => f,
                Err(e) => {
                    let mut state = sender_state.lock().unwrap();
                    state.last_error = Some(format!("Error extracting telemetry: {}", e));
                    continue;
                }
            };

            let bytes = match frame.to_bytes() {
                Ok(b) => b,
                Err(e) => {
                    let mut state = sender_state.lock().unwrap();
                    state.last_error = Some(format!("Error serializing frame: {}", e));
                    continue;
                }
            };

            if let Err(e) = framed.send(Bytes::from(bytes)).await {
                let mut state = sender_state.lock().unwrap();
                state.status = SenderStatus::Error(format!("Send error: {}", e));
                state.last_error = Some(format!("Failed to send data: {}", e));
                break;
            }

            frame_count += 1;
            total_frames_sent += 1;

            if last_report.elapsed().as_secs() >= 1 {
                let hz = frame_count;
                let mut state = sender_state.lock().unwrap();
                state.status = SenderStatus::Streaming {
                    hz,
                    frames_sent: total_frames_sent,
                };
                frame_count = 0;
                last_report = tokio::time::Instant::now();
            }
        }

        sleep(Duration::from_secs(3)).await;
    }
}

/// Returns the track length in meters from the session YAML, or 0.0 if it is
/// not there. The value can be in km or miles, as in iFL03.
#[cfg(windows)]
fn parse_track_length_from_yaml(yaml: &str) -> f32 {
    // The line is, for example, `TrackLength: 3.50 km` under `WeekendInfo:`.
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("TrackLength:") {
            if let Some(value_str) = trimmed.strip_prefix("TrackLength:") {
                let value_str = value_str.trim().trim_matches('\'').trim_matches('"');

                let parts: Vec<&str> = value_str.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                if let Ok(value) = parts[0].parse::<f32>() {
                    let unit = parts.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

                    if unit.contains("km") {
                        return value * 1000.0;
                    } else if unit.contains("mi") {
                        return value * 1609.344;
                    } else {
                        // No unit means meters.
                        return value;
                    }
                }
            }
        }
    }

    0.0
}

/// Gets the driver data from the session info and the telemetry.
/// Returns the drivers and an error message, if there is an error.
#[cfg(windows)]
fn extract_drivers(
    irsdk: &IrSdkClient,
    data: &[u8],
    player_car_idx: i32,
    pit_tracker: &mut PitTracker,
) -> (Vec<DriverInfo>, Option<String>) {
    let session_yaml = match irsdk.get_session_info() {
        Ok(yaml) => yaml,
        Err(e) => {
            let error_msg = format!("Cannot get the session info YAML: {}", e);
            return (vec![], Some(error_msg));
        }
    };

    let session_info: SessionInfo = match serde_yaml::from_str(&session_yaml) {
        Ok(info) => info,
        Err(e) => {
            let error_msg = format!("Cannot parse the session YAML for the driver data: {}", e);
            return (vec![], Some(error_msg));
        }
    };

    let positions = irsdk.get_var_i32_array("CarIdxPosition", data).ok();
    let class_positions = irsdk.get_var_i32_array("CarIdxClassPosition", data).ok();
    let laps = irsdk.get_var_i32_array("CarIdxLap", data).ok();
    let lap_dist_pcts = irsdk.get_var_f32_array("CarIdxLapDistPct", data).ok();
    let on_pit_road = irsdk.get_var_i32_array("CarIdxOnPitRoad", data).ok();
    // CarIdxTrackSurface contains TrkLoc (the location), and not TrkSurf (the surface material).
    // TrkLoc: -1=not_in_world, 0=off_track, 1=in_pit_stall, 2=approaching_pits, 3=on_track
    let track_surface = irsdk.get_var_i32_array("CarIdxTrackSurface", data).ok();
    let best_lap_times = irsdk.get_var_f32_array("CarIdxBestLapTime", data).ok();
    let last_lap_times = irsdk.get_var_f32_array("CarIdxLastLapTime", data).ok();
    let est_times = irsdk.get_var_f32_array("CarIdxEstTime", data).ok();

    let mut drivers = Vec::new();

    for session_driver in session_info.driver_info.drivers {
        let idx = session_driver.car_idx as usize;

        let position = positions
            .as_ref()
            .and_then(|p| p.get(idx).copied())
            .unwrap_or(0);
        let class_position = class_positions
            .as_ref()
            .and_then(|p| p.get(idx).copied())
            .unwrap_or(0);
        let lap = laps.as_ref().and_then(|l| l.get(idx).copied()).unwrap_or(0);
        let lap_dist_pct = lap_dist_pcts
            .as_ref()
            .and_then(|l| l.get(idx).copied())
            .unwrap_or(-1.0);
        let pit_road = on_pit_road
            .as_ref()
            .and_then(|p| p.get(idx).copied())
            .unwrap_or(0);
        let surface = track_surface
            .as_ref()
            .and_then(|s| s.get(idx).copied())
            .unwrap_or(3);
        let best_time = best_lap_times
            .as_ref()
            .and_then(|b| b.get(idx).copied())
            .unwrap_or(0.0);
        let last_time = last_lap_times
            .as_ref()
            .and_then(|l| l.get(idx).copied())
            .unwrap_or(0.0);
        let est_time = est_times
            .as_ref()
            .and_then(|e| e.get(idx).copied())
            .unwrap_or(0.0);

        // A negative value means that the car is not in the world.
        if lap_dist_pct < 0.0 {
            continue;
        }

        let mut driver_info = DriverInfo::new(
            session_driver.car_idx,
            &session_driver.user_name,
            &session_driver.car_number,
            &session_driver.lic_string,
            session_driver.irating,
            position,
            lap,
            lap_dist_pct,
        );

        driver_info.car_class_id = session_driver.car_class_id;
        driver_info.class_position = class_position;
        driver_info.lap_best_time = best_time;
        driver_info.lap_last_time = last_time;
        driver_info.est_time = est_time;
        driver_info.class_est_lap_time = session_driver.car_class_est_lap_time;

        // The CarIdxOnPitRoad flag is not reliable, so also use TrkLoc.
        // TrkLoc: 1=in_pit_stall, 2=approaching_pits
        let is_in_pit = surface == 1 || surface == 2;
        driver_info.on_pit_road = if is_in_pit { 1 } else { pit_road };
        driver_info.track_surface = surface;

        let car_idx = session_driver.car_idx;
        let (last_lap, was_in_pit) = pit_tracker
            .car_states
            .entry(car_idx)
            .or_insert((lap, false));

        let laps_since_pit = if is_in_pit {
            0
        } else if *was_in_pit && !is_in_pit {
            // The first lap after the pit stop is lap 0.
            0
        } else {
            lap - *last_lap
        };

        if is_in_pit {
            *last_lap = lap;
        }
        *was_in_pit = is_in_pit;

        driver_info.laps_since_pit = laps_since_pit.max(0);
        driver_info.is_pace_car = 0; // TODO: Detect the pace car.
        driver_info.is_pacing = 0; // TODO: Detect pacing.

        drivers.push(driver_info);
    }

    (drivers, None)
}

#[cfg(windows)]
fn extract_telemetry(
    irsdk: &IrSdkClient,
    data: &[u8],
    pit_tracker: &mut PitTracker,
    cached_fuel_max_ltr: f32,
    cached_track_length_meters: f32,
) -> Result<TelemetryFrame> {
    // Gets a variable, or the default if the variable is not available.
    macro_rules! get_var {
        ($method:ident, $name:expr, $default:expr) => {
            irsdk.$method($name, data).unwrap_or($default)
        };
    }

    let mut frame = TelemetryFrame {
        version: PROTOCOL_VERSION,
        timestamp_ms: current_time_ms(),

        // Driving inputs
        speed: get_var!(get_var_f32, "Speed", 0.0),
        rpm: get_var!(get_var_f32, "RPM", 0.0),
        gear: get_var!(get_var_i32, "Gear", 0),
        throttle: get_var!(get_var_f32, "Throttle", 0.0),
        brake: get_var!(get_var_f32, "Brake", 0.0),
        clutch: get_var!(get_var_f32, "Clutch", 0.0),
        steering_wheel_angle: get_var!(get_var_f32, "SteeringWheelAngle", 0.0),

        // Timing
        session_time: get_var!(get_var_f64, "SessionTime", 0.0),
        lap_current: get_var!(get_var_i32, "Lap", 0),
        lap_best_time: {
            let best_ms = get_var!(get_var_i32, "LapBestLapTime", 0);
            best_ms as f32 / 1000.0
        },
        lap_last_time: get_var!(get_var_f32, "LapLastLapTime", 0.0),
        lap_dist_pct: get_var!(get_var_f32, "LapDistPct", 0.0),
        leader_lap: {
            // Use the lap of the car in position 1. If there is none, use the player lap.
            let leader_lap = match irsdk.get_var_i32_array("CarIdxPosition", data) {
                Ok(positions) => {
                    positions
                        .iter()
                        .position(|&pos| pos == 1)
                        .and_then(|leader_idx| {
                            irsdk
                                .get_var_i32_array("CarIdxLap", data)
                                .ok()
                                .and_then(|laps| laps.get(leader_idx).copied())
                        })
                }
                _ => None,
            };

            leader_lap.unwrap_or_else(|| get_var!(get_var_i32, "Lap", 0))
        },

        // Car state
        fuel_level: get_var!(get_var_f32, "FuelLevel", 0.0),
        fuel_level_pct: get_var!(get_var_f32, "FuelLevelPct", 0.0),
        fuel_max_ltr: cached_fuel_max_ltr,
        fuel_use_per_hour: get_var!(get_var_f32, "FuelUsePerHour", 0.0),
        pit_sv_fuel: get_var!(get_var_f32, "PitSvFuel", 0.0),
        fuel_capacity: {
            let driver_max = cached_fuel_max_ltr;
            let fuel_level_pct = get_var!(get_var_f32, "FuelLevelPct", 0.0);
            let fuel_level = get_var!(get_var_f32, "FuelLevel", 0.0);

            if fuel_level_pct > 0.001 && fuel_level > 0.0 {
                let calculated = fuel_level / fuel_level_pct;
                if driver_max < 1.0 || (calculated > 10.0 && (driver_max - calculated).abs() > 10.0)
                {
                    calculated
                } else {
                    driver_max
                }
            } else {
                driver_max.max(0.0)
            }
        },
        oil_temp: get_var!(get_var_f32, "OilTemp", 0.0),
        water_temp: get_var!(get_var_f32, "WaterTemp", 0.0),
        oil_pressure: get_var!(get_var_f32, "OilPress", 0.0),
        voltage: get_var!(get_var_f32, "Voltage", 0.0),

        // Tire temperatures
        tire_temps_surface: {
            let lf = get_var!(get_var_f32, "LFtempCL", 0.0);
            let rf = get_var!(get_var_f32, "RFtempCL", 0.0);
            let lr = get_var!(get_var_f32, "LRtempCL", 0.0);
            let rr = get_var!(get_var_f32, "RRtempCL", 0.0);
            vec![lf, rf, lr, rr]
        },
        tire_temps_inner: {
            let lf = get_var!(get_var_f32, "LFtempCL", 0.0);
            let rf = get_var!(get_var_f32, "RFtempCL", 0.0);
            let lr = get_var!(get_var_f32, "LRtempCL", 0.0);
            let rr = get_var!(get_var_f32, "RRtempCL", 0.0);
            vec![lf, rf, lr, rr]
        },
        tire_temps_middle: {
            let lf = get_var!(get_var_f32, "LFtempCM", 0.0);
            let rf = get_var!(get_var_f32, "RFtempCM", 0.0);
            let lr = get_var!(get_var_f32, "LRtempCM", 0.0);
            let rr = get_var!(get_var_f32, "RRtempCM", 0.0);
            vec![lf, rf, lr, rr]
        },
        tire_temps_outer: {
            let lf = get_var!(get_var_f32, "LFtempCR", 0.0);
            let rf = get_var!(get_var_f32, "RFtempCR", 0.0);
            let lr = get_var!(get_var_f32, "LRtempCR", 0.0);
            let rr = get_var!(get_var_f32, "RRtempCR", 0.0);
            vec![lf, rf, lr, rr]
        },
        tire_pressure: {
            let lf = get_var!(get_var_f32, "LFpressure", 0.0);
            let rf = get_var!(get_var_f32, "RFpressure", 0.0);
            let lr = get_var!(get_var_f32, "LRpressure", 0.0);
            let rr = get_var!(get_var_f32, "RRpressure", 0.0);
            vec![lf, rf, lr, rr]
        },
        tire_wear: {
            let lf = get_var!(get_var_f32, "LFwear", 0.0);
            let rf = get_var!(get_var_f32, "RFwear", 0.0);
            let lr = get_var!(get_var_f32, "LRwear", 0.0);
            let rr = get_var!(get_var_f32, "RRwear", 0.0);
            vec![lf, rf, lr, rr]
        },

        // Position & session
        track_temp: get_var!(get_var_f32, "TrackTemp", 0.0),
        air_temp: get_var!(get_var_f32, "AirTemp", 0.0),
        track_length_meters: cached_track_length_meters,
        position: get_var!(get_var_i32, "PlayerCarPosition", 0),
        class_position: get_var!(get_var_i32, "PlayerCarClassPosition", 0),
        session_state: get_var!(get_var_i32, "SessionState", 0),
        session_flags: get_var!(get_var_u32, "SessionFlags", 0),
        session_laps_total: get_var!(get_var_i32, "SessionLapsTotal", 0),
        session_laps_remain_ex: get_var!(get_var_i32, "SessionLapsRemainEx", 0),
        session_time_remain: get_var!(get_var_f64, "SessionTimeRemain", 0.0),
        session_num: get_var!(get_var_i32, "SessionNum", 0),
        on_pit_road: get_var!(get_var_i32, "OnPitRoad", 0),

        // Damage
        lr_shockdefl: get_var!(get_var_f32, "LRshockDefl", 0.0),
        rr_shockdefl: get_var!(get_var_f32, "RRshockDefl", 0.0),

        // Player car index
        player_car_idx: get_var!(get_var_i32, "PlayerCarIdx", 0),

        // Car left/right detection (irsdk_CarLeftRight bitfield)
        car_left_right: get_var!(get_var_i32, "CarLeftRight", 0),

        // extract_drivers sets these values below.
        driver_count: 0,
        drivers: vec![],
    };

    let (drivers, error) = extract_drivers(irsdk, data, frame.player_car_idx, pit_tracker);
    frame.drivers = drivers;
    frame.driver_count = frame.drivers.len();

    if let Some(err_msg) = error {
        if frame.drivers.is_empty() {
            // Fail only if there are no drivers. The sender loop records the error.
            return Err(anyhow::anyhow!("Cannot get the driver data: {}", err_msg));
        }
    }

    Ok(frame)
}

/// Stub for platforms other than Windows. It does nothing.
#[cfg(not(windows))]
pub async fn run_sender_loop(
    _sender_state: std::sync::Arc<std::sync::Mutex<checking_fuel::SenderState>>,
) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
    }
}
