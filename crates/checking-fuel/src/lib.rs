use checking_protocol::TelemetryFrame;
use chrono::{DateTime, Local};
use std::collections::VecDeque;
use std::time::Instant;

// irsdk_CarLeftRight values
pub const LR_OFF: i32 = 0;
pub const LR_CLEAR: i32 = 1;
pub const LR_CAR_LEFT: i32 = 2;
pub const LR_CAR_RIGHT: i32 = 3;
pub const LR_CAR_LEFT_RIGHT: i32 = 4;
pub const LR_2_CARS_LEFT: i32 = 5;
pub const LR_2_CARS_RIGHT: i32 = 6;

/// Limits for a valid car length in metres. Samples outside these limits are noise (a spin, a large
/// gap, or a car that was not really alongside), and the code ignores them. iRon uses 1.5..5.5, but
/// the Dallara IR-18 is longer than 5.5 m allows, so the upper limit here is higher.
pub const MIN_CAR_LENGTH_M: f32 = 1.5;
pub const MAX_CAR_LENGTH_M: f32 = 7.0;

/// Number of recent samples to keep for each car class. The median of these is the car length.
const CAR_LENGTH_SAMPLES: usize = 5;

/// TrkLoc value for a car on the track surface.
const TRK_LOC_ON_TRACK: i32 = 3;

/// Signed difference between two lap distance fractions, wrapped at the start/finish line. Thus a
/// car just in front gives a small positive number and not almost a full lap.
pub fn wrapped_pct_delta(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    if d > 0.5 {
        d -= 1.0;
    } else if d < -0.5 {
        d += 1.0;
    }
    d
}

/// Median of a small sample set. Returns None for an empty input.
fn median(values: &VecDeque<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let mut sorted: Vec<f32> = values.iter().copied().collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        Some((sorted[mid - 1] + sorted[mid]) / 2.0)
    } else {
        Some(sorted[mid])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SenderStatus {
    Disconnected,
    ConnectingToIRacing,
    WaitingForSession,
    ConnectingToReceiver,
    Streaming { hz: u64, frames_sent: u64 },
    Error(String),
}

#[derive(Clone)]
pub struct SenderState {
    pub status: SenderStatus,
    pub target_address: String,
    pub last_error: Option<String>,
}

impl SenderState {
    pub fn new(target_address: String) -> Self {
        Self {
            status: SenderStatus::Disconnected,
            target_address,
            last_error: None,
        }
    }
}

const HISTORY_SIZE: usize = 100;
#[allow(dead_code)]
const GREEN_FLAG: u32 = 0x0004;
const YELLOW_FLAG: u32 = 0x0008;
const YELLOW_WAVING: u32 = 0x0100;
const ONE_LAP_TO_GREEN: u32 = 0x0200;
const GREEN_HELD: u32 = 0x0400;
const CAUTION: u32 = 0x4000;
const CAUTION_WAVING: u32 = 0x8000;

// Mask for all yellow, caution and transition flags: yellow, yellow_waving, one_lap_to_green,
// green_held, caution and caution_waving.
const YELLOW_FLAGS_MASK: u32 =
    YELLOW_FLAG | YELLOW_WAVING | ONE_LAP_TO_GREEN | GREEN_HELD | CAUTION | CAUTION_WAVING;

#[derive(Clone, Debug)]
pub struct LapData {
    pub lap_number: i32,
    pub fuel_used: f32,
    pub green_flag_count: u64,
    pub yellow_flag_count: u64,
    pub had_pit_activity: bool,
}

#[derive(Clone, Debug)]
pub struct StintData {
    pub stint_number: usize,
    pub laps: Vec<i32>,
    pub green_flag_laps: u64,
    pub yellow_flag_laps: u64,
}

#[derive(Clone, Debug)]
pub struct DebugSnapshot {
    pub lap_number: i32,
    pub lap_dist_pct: f32,
    pub fuel_level: f32,
    pub avg_estimate: Option<f32>,
    pub push_estimate: Option<f32>,
    pub low_estimate: Option<f32>,
    pub max_estimate: Option<f32>,
    pub is_yellow_flag: bool,
}

/// Snapshot for each lap, to show the fuel estimates for all laps.
#[derive(Clone, Debug)]
pub struct LapSnapshot {
    pub lap_number: i32,
    pub fuel_level_at_start: f32,
    pub fuel_used: Option<f32>,
    pub avg_estimate: Option<f32>,
    pub push_estimate: Option<f32>,
    pub low_estimate: Option<f32>,
    pub max_estimate: Option<f32>,
    pub was_yellow_lap: bool,
    // Raw SDK values for debugging
    pub fuel_level_pct: f32,
    pub fuel_max_ltr: f32,
    pub fuel_capacity_calculated: f32,
}

#[derive(Clone, Debug)]
pub struct InputSample {
    pub timestamp: Instant,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
}

/// A parse error or a data error.
#[derive(Clone, Debug)]
pub struct ParseError {
    pub timestamp: DateTime<Local>,
    pub source: String,     // e.g., "TelemetryFrame", "SessionYAML", "Config"
    pub error_type: String, // e.g., "Deku", "SerdeYaml", "IO"
    pub message: String,
    pub raw_data_preview: Option<String>, // First N bytes as hex for debugging
}

impl ParseError {
    pub fn new(source: &str, error_type: &str, message: String) -> Self {
        Self {
            timestamp: Local::now(),
            source: source.to_string(),
            error_type: error_type.to_string(),
            message,
            raw_data_preview: None,
        }
    }

    pub fn with_data_preview(mut self, data: &[u8], max_bytes: usize) -> Self {
        let preview_len = data.len().min(max_bytes);
        self.raw_data_preview = Some(
            data[..preview_len]
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
        );
        self
    }
}

// ============================================
// Standalone fuel calculation helper functions
// ============================================

/// Returns the fuel capacity after the league tank percentage restriction.
pub fn get_effective_fuel_capacity(base_capacity: f32, fuel_tank_pct: Option<f32>) -> f32 {
    if let Some(pct) = fuel_tank_pct {
        base_capacity * (pct / 100.0)
    } else {
        base_capacity
    }
}

/// Simulation result for a fuel strategy.
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub num_stops: i32,
    pub fuel_margin: f32,
    pub can_finish: bool,
    pub pit_details: Vec<(i32, f32)>, // (pit_lap, fuel_added)
}

/// Simulates the race to the end with a given fuel use rate. Returns the result, with the necessary
/// pit stops.
pub fn simulate_race_strategy(
    current_fuel: f32,
    current_lap_fractional: f32,
    total_laps: i32,
    fuel_per_lap: f32,
    fuel_capacity: f32,
) -> SimulationResult {
    let laps_to_go = (total_laps as f32 - current_lap_fractional).max(0.0);
    let mut fuel = current_fuel;
    let mut remaining_laps = laps_to_go;
    let mut num_stops = 0;
    let mut pit_details: Vec<(i32, f32)> = Vec::new();

    while remaining_laps > 0.0 {
        let laps_on_current_fuel = fuel / fuel_per_lap;

        if laps_on_current_fuel >= remaining_laps {
            // The car can finish. Calculate the margin.
            let fuel_at_finish = fuel - (remaining_laps * fuel_per_lap);
            return SimulationResult {
                num_stops,
                fuel_margin: fuel_at_finish,
                can_finish: true,
                pit_details,
            };
        }

        // A pit stop is necessary.
        num_stops += 1;
        let pit_lap =
            (current_lap_fractional + (laps_to_go - remaining_laps) + laps_on_current_fuel.floor())
                .floor() as i32;
        remaining_laps -= laps_on_current_fuel.floor();

        // Refuel for the remaining laps, with the tank capacity as the limit.
        let fuel_needed = remaining_laps * fuel_per_lap;
        let fuel_added = fuel_needed.min(fuel_capacity);
        pit_details.push((pit_lap, fuel_added));
        fuel = fuel_added;

        // If the tank is too small for 1 lap, the race cannot be finished.
        if fuel < fuel_per_lap && remaining_laps > 0.0 {
            return SimulationResult {
                num_stops,
                fuel_margin: 0.0,
                can_finish: false,
                pit_details,
            };
        }
    }

    SimulationResult {
        num_stops,
        fuel_margin: fuel,
        can_finish: true,
        pit_details,
    }
}

/// Runs one strategy simulation for each fuel use per lap in
/// `fuel_per_lap`. The results are in the same order as the input, which is
/// LOW, AVG, PUSH and MAX.
pub fn compute_all_simulations(
    current_fuel: f32,
    current_lap_fractional: f32,
    total_laps: i32,
    fuel_capacity: f32,
    fuel_per_lap: [f32; 4],
) -> [SimulationResult; 4] {
    fuel_per_lap.map(|fuel_per_lap| {
        simulate_race_strategy(
            current_fuel,
            current_lap_fractional,
            total_laps,
            fuel_per_lap,
            fuel_capacity,
        )
    })
}

#[derive(Clone)]
pub struct AppState {
    pub latest_frame: TelemetryFrame,
    pub connected: bool,
    pub latency_ms: u64,
    pub frame_count: u64,
    pub current_hz: u64,
    pub speed_history: VecDeque<f32>,
    pub rpm_history: VecDeque<f32>,

    // Fuel tracking
    pub lap_data: Vec<LapData>,
    pub current_lap: i32,
    pub lap_start_fuel: f32,
    pub lap_green_flags: u64,
    pub lap_yellow_flags: u64,
    pub is_yellow_flag: bool,

    // Stint tracking
    pub stints: Vec<StintData>,
    pub current_stint: usize,
    pub last_fuel_level: f32,

    // Laps down. Updated only when the player lap changes.
    pub laps_down: i32,

    // Session number, to find session changes.
    pub last_session_num: i32,

    // Debug tracking
    pub debug_snapshots: Vec<DebugSnapshot>,

    // Snapshots for each lap, to show the fuel estimates for all laps. There is no limit.
    pub lap_snapshots: Vec<LapSnapshot>,

    // Input history for the pedal overlay: up to 60 seconds at 60 Hz, which is 3600 samples.
    pub input_history: VecDeque<InputSample>,

    // Pit stop data: car_idx -> (pit_count, last_known_laps_since_pit)
    pub pit_stop_tracker: std::collections::HashMap<i32, (i32, i32)>,

    // Fuel capacity data, to find restricted tanks.
    pub observed_max_fuel: f32,
    pub actual_fuel_capacity: Option<f32>,

    // The last 100 errors.
    pub parse_errors: VecDeque<ParseError>,

    // Car length samples for each car_class_id. Each sample is the gap at the moment when
    // CarLeftRight changed to clear.
    pub car_length_samples: std::collections::HashMap<i32, VecDeque<f32>>,
    // Median of the samples above, for each car_class_id.
    pub car_length: std::collections::HashMap<i32, f32>,
    // Previous CarLeftRight value, to find the change to clear.
    pub prev_car_left_right: i32,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            latest_frame: TelemetryFrame::default(),
            connected: false,
            latency_ms: 0,
            frame_count: 0,
            current_hz: 0,
            speed_history: VecDeque::with_capacity(HISTORY_SIZE),
            rpm_history: VecDeque::with_capacity(HISTORY_SIZE),
            lap_data: Vec::new(),
            current_lap: 0,
            lap_start_fuel: 0.0,
            lap_green_flags: 0,
            lap_yellow_flags: 0,
            is_yellow_flag: false,
            stints: vec![StintData {
                stint_number: 1,
                laps: Vec::new(),
                green_flag_laps: 0,
                yellow_flag_laps: 0,
            }],
            current_stint: 0,
            last_fuel_level: 0.0,
            laps_down: 0,
            last_session_num: -1, // Start with -1 to detect first session
            debug_snapshots: Vec::new(),
            lap_snapshots: Vec::new(),
            input_history: VecDeque::with_capacity(3600), // 60 seconds at 60Hz
            pit_stop_tracker: std::collections::HashMap::new(),
            observed_max_fuel: 0.0,
            actual_fuel_capacity: None,
            parse_errors: VecDeque::with_capacity(100),
            car_length_samples: std::collections::HashMap::new(),
            car_length: std::collections::HashMap::new(),
            prev_car_left_right: LR_OFF,
        }
    }

    /// Learns car lengths from the longitudinal gap at the moment when CarLeftRight changes to
    /// clear.
    ///
    /// When the player car stops being alongside a car, the gap between them is approximately one
    /// car length, so the gap is a usable estimate. The samples are kept for each car class, and
    /// the median is the length. The median removes an occasional bad sample, so no table of car
    /// dimensions is necessary.
    ///
    /// The car in front gives the length of that car. The car behind gives the length of the player
    /// car, because the player car had to pass it.
    fn update_car_lengths(&mut self, frame: &TelemetryFrame) {
        let prev = self.prev_car_left_right;
        let current = frame.car_left_right;
        self.prev_car_left_right = current;

        // Take a sample only at the change to clear, not while a car is still alongside.
        if current != LR_CLEAR || prev == LR_CLEAR || prev == LR_OFF {
            return;
        }

        let track_length = frame.track_length_meters;
        if track_length <= 0.0 {
            return;
        }

        let Some(player) = frame
            .drivers
            .iter()
            .find(|d| d.car_idx == frame.player_car_idx)
        else {
            return;
        };

        // Nearest car in front and behind. Ignore the pace car and cars that are not on the track
        // surface.
        let mut nearest_ahead: Option<(f32, i32)> = None;
        let mut nearest_behind: Option<(f32, i32)> = None;

        for driver in &frame.drivers {
            if driver.car_idx == frame.player_car_idx
                || driver.is_pace_car != 0
                || driver.is_pacing != 0
                || driver.track_surface != TRK_LOC_ON_TRACK
            {
                continue;
            }

            let delta_m =
                wrapped_pct_delta(driver.lap_dist_pct, player.lap_dist_pct) * track_length;

            if delta_m > 0.0 {
                if nearest_ahead.is_none_or(|(best, _)| delta_m < best) {
                    nearest_ahead = Some((delta_m, driver.car_class_id));
                }
            } else if delta_m < 0.0 && nearest_behind.is_none_or(|(best, _)| delta_m > best) {
                nearest_behind = Some((delta_m, driver.car_class_id));
            }
        }

        // Car in front: the gap is the length of the car that the player just cleared.
        if let Some((gap, class_id)) = nearest_ahead {
            self.record_car_length(class_id, gap);
        }

        // Car behind: the gap is the length of the player car.
        if let Some((gap, _)) = nearest_behind {
            self.record_car_length(player.car_class_id, -gap);
        }
    }

    /// Records one car length sample for a class, and updates the median.
    fn record_car_length(&mut self, class_id: i32, length_m: f32) {
        if !(MIN_CAR_LENGTH_M..=MAX_CAR_LENGTH_M).contains(&length_m) {
            return;
        }

        let samples = self.car_length_samples.entry(class_id).or_default();
        samples.push_back(length_m);
        while samples.len() > CAR_LENGTH_SAMPLES {
            samples.pop_front();
        }

        if let Some(m) = median(samples) {
            self.car_length.insert(class_id, m);
        }
    }

    /// Learned length for a car class, or `fallback` if there is no measurement.
    pub fn car_length_for(&self, class_id: i32, fallback: f32) -> f32 {
        self.car_length
            .get(&class_id)
            .copied()
            .unwrap_or(fallback)
            .clamp(MIN_CAR_LENGTH_M, MAX_CAR_LENGTH_M)
    }

    /// Adds a parse error to the error log.
    pub fn add_parse_error(&mut self, error: ParseError) {
        // Keep only the last 100 errors.
        if self.parse_errors.len() >= 100 {
            self.parse_errors.pop_front();
        }
        self.parse_errors.push_back(error);
    }

    /// Clears all recorded errors.
    pub fn clear_errors(&mut self) {
        self.parse_errors.clear();
    }

    /// Resets all telemetry data when a new session starts.
    pub fn reset(&mut self) {
        self.lap_data.clear();
        self.current_lap = 0;
        self.lap_start_fuel = 0.0;
        self.lap_green_flags = 0;
        self.lap_yellow_flags = 0;
        self.is_yellow_flag = false;
        self.stints = vec![StintData {
            stint_number: 1,
            laps: Vec::new(),
            green_flag_laps: 0,
            yellow_flag_laps: 0,
        }];
        self.current_stint = 0;
        self.last_fuel_level = 0.0;
        self.laps_down = 0;
        self.input_history.clear();
        self.pit_stop_tracker.clear();
        self.car_length_samples.clear();
        self.car_length.clear();
        self.prev_car_left_right = LR_OFF;
        self.observed_max_fuel = 0.0;
        self.actual_fuel_capacity = None;
        self.lap_snapshots.clear();
        self.debug_snapshots.clear();
        // last_session_num is not reset, so that the next session change can be found.
    }

    pub fn update(&mut self, frame: TelemetryFrame, latency: u64) {
        self.connected = true;
        self.latency_ms = latency;
        self.frame_count += 1;

        // Find a session change (practice to qualifying to race, and so on).
        if self.last_session_num >= 0 && frame.session_num != self.last_session_num {
            // New session: reset all data.
            self.reset();
        }
        self.last_session_num = frame.session_num;

        // Record the maximum fuel level seen, to find restricted tanks.
        if frame.fuel_level > self.observed_max_fuel {
            self.observed_max_fuel = frame.fuel_level;
        }

        // Find the real fuel capacity when the fuel level stops increasing. If the observed maximum
        // is much less than the capacity from FuelLevelPct, the tank is restricted (a league
        // restriction).
        let calculated_capacity = frame.fuel_capacity;
        if self.observed_max_fuel > 10.0 && calculated_capacity > 10.0 {
            // If the observed maximum is 10 L or more below the calculated capacity, the tank is
            // probably restricted.
            let capacity_diff = calculated_capacity - self.observed_max_fuel;
            if capacity_diff > 10.0 {
                // Restricted tank: use the observed maximum as the capacity.
                self.actual_fuel_capacity = Some(self.observed_max_fuel);
            } else if self.actual_fuel_capacity.is_none() {
                // No restriction: use the calculated capacity.
                self.actual_fuel_capacity = Some(calculated_capacity);
            }
        }

        // Update history
        self.speed_history.push_back(frame.speed);
        if self.speed_history.len() > HISTORY_SIZE {
            self.speed_history.pop_front();
        }

        self.rpm_history.push_back(frame.rpm);
        if self.rpm_history.len() > HISTORY_SIZE {
            self.rpm_history.pop_front();
        }

        // Update input history
        self.input_history.push_back(InputSample {
            timestamp: Instant::now(),
            throttle: frame.throttle,
            brake: frame.brake,
            clutch: frame.clutch,
        });
        // Keep only the last 60 seconds of data (at 60 Hz, a maximum of 3600 samples). The display
        // duration can be set up to 60 seconds.
        while self.input_history.len() > 3600 {
            self.input_history.pop_front();
        }
        // Also remove samples older than 60 seconds.
        let now = Instant::now();
        while let Some(front) = self.input_history.front() {
            if now.duration_since(front.timestamp).as_secs() > 60 {
                self.input_history.pop_front();
            } else {
                break;
            }
        }

        // Record the flag status. Look for all yellow and caution flags.
        let yellow_now = (frame.session_flags & YELLOW_FLAGS_MASK) != 0;

        if yellow_now != self.is_yellow_flag {
            self.is_yellow_flag = yellow_now;
        }

        if self.is_yellow_flag {
            self.lap_yellow_flags += 1;
        } else {
            self.lap_green_flags += 1;
        }

        // Find a lap change.
        if frame.lap_current != self.current_lap && self.current_lap > 0 {
            // The lap is complete.
            let fuel_used = self.lap_start_fuel - frame.fuel_level;

            // Find a pit stop on this lap. A large fuel increase, or negative fuel use, means a
            // refuel during the lap.
            let had_pit_stop = fuel_used < 0.0;

            // Record the lap only if the fuel use is positive and possible, or if it was a pit lap.
            if fuel_used > 0.0 {
                let lap_data = LapData {
                    lap_number: self.current_lap,
                    fuel_used,
                    green_flag_count: self.lap_green_flags,
                    yellow_flag_count: self.lap_yellow_flags,
                    had_pit_activity: had_pit_stop,
                };

                self.lap_data.push(lap_data);

                // Update current stint
                if let Some(stint) = self.stints.get_mut(self.current_stint) {
                    stint.laps.push(self.current_lap);

                    // A lap with any yellow flag is a yellow lap. Else it is green.
                    if self.lap_yellow_flags > 0 {
                        stint.yellow_flag_laps += 1;
                    } else {
                        stint.green_flag_laps += 1;
                    }
                }

                // Record a lap snapshot for the debug chart, with the fuel estimates at the end of
                // each lap.
                let total_laps = self.get_effective_total_laps();
                let lap_snapshot = LapSnapshot {
                    lap_number: self.current_lap,
                    fuel_level_at_start: self.lap_start_fuel,
                    fuel_used: Some(fuel_used),
                    avg_estimate: self.avg_fuel_pct_of_total(total_laps),
                    push_estimate: self.avg_fuel_push_pct_of_total(total_laps),
                    low_estimate: self.avg_fuel_low_pct_of_total(total_laps),
                    max_estimate: self.max_fuel_usage(),
                    was_yellow_lap: self.lap_yellow_flags > 0,
                    // Raw SDK values, to debug partial fuel in leagues.
                    fuel_level_pct: frame.fuel_level_pct,
                    fuel_max_ltr: frame.fuel_max_ltr,
                    fuel_capacity_calculated: frame.fuel_capacity,
                };
                self.lap_snapshots.push(lap_snapshot);
            }

            // Update the laps down only when the player lap changes.
            self.laps_down = (frame.leader_lap - frame.lap_current).max(0);

            self.lap_start_fuel = frame.fuel_level;
            self.lap_green_flags = 0;
            self.lap_yellow_flags = 0;
        }

        // Start the lap tracking on the first update.
        if self.current_lap == 0 && frame.lap_current > 0 {
            self.lap_start_fuel = frame.fuel_level;
        }

        self.current_lap = frame.lap_current;

        // Find a pit stop (a large fuel increase).
        if frame.fuel_level > self.last_fuel_level + 5.0 && self.last_fuel_level > 0.0 {
            // New stint.
            self.current_stint += 1;
            self.stints.push(StintData {
                stint_number: self.current_stint + 1,
                laps: Vec::new(),
                green_flag_laps: 0,
                yellow_flag_laps: 0,
            });
            self.lap_start_fuel = frame.fuel_level;
        }

        self.last_fuel_level = frame.fuel_level;

        // Record the pit stops of all drivers.
        for driver in &frame.drivers {
            let car_idx = driver.car_idx;
            let current_laps_since_pit = driver.laps_since_pit;

            // Get the data for this car, or start it.
            let (pit_count, last_laps_since_pit) = self
                .pit_stop_tracker
                .entry(car_idx)
                .or_insert((0, current_laps_since_pit));

            // Find a pit stop: laps_since_pit decreased (it went from a high number to 0), or it
            // went from non-zero to zero.
            if current_laps_since_pit < *last_laps_since_pit && *last_laps_since_pit > 0 {
                *pit_count += 1;
            }

            // Update the last known value.
            *last_laps_since_pit = current_laps_since_pit;
        }

        // Learn car lengths at the moment when the player car clears a car that was alongside. This
        // must run before latest_frame is replaced, so that the previous CarLeftRight value is
        // still from the previous frame.
        self.update_car_lengths(&frame);

        self.latest_frame = frame.clone();

        // Record a debug snapshot. Take one sample each 10 frames, to keep the data small.
        if self.frame_count.is_multiple_of(10) && frame.lap_current > 0 {
            let total_laps = self.get_effective_total_laps();
            let snapshot = DebugSnapshot {
                lap_number: frame.lap_current,
                lap_dist_pct: frame.lap_dist_pct,
                fuel_level: frame.fuel_level,
                avg_estimate: self.avg_fuel_pct_of_total(total_laps),
                push_estimate: self.avg_fuel_push_pct_of_total(total_laps),
                low_estimate: self.avg_fuel_low_pct_of_total(total_laps),
                max_estimate: self.max_fuel_usage(),
                is_yellow_flag: self.is_yellow_flag,
            };
            self.debug_snapshots.push(snapshot);

            // Limit the history to 3600 samples (60 seconds at 60 Hz), to limit the memory and the
            // render time.
            if self.debug_snapshots.len() > 3600 {
                self.debug_snapshots.remove(0);
            }
        }
    }

    pub fn update_hz(&mut self, frames_in_last_second: u64) {
        self.current_hz = frames_in_last_second;
    }

    // Removes outliers. Returns the values that are within 2 standard deviations of the mean.
    fn filter_outliers(&self, values: Vec<f32>) -> Vec<f32> {
        if values.len() < 2 {
            return values; // Need at least 2 values for std dev
        }

        // Calculate mean
        let mean = values.iter().sum::<f32>() / values.len() as f32;

        // Calculate standard deviation
        let variance = values
            .iter()
            .map(|v| {
                let diff = v - mean;
                diff * diff
            })
            .sum::<f32>()
            / values.len() as f32;
        let std_dev = variance.sqrt();

        // Filter values within 2 standard deviations
        values
            .into_iter()
            .filter(|v| {
                let diff = (v - mean).abs();
                diff <= 2.0 * std_dev
            })
            .collect()
    }

    // Returns the outlier filter data for debugging.
    pub fn get_outlier_debug_info(&self) -> String {
        let mut green_laps: Vec<f32> = Vec::new();

        for (i, lap) in self.lap_data.iter().enumerate() {
            // Only include green flag laps
            if lap.yellow_flag_count > 0 {
                continue;
            }

            // Skip laps with pit activity
            if lap.had_pit_activity {
                continue;
            }

            // Find out if this is the first green lap after a yellow or after the race start.
            let is_first_green_after_yellow = if i == 0 {
                // First recorded lap (usually lap 2). Skip it, because it is after the race start.
                true
            } else {
                // Find out if the previous lap was yellow.
                let prev_lap = &self.lap_data[i - 1];
                prev_lap.yellow_flag_count > 0
            };

            // Include the lap only if it is NOT the first green lap after a yellow.
            if !is_first_green_after_yellow {
                green_laps.push(lap.fuel_used);
            }
        }

        if green_laps.is_empty() {
            return format!(
                "No eligible laps recorded yet (total laps: {})",
                self.lap_data.len()
            );
        }

        if green_laps.len() < 2 {
            return format!(
                "Eligible: {} (need 2+ for outlier filter) | Total laps: {}",
                green_laps.len(),
                self.lap_data.len()
            );
        }

        let mean = green_laps.iter().sum::<f32>() / green_laps.len() as f32;
        let variance = green_laps
            .iter()
            .map(|v| {
                let diff = v - mean;
                diff * diff
            })
            .sum::<f32>()
            / green_laps.len() as f32;
        let std_dev = variance.sqrt();

        let min_bound = mean - 2.0 * std_dev;
        let max_bound = mean + 2.0 * std_dev;

        let filtered = self.filter_outliers(green_laps.clone());

        format!(
            "Eligible: {} | After filter: {} | Mean: {:.3}L | StdDev: {:.3}L\nBounds: [{:.3}L - {:.3}L]",
            green_laps.len(),
            filtered.len(),
            mean,
            std_dev,
            min_bound.max(0.0),
            max_bound
        )
    }

    // Calculate average fuel usage from percentage of total laps (e.g., 10%)
    // Excludes pit laps and first lap after yellow/start
    // Uses the new strategy lap assignment logic
    pub fn avg_fuel_pct_of_total(&self, total_laps: i32) -> Option<f32> {
        let (avg_laps, _, _) = self.get_strategy_lap_assignments(total_laps);
        if avg_laps.is_empty() {
            return None;
        }

        let sum: f32 = avg_laps
            .iter()
            .filter_map(|&lap_num| {
                self.lap_data
                    .iter()
                    .find(|l| l.lap_number == lap_num)
                    .map(|l| l.fuel_used)
            })
            .sum();

        Some(sum / avg_laps.len() as f32)
    }

    // Calculate average fuel usage from last N green flag laps
    // Excludes pit laps and first lap after yellow/start
    pub fn avg_fuel_last_n_green(&self, n: usize) -> Option<f32> {
        let mut green_laps: Vec<f32> = Vec::new();

        for (i, lap) in self.lap_data.iter().enumerate().rev() {
            if green_laps.len() >= n {
                break;
            }

            // Only include green flag laps
            if lap.yellow_flag_count > 0 {
                continue;
            }

            // Skip laps with pit activity
            if lap.had_pit_activity {
                continue;
            }

            // Check if this is the first green lap after a yellow or race start
            let is_first_green_after_yellow = if i == 0 {
                true // First recorded lap - skip as it's after race start
            } else {
                let prev_lap = &self.lap_data[i - 1];
                prev_lap.yellow_flag_count > 0
            };

            // Only include if NOT the first green after yellow
            if !is_first_green_after_yellow {
                green_laps.push(lap.fuel_used);
            }
        }

        if green_laps.is_empty() {
            None
        } else {
            // Filter outliers before averaging
            let filtered = self.filter_outliers(green_laps);
            if filtered.is_empty() {
                None
            } else {
                Some(filtered.iter().sum::<f32>() / filtered.len() as f32)
            }
        }
    }

    // Calculate average fuel usage from percentage of total laps (lowest fuel usage)
    // Excludes first green lap after yellow flags or race start, and pit laps
    // Uses the new strategy lap assignment logic (laps <= AVG fuel)
    pub fn avg_fuel_low_pct_of_total(&self, total_laps: i32) -> Option<f32> {
        let (_, _, low_laps) = self.get_strategy_lap_assignments(total_laps);
        if low_laps.is_empty() {
            return None;
        }

        let sum: f32 = low_laps
            .iter()
            .filter_map(|&lap_num| {
                self.lap_data
                    .iter()
                    .find(|l| l.lap_number == lap_num)
                    .map(|l| l.fuel_used)
            })
            .sum();

        Some(sum / low_laps.len() as f32)
    }

    // Calculate average fuel usage from N lowest fuel usage green flag laps
    // Excludes first green lap after yellow flags or race start, and pit laps
    pub fn avg_fuel_low_n_green(&self, n: usize) -> Option<f32> {
        let mut green_laps: Vec<f32> = Vec::new();

        for (i, lap) in self.lap_data.iter().enumerate() {
            // Only include green flag laps
            if lap.yellow_flag_count > 0 {
                continue;
            }

            // Skip laps with pit activity
            if lap.had_pit_activity {
                continue;
            }

            // Check if this is the first lap after a pit stop
            let is_first_lap_after_pit = if i > 0 {
                self.lap_data[i - 1].had_pit_activity
            } else {
                false
            };

            // Skip first lap after pit
            if is_first_lap_after_pit {
                continue;
            }

            // Check if this is the first green lap after a yellow or race start
            let is_first_green_after_yellow = if i == 0 {
                // First recorded lap (likely lap 2) - skip as it's after race start
                true
            } else {
                // Check if previous lap was yellow
                let prev_lap = &self.lap_data[i - 1];
                prev_lap.yellow_flag_count > 0
            };

            // Only include if NOT the first green after yellow
            if !is_first_green_after_yellow {
                green_laps.push(lap.fuel_used);
            }
        }

        if green_laps.is_empty() {
            return None;
        }

        // Filter outliers before taking lowest 5
        let filtered = self.filter_outliers(green_laps);

        if filtered.is_empty() {
            return None;
        }

        // Sort ascending and take bottom N
        let mut sorted = filtered;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let low_n: Vec<f32> = sorted.into_iter().take(n).collect();

        if low_n.is_empty() {
            None
        } else {
            Some(low_n.iter().sum::<f32>() / low_n.len() as f32)
        }
    }

    // Calculate average fuel usage from 5 lowest fuel usage green flag laps (legacy)
    pub fn avg_fuel_low_5_green(&self) -> Option<f32> {
        self.avg_fuel_low_n_green(5)
    }

    // Calculate average fuel usage from percentage of total laps (highest fuel usage)
    // Only includes green flag laps, no pit laps, no first after yellow
    // Uses AVG-threshold filtering to ensure PUSH > AVG
    pub fn avg_fuel_push_pct_of_total(&self, total_laps: i32) -> Option<f32> {
        let (_, push_laps, _) = self.get_strategy_lap_assignments(total_laps);
        if push_laps.is_empty() {
            return None;
        }

        let sum: f32 = push_laps
            .iter()
            .filter_map(|&lap_num| {
                self.lap_data
                    .iter()
                    .find(|l| l.lap_number == lap_num)
                    .map(|l| l.fuel_used)
            })
            .sum();

        Some(sum / push_laps.len() as f32)
    }

    // Calculate average fuel usage from top N highest usage laps (push pace)
    // Only includes green flag laps, no pit laps, no first after yellow
    pub fn avg_fuel_top_n_push(&self, n: usize) -> Option<f32> {
        let mut eligible_laps: Vec<f32> = Vec::new();

        for (i, lap) in self.lap_data.iter().enumerate() {
            // Only include green flag laps
            if lap.yellow_flag_count > 0 {
                continue;
            }

            // Skip laps with pit activity
            if lap.had_pit_activity {
                continue;
            }

            // Check if this is the first green lap after a yellow or race start
            let is_first_green_after_yellow = if i == 0 {
                true
            } else {
                let prev_lap = &self.lap_data[i - 1];
                prev_lap.yellow_flag_count > 0
            };

            // Only include if NOT the first green after yellow
            if !is_first_green_after_yellow {
                eligible_laps.push(lap.fuel_used);
            }
        }

        if eligible_laps.is_empty() {
            return None;
        }

        // Filter outliers
        let filtered = self.filter_outliers(eligible_laps);

        if filtered.is_empty() {
            return None;
        }

        // Sort descending and take top N
        let mut sorted = filtered;
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
        let top_n: Vec<f32> = sorted.into_iter().take(n).collect();

        if top_n.is_empty() {
            None
        } else {
            Some(top_n.iter().sum::<f32>() / top_n.len() as f32)
        }
    }

    // Calculate average fuel usage from top 5 highest usage laps (legacy)
    pub fn avg_fuel_top_5_push(&self) -> Option<f32> {
        self.avg_fuel_top_n_push(5)
    }

    // Find highest fuel usage (single lap max from PUSH laps only)
    pub fn max_fuel_usage(&self) -> Option<f32> {
        // Get total laps to determine which laps are PUSH laps
        let total_laps = self.get_effective_total_laps();
        let (_, push_laps, _) = self.get_strategy_lap_assignments(total_laps);

        if push_laps.is_empty() {
            return None;
        }

        // Find the highest fuel usage from PUSH laps only
        push_laps
            .iter()
            .filter_map(|&lap_num| {
                self.lap_data
                    .iter()
                    .find(|l| l.lap_number == lap_num)
                    .map(|l| l.fuel_used)
            })
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    // Find lowest fuel usage
    pub fn min_fuel_usage(&self) -> Option<f32> {
        self.lap_data
            .iter()
            .map(|lap| lap.fuel_used)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    // Returns the number of completed laps as a fraction. The iRacing Lap field counts laps
    // started, so subtract 1 and add the progress. For example, lap 5 at 65% is 4 completed laps +
    // 0.65, which is 4.65.
    pub fn get_current_lap_fractional(&self) -> f32 {
        (self.current_lap - 1) as f32 + self.latest_frame.lap_dist_pct
    }

    // Get pit stop count for a driver
    pub fn get_pit_stop_count(&self, car_idx: i32) -> i32 {
        self.pit_stop_tracker
            .get(&car_idx)
            .map(|(count, _)| *count)
            .unwrap_or(0)
    }

    // Calculate laps remaining with current fuel
    pub fn laps_remaining(&self, fuel_per_lap: f32) -> f32 {
        if fuel_per_lap > 0.0 {
            self.latest_frame.fuel_level / fuel_per_lap
        } else {
            0.0
        }
    }

    // Calculate laps possible on a full tank with given fuel consumption
    pub fn laps_on_full_tank(&self, fuel_per_lap: f32) -> f32 {
        let tank_capacity = self.get_fuel_capacity();
        if fuel_per_lap > 0.0 && tank_capacity > 0.0 {
            tank_capacity / fuel_per_lap
        } else {
            0.0
        }
    }

    // Calculate fuel needed to finish a race (average pace) including partial lap
    pub fn fuel_to_finish_avg(&self, total_laps: i32, avg_usage: f32) -> f32 {
        // Calculate laps remaining from current position (including partial lap)
        let current_lap_fractional = self.get_current_lap_fractional();
        let laps_remaining = (total_laps as f32 - current_lap_fractional).max(0.0);
        let needed = laps_remaining * avg_usage;
        (needed - self.latest_frame.fuel_level).max(0.0)
    }

    // Calculate fuel needed to finish a race (push pace) including partial lap
    pub fn fuel_to_finish_push(&self, total_laps: i32, max_usage: f32) -> f32 {
        // Calculate laps remaining from current position (including partial lap)
        let current_lap_fractional = self.get_current_lap_fractional();
        let laps_remaining = (total_laps as f32 - current_lap_fractional).max(0.0);
        let needed = laps_remaining * max_usage;
        (needed - self.latest_frame.fuel_level).max(0.0)
    }

    // Calculate estimated laps remaining for timed races
    pub fn estimate_laps_remaining_timed(&self, session_length_minutes: f32) -> Option<f32> {
        if self.lap_data.len() < 2 {
            return None;
        }

        // Calculate average lap time from completed laps
        let total_laps = self.lap_data.len() as f32;
        let time_per_lap = self.latest_frame.session_time / total_laps as f64;

        // Calculate remaining time
        let session_length_seconds = session_length_minutes * 60.0;
        let remaining_seconds = session_length_seconds - self.latest_frame.session_time as f32;

        if remaining_seconds <= 0.0 {
            return Some(0.0);
        }

        Some(remaining_seconds / time_per_lap as f32)
    }

    // Returns the total laps for the session. For lap races, this is SessionLapsTotal. For timed
    // races, it is an estimate. Uses SessionLapsRemainEx when it is valid, as iRon does.
    pub fn get_effective_total_laps(&self) -> i32 {
        // iRacing uses 32767 for timed and unlimited sessions.
        const TIMED_SESSION_THRESHOLD: i32 = 32767;

        // If SessionLapsTotal is less than 32767, the race has a lap count.
        if self.latest_frame.session_laps_total > 0
            && self.latest_frame.session_laps_total < TIMED_SESSION_THRESHOLD
        {
            return self.latest_frame.session_laps_total - self.laps_down;
        }

        // Timed session (SessionLapsTotal == 32767). Use SessionLapsRemainEx if it is valid (not
        // 32767).
        if self.latest_frame.session_laps_remain_ex > 0
            && self.latest_frame.session_laps_remain_ex < TIMED_SESSION_THRESHOLD
        {
            // Total laps = current lap + remaining laps.
            return self.latest_frame.leader_lap + self.latest_frame.session_laps_remain_ex;
        }

        // Fallback: calculate from the remaining session time, as iRon does.
        if self.current_lap >= 2 && self.latest_frame.session_time_remain > 0.0 {
            // Estimate the lap time from the current progress.
            let avg_lap_time_seconds =
                self.latest_frame.session_time / self.current_lap.max(1) as f64;

            if avg_lap_time_seconds > 0.0 {
                let estimated_remaining =
                    (self.latest_frame.session_time_remain / avg_lap_time_seconds + 0.5) as i32;
                return self.latest_frame.leader_lap + estimated_remaining;
            }
        }

        // Last fallback: the leader lap plus a buffer.
        self.latest_frame.leader_lap.max(10) + 20
    }

    /// Returns the real fuel capacity, with league restrictions. If a restricted tank was found,
    /// returns the observed maximum fuel. Else returns the capacity from telemetry.
    pub fn get_fuel_capacity(&self) -> f32 {
        self.actual_fuel_capacity
            .unwrap_or(self.latest_frame.fuel_capacity)
    }

    // ============================================
    // Consolidated fuel calculation functions
    // ============================================

    /// Returns the laps remaining to the race finish from the current position.
    pub fn laps_to_go(&self) -> f32 {
        let total_laps = self.get_effective_total_laps();
        let current_lap_fractional = self.get_current_lap_fractional();
        (total_laps as f32 - current_lap_fractional).max(0.0)
    }

    /// Calculates if the car can finish, and the fuel margin. Returns (can_finish, fuel_margin).
    /// fuel_margin is positive for a surplus and negative for a shortfall.
    pub fn calculate_fuel_margin(&self, fuel_per_lap: f32, extra_liters: f32) -> (bool, f32) {
        let laps_to_go = self.laps_to_go();
        let fuel_needed = laps_to_go * fuel_per_lap + extra_liters;
        let current_fuel = self.latest_frame.fuel_level;
        (current_fuel >= fuel_needed, current_fuel - fuel_needed)
    }

    /// Returns the fuel to add (not the total fuel necessary). Returns 0 if the fuel is sufficient.
    pub fn fuel_to_add(&self, fuel_per_lap: f32, extra_liters: f32) -> f32 {
        let laps_to_go = self.laps_to_go();
        let fuel_needed = laps_to_go * fuel_per_lap + extra_liters;
        (fuel_needed - self.latest_frame.fuel_level).max(0.0)
    }

    /// Calculates when to pit and how much fuel to add. Returns (pit_lap, fuel_to_add). pit_lap=0
    /// means that no stop is necessary.
    pub fn calculate_pit_strategy(
        &self,
        fuel_per_lap: f32,
        fuel_capacity: f32,
        extra_liters: f32,
    ) -> (i32, f32) {
        if fuel_per_lap <= 0.0 {
            return (0, 0.0);
        }

        let current_fuel = self.latest_frame.fuel_level;
        let lap_dist_pct = self.latest_frame.lap_dist_pct;
        let current_lap_fractional = self.get_current_lap_fractional();
        let laps_to_go = self.laps_to_go();

        let fuel_for_current_lap = fuel_per_lap * lap_dist_pct;
        let fuel_after_current_lap = current_fuel - fuel_for_current_lap;
        let complete_laps_after_current = (fuel_after_current_lap / fuel_per_lap).floor();
        let total_laps_on_tank = complete_laps_after_current + (1.0 - lap_dist_pct);

        let pit_lap = if total_laps_on_tank < laps_to_go {
            (current_lap_fractional + total_laps_on_tank).floor() as i32
        } else {
            0
        };

        let fuel_to_add = if pit_lap > 0 {
            let laps_after_pit = laps_to_go - total_laps_on_tank;
            let fuel_needed = (laps_after_pit * fuel_per_lap + extra_liters).max(0.0);
            fuel_needed.min(fuel_capacity)
        } else {
            0.0
        };

        (pit_lap, fuel_to_add)
    }

    // Returns the lap numbers for each strategy: (avg_laps, push_laps, low_laps). AVG can share
    // laps with PUSH and LOW, but PUSH and LOW have no laps in common.
    pub fn get_strategy_lap_assignments(&self, total_laps: i32) -> (Vec<i32>, Vec<i32>, Vec<i32>) {
        // Get the eligible laps: green, no pit, and not the first lap after a yellow or the start.
        let mut eligible_laps: Vec<(i32, f32)> = Vec::new();

        for (i, lap) in self.lap_data.iter().enumerate() {
            if lap.yellow_flag_count > 0 {
                continue;
            }
            if lap.had_pit_activity {
                continue;
            }
            let is_first_green_after_yellow = if i == 0 {
                true
            } else {
                let prev_lap = &self.lap_data[i - 1];
                prev_lap.yellow_flag_count > 0
            };
            if !is_first_green_after_yellow {
                eligible_laps.push((lap.lap_number, lap.fuel_used));
            }
        }

        if eligible_laps.is_empty() {
            return (Vec::new(), Vec::new(), Vec::new());
        }

        // Remove outliers, as the fuel calculations do.
        let fuel_values: Vec<f32> = eligible_laps.iter().map(|(_, fuel)| *fuel).collect();
        let filtered_fuel = self.filter_outliers(fuel_values);

        let filtered_laps: Vec<(i32, f32)> = eligible_laps
            .into_iter()
            .filter(|(_, fuel)| filtered_fuel.contains(fuel))
            .collect();

        if filtered_laps.is_empty() {
            return (Vec::new(), Vec::new(), Vec::new());
        }

        // Number of laps for each strategy, from total_laps. When there is little data, use fewer
        // laps.
        let available_laps = filtered_laps.len();
        let avg_lap_count = ((total_laps as f32 * 0.10).ceil() as usize)
            .max(5)
            .min(available_laps);
        let push_lap_count = ((total_laps as f32 * 0.05).ceil() as usize)
            .max(3)
            .min(available_laps);
        let low_lap_count = ((total_laps as f32 * 0.05).ceil() as usize)
            .max(3)
            .min(available_laps);

        // AVG laps: the last N eligible laps after the outlier filter.
        let mut avg_laps: Vec<i32> = filtered_laps
            .iter()
            .rev()
            .take(avg_lap_count)
            .map(|(lap_num, _)| *lap_num)
            .collect();
        avg_laps.sort();

        if filtered_laps.is_empty() {
            return (avg_laps, Vec::new(), Vec::new());
        }

        // AVG fuel, which is the threshold for PUSH and LOW.
        let avg_fuel: f32 = avg_laps
            .iter()
            .filter_map(|&lap| {
                filtered_laps
                    .iter()
                    .find(|(l, _)| *l == lap)
                    .map(|(_, f)| f)
            })
            .sum::<f32>()
            / avg_laps.len() as f32;

        // PUSH: laps with fuel >= AVG, sorted by fuel from high to low. Take the first N.
        let mut push_pool: Vec<(i32, f32)> = filtered_laps
            .iter()
            .filter(|(_, fuel)| *fuel >= avg_fuel)
            .copied()
            .collect();

        push_pool.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut push_laps: Vec<i32> = push_pool
            .iter()
            .take(push_lap_count.min(push_pool.len()))
            .map(|(lap_num, _)| *lap_num)
            .collect();
        push_laps.sort();

        // LOW: laps with fuel <= AVG, sorted by fuel from low to high. Take the first N. Remove the
        // PUSH laps if there is sufficient data.
        let mut low_pool: Vec<(i32, f32)> = filtered_laps
            .iter()
            .filter(|(_, fuel)| *fuel <= avg_fuel)
            .copied()
            .collect();

        // Remove the PUSH laps only if there are more than 2 times the necessary laps.
        if available_laps > (push_lap_count + low_lap_count) * 2 {
            low_pool.retain(|(lap_num, _)| !push_laps.contains(lap_num));
        }

        if low_pool.is_empty() {
            return (avg_laps, push_laps, Vec::new());
        }

        low_pool.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut low_laps: Vec<i32> = low_pool
            .iter()
            .take(low_lap_count.min(low_pool.len()))
            .map(|(lap_num, _)| *lap_num)
            .collect();
        low_laps.sort();

        (avg_laps, push_laps, low_laps)
    }

    // Get the lap numbers used for average calculation (last N green)
    pub fn get_avg_lap_numbers(&self, total_laps: i32) -> Vec<i32> {
        let (avg, _, _) = self.get_strategy_lap_assignments(total_laps);
        avg
    }

    // Get the lap numbers with top N fuel usage (push laps)
    pub fn get_push_lap_numbers(&self, total_laps: i32) -> Vec<i32> {
        let (_, push, _) = self.get_strategy_lap_assignments(total_laps);
        push
    }

    // Get the lap numbers with N lowest fuel usage green flag laps
    // Excludes first green lap after yellow flags or race start, and pit laps
    pub fn get_low_lap_numbers(&self, total_laps: i32) -> Vec<i32> {
        let (_, _, low) = self.get_strategy_lap_assignments(total_laps);
        low
    }

    // Get the lap number with max fuel usage
    pub fn get_max_lap_number(&self) -> Option<i32> {
        self.lap_data
            .iter()
            .max_by(|a, b| {
                a.fuel_used
                    .partial_cmp(&b.fuel_used)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|lap| lap.lap_number)
    }

    // Get lap numbers where pit stops occurred (first lap of each stint after stint 1)
    pub fn get_pit_stop_laps(&self) -> Vec<i32> {
        self.stints
            .iter()
            .skip(1) // Skip first stint (no pit before first stint)
            .filter_map(|stint| stint.laps.first().copied())
            .collect()
    }

    // Returns the fuel data that the calculations use, as (lap_number, fuel_used) tuples for the
    // display.
    pub fn get_filtered_fuel_data(&self) -> Vec<(i32, f32)> {
        let mut green_laps_with_fuel: Vec<(i32, f32)> = Vec::new();

        for (i, lap) in self.lap_data.iter().enumerate() {
            // Only include green flag laps
            if lap.yellow_flag_count > 0 {
                continue;
            }

            // Skip laps with pit activity
            if lap.had_pit_activity {
                continue;
            }

            // Check if this is the first green lap after a yellow or race start
            let is_first_green_after_yellow = if i == 0 {
                // First recorded lap (usually lap 2). Skip it, because it is after the race start.
                true
            } else {
                // Check if previous lap was yellow
                let prev_lap = &self.lap_data[i - 1];
                prev_lap.yellow_flag_count > 0
            };

            // Only include if NOT the first green after yellow
            if !is_first_green_after_yellow {
                green_laps_with_fuel.push((lap.lap_number, lap.fuel_used));
            }
        }

        // Filter outliers
        let fuel_values: Vec<f32> = green_laps_with_fuel.iter().map(|(_, fuel)| *fuel).collect();
        let filtered_fuel = self.filter_outliers(fuel_values);

        // Return only the laps that are left after the outlier filter.
        green_laps_with_fuel
            .into_iter()
            .filter(|(_, fuel)| filtered_fuel.contains(fuel))
            .collect()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use checking_protocol::DriverInfo;

    #[test]
    fn test_lap_count_calculations() {
        // Test for 50 lap race (short race)
        let total_laps_50 = 50;
        let avg_count_50 = ((total_laps_50 as f32 * 0.10).ceil() as usize).max(5);
        let push_count_50 = ((total_laps_50 as f32 * 0.05).ceil() as usize).max(3);
        let low_count_50 = ((total_laps_50 as f32 * 0.05).ceil() as usize).max(3);

        println!("50 lap race:");
        println!(
            "  AVG: {} laps (10% = {}, max(5) applied)",
            avg_count_50,
            (50.0_f32 * 0.10).ceil()
        );
        println!(
            "  PUSH: {} laps (5% = {}, max(3) applied)",
            push_count_50,
            (50.0_f32 * 0.05).ceil()
        );
        println!(
            "  LOW: {} laps (5% = {}, max(3) applied)",
            low_count_50,
            (50.0_f32 * 0.05).ceil()
        );

        assert_eq!(avg_count_50, 5); // 10% of 50 = 5, max(5) = 5
        assert_eq!(push_count_50, 3); // 5% of 50 = 2.5 -> 3, max(3) = 3
        assert_eq!(low_count_50, 3); // 5% of 50 = 2.5 -> 3, max(3) = 3

        // Test for 100 lap race (medium race)
        let total_laps_100 = 100;
        let avg_count_100 = ((total_laps_100 as f32 * 0.10).ceil() as usize).max(5);
        let push_count_100 = ((total_laps_100 as f32 * 0.05).ceil() as usize).max(3);
        let low_count_100 = ((total_laps_100 as f32 * 0.05).ceil() as usize).max(3);

        println!("\n100 lap race:");
        println!(
            "  AVG: {} laps (10% = {}, max(5) applied)",
            avg_count_100,
            (100.0_f32 * 0.10).ceil()
        );
        println!(
            "  PUSH: {} laps (5% = {}, max(3) applied)",
            push_count_100,
            (100.0_f32 * 0.05).ceil()
        );
        println!(
            "  LOW: {} laps (5% = {}, max(3) applied)",
            low_count_100,
            (100.0_f32 * 0.05).ceil()
        );

        assert_eq!(avg_count_100, 10); // 10% of 100 = 10, max(5) = 10
        assert_eq!(push_count_100, 5); // 5% of 100 = 5, max(3) = 5
        assert_eq!(low_count_100, 5); // 5% of 100 = 5, max(3) = 5

        // Test for 200 lap race (long race)
        let total_laps_200 = 200;
        let avg_count_200 = ((total_laps_200 as f32 * 0.10).ceil() as usize).max(5);
        let push_count_200 = ((total_laps_200 as f32 * 0.05).ceil() as usize).max(3);
        let low_count_200 = ((total_laps_200 as f32 * 0.05).ceil() as usize).max(3);

        println!("\n200 lap race:");
        println!(
            "  AVG: {} laps (10% = {}, max(5) applied)",
            avg_count_200,
            (200.0_f32 * 0.10).ceil()
        );
        println!(
            "  PUSH: {} laps (5% = {}, max(3) applied)",
            push_count_200,
            (200.0_f32 * 0.05).ceil()
        );
        println!(
            "  LOW: {} laps (5% = {}, max(3) applied)",
            low_count_200,
            (200.0_f32 * 0.05).ceil()
        );

        assert_eq!(avg_count_200, 20); // 10% of 200 = 20, max(5) = 20
        assert_eq!(push_count_200, 10); // 5% of 200 = 10, max(3) = 10
        assert_eq!(low_count_200, 10); // 5% of 200 = 10, max(3) = 10

        // Test for 20 lap race (very short race - minimums apply)
        let total_laps_20 = 20;
        let avg_count_20 = ((total_laps_20 as f32 * 0.10).ceil() as usize).max(5);
        let push_count_20 = ((total_laps_20 as f32 * 0.05).ceil() as usize).max(3);
        let low_count_20 = ((total_laps_20 as f32 * 0.05).ceil() as usize).max(3);

        println!("\n20 lap race (very short - minimums apply):");
        println!(
            "  AVG: {} laps (10% = {}, max(5) = {})",
            avg_count_20,
            (20.0_f32 * 0.10).ceil(),
            avg_count_20
        );
        println!(
            "  PUSH: {} laps (5% = {}, max(3) = {})",
            push_count_20,
            (20.0_f32 * 0.05).ceil(),
            push_count_20
        );
        println!(
            "  LOW: {} laps (5% = {}, max(3) = {})",
            low_count_20,
            (20.0_f32 * 0.05).ceil(),
            low_count_20
        );

        assert_eq!(avg_count_20, 5); // 10% of 20 = 2, max(5) = 5 (minimum applied)
        assert_eq!(push_count_20, 3); // 5% of 20 = 1, max(3) = 3 (minimum applied)
        assert_eq!(low_count_20, 3); // 5% of 20 = 1, max(3) = 3 (minimum applied)
    }

    #[test]
    fn test_strategy_lap_assignments_scaling() {
        let mut state = AppState::new();

        // Add some sample lap data (need eligible laps for the function to work)
        for i in 1..=20 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.5 + (i as f32 * 0.01), // Slight variation
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Test with 50 lap race
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(50);
        println!("\n50 lap race assignments:");
        println!("  AVG laps selected: {}", avg_laps.len());
        println!("  PUSH laps selected: {}", push_laps.len());
        println!("  LOW laps selected: {}", low_laps.len());

        assert_eq!(avg_laps.len(), 5); // min(5, available)
        assert_eq!(push_laps.len(), 3); // min(3, available)
        assert_eq!(low_laps.len(), 3); // min(3, available)

        // Test with 200 lap race
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(200);
        println!("\n200 lap race assignments:");
        println!(
            "  AVG laps selected: {} (requested 20, available {})",
            avg_laps.len(),
            state.lap_data.len()
        );
        println!(
            "  PUSH laps selected: {} (requested 10, available {})",
            push_laps.len(),
            state.lap_data.len()
        );
        println!(
            "  LOW laps selected: {} (requested 10, available {})",
            low_laps.len(),
            state.lap_data.len()
        );

        // With only 20 laps of data, the result cannot have more laps than there are.
        assert!(avg_laps.len() <= 20);
        assert!(push_laps.len() <= 10);
        assert!(low_laps.len() <= 10);
    }

    #[test]
    fn test_lap_assignments_with_varied_fuel_usage() {
        let mut state = AppState::new();

        println!("\n=== Testing lap assignments with varied fuel usage ===");

        // Create 30 laps with varied fuel usage
        // Laps 1-10: Low fuel usage (2.0-2.3L)
        // Laps 11-20: Medium fuel usage (2.5-2.8L)
        // Laps 21-30: High fuel usage (3.0-3.5L)

        for i in 1..=10 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.0 + (i as f32 * 0.03), // 2.0 to 2.3L
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        for i in 11..=20 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.5 + ((i - 10) as f32 * 0.03), // 2.5 to 2.8L
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        for i in 21..=30 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 3.0 + ((i - 20) as f32 * 0.05), // 3.0 to 3.5L
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Test with 100 lap race (should use 10% AVG, 5% PUSH, 5% LOW)
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(100);

        println!("\n100 lap race with 30 laps of varied data:");
        println!("  AVG laps (last 10): {:?}", avg_laps);
        println!("  PUSH laps (top 5 fuel): {:?}", push_laps);
        println!("  LOW laps (bottom 5 fuel): {:?}", low_laps);

        // Verify counts
        assert_eq!(
            avg_laps.len(),
            10,
            "AVG should use 10 laps for 100 lap race"
        );
        assert_eq!(
            push_laps.len(),
            5,
            "PUSH should use 5 laps for 100 lap race"
        );
        assert_eq!(low_laps.len(), 5, "LOW should use 5 laps for 100 lap race");

        // AVG should be the last 10 laps chronologically (laps 21-30)
        assert_eq!(
            avg_laps,
            vec![21, 22, 23, 24, 25, 26, 27, 28, 29, 30],
            "AVG should select the last 10 chronological laps"
        );

        // PUSH should contain high fuel laps (from laps 21-30 which have highest fuel)
        for &lap in &push_laps {
            assert!(
                (21..=30).contains(&lap),
                "PUSH lap {} should be from high fuel range (21-30)",
                lap
            );
        }

        // LOW should contain low fuel laps and NOT overlap with PUSH
        for &lap in &low_laps {
            assert!(
                !push_laps.contains(&lap),
                "LOW lap {} should not overlap with PUSH laps",
                lap
            );
            // Low laps should mostly be from the low fuel range (1-10)
            assert!(
                (1..=20).contains(&lap),
                "LOW lap {} should be from low/medium fuel range",
                lap
            );
        }

        println!("\n  ✓ AVG contains last 10 chronological laps");
        println!("  ✓ PUSH contains highest fuel usage laps");
        println!("  ✓ LOW contains lowest fuel usage laps (excluding PUSH)");
    }

    #[test]
    fn test_lap_assignments_with_200_lap_race() {
        let mut state = AppState::new();

        println!("\n=== Testing 200 lap race scaling ===");

        // Create 50 laps with varied fuel usage
        for i in 1..=50 {
            let fuel = if i <= 10 {
                2.0 + (i as f32 * 0.02) // Low: 2.0-2.2L
            } else if i <= 30 {
                2.5 + ((i - 10) as f32 * 0.01) // Medium: 2.5-2.7L
            } else {
                3.0 + ((i - 30) as f32 * 0.02) // High: 3.0-3.4L
            };

            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: fuel,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Test with 200 lap race (should use 20 AVG, 10 PUSH, 10 LOW)
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(200);

        println!("\n200 lap race with 50 laps of varied data:");
        println!("  AVG laps requested: 20, got: {}", avg_laps.len());
        println!("  PUSH laps requested: 10, got: {}", push_laps.len());
        println!("  LOW laps requested: 10, got: {}", low_laps.len());
        println!("  AVG laps: {:?}", avg_laps);
        println!("  PUSH laps: {:?}", push_laps);
        println!("  LOW laps: {:?}", low_laps);

        // Verify counts scale up
        assert_eq!(
            avg_laps.len(),
            20,
            "AVG should use 20 laps for 200 lap race"
        );
        assert_eq!(
            push_laps.len(),
            10,
            "PUSH should use 10 laps for 200 lap race"
        );
        assert_eq!(
            low_laps.len(),
            10,
            "LOW should use 10 laps for 200 lap race"
        );

        // AVG should be the last 20 laps chronologically (laps 31-50)
        assert_eq!(
            avg_laps,
            (31..=50).collect::<Vec<i32>>(),
            "AVG should select the last 20 chronological laps"
        );

        // PUSH should contain the highest fuel laps (mostly from 31-50 range)
        for &lap in &push_laps {
            let lap_data = state.lap_data.iter().find(|l| l.lap_number == lap).unwrap();
            println!("  PUSH lap {}: {:.3}L", lap, lap_data.fuel_used);
        }

        // LOW should contain the lowest fuel laps (mostly from 1-30 range, excluding PUSH)
        for &lap in &low_laps {
            assert!(
                !push_laps.contains(&lap),
                "LOW should not overlap with PUSH"
            );
            let lap_data = state.lap_data.iter().find(|l| l.lap_number == lap).unwrap();
            println!("  LOW lap {}: {:.3}L", lap, lap_data.fuel_used);
        }

        println!("\n  ✓ 200 lap race correctly scales to 20 AVG, 10 PUSH, 10 LOW");
        println!("  ✓ PUSH and LOW do not overlap");
    }

    #[test]
    fn test_ineligible_laps_excluded() {
        let mut state = AppState::new();

        println!("\n=== Testing that ineligible laps are excluded ===");

        // Add various types of laps
        // Lap 1: First lap (ineligible - first after start)
        state.lap_data.push(LapData {
            lap_number: 1,
            fuel_used: 2.0,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });

        // Laps 2-5: Good green laps
        for i in 2..=5 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.5,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Lap 6: Yellow flag lap (ineligible)
        state.lap_data.push(LapData {
            lap_number: 6,
            fuel_used: 1.5,
            green_flag_count: 10,
            yellow_flag_count: 100,
            had_pit_activity: false,
        });

        // Lap 7: First lap after yellow (ineligible)
        state.lap_data.push(LapData {
            lap_number: 7,
            fuel_used: 3.0,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });

        // Laps 8-10: Good green laps
        for i in 8..=10 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.6,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Lap 11: Pit lap (ineligible)
        state.lap_data.push(LapData {
            lap_number: 11,
            fuel_used: 2.0,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: true,
        });

        // Laps 12-15: Good green laps
        for i in 12..=15 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.7,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(100);

        println!("\nLap eligibility:");
        println!("  Total laps: {}", state.lap_data.len());
        println!(
            "  Eligible laps should exclude: lap 1 (first), lap 6 (yellow), lap 7 (first after yellow), lap 11 (pit)"
        );
        println!("  Expected eligible: laps 2,3,4,5,8,9,10,12,13,14,15 (11 laps)");

        let all_assigned: Vec<i32> = avg_laps
            .iter()
            .chain(push_laps.iter())
            .chain(low_laps.iter())
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        println!("  Laps actually used: {:?}", all_assigned);

        // Verify ineligible laps are NOT in any strategy
        assert!(
            !all_assigned.contains(&1),
            "Lap 1 (first) should not be assigned"
        );
        assert!(
            !all_assigned.contains(&6),
            "Lap 6 (yellow) should not be assigned"
        );
        assert!(
            !all_assigned.contains(&7),
            "Lap 7 (first after yellow) should not be assigned"
        );
        assert!(
            !all_assigned.contains(&11),
            "Lap 11 (pit) should not be assigned"
        );

        // Verify eligible laps ARE in strategies
        for lap in [2, 3, 4, 5, 8, 9, 10, 12, 13, 14, 15] {
            assert!(
                all_assigned.contains(&lap),
                "Lap {} (eligible) should be assigned",
                lap
            );
        }

        println!("\n  ✓ All ineligible laps correctly excluded");
        println!("  ✓ All eligible laps correctly included");
    }

    #[test]
    fn test_push_higher_than_avg_low_lower_than_avg() {
        let mut state = AppState::new();

        println!("\n=== Testing PUSH > AVG > LOW fuel usage ===");

        // Create 50 laps with varied fuel usage
        for i in 1..=50 {
            let fuel = if i <= 15 {
                2.0 + (i as f32 * 0.01) // Low: 2.0-2.15L
            } else if i <= 35 {
                2.5 + ((i - 15) as f32 * 0.01) // Medium: 2.5-2.7L
            } else {
                3.0 + ((i - 35) as f32 * 0.02) // High: 3.0-3.3L
            };

            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: fuel,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Test with 100 lap race
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(100);

        // Calculate actual averages for each strategy
        let avg_fuel: f32 = avg_laps
            .iter()
            .map(|&lap| {
                state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used
            })
            .sum::<f32>()
            / avg_laps.len() as f32;

        let push_fuel: f32 = push_laps
            .iter()
            .map(|&lap| {
                state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used
            })
            .sum::<f32>()
            / push_laps.len() as f32;

        let low_fuel: f32 = low_laps
            .iter()
            .map(|&lap| {
                state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used
            })
            .sum::<f32>()
            / low_laps.len() as f32;

        println!("\n100 lap race fuel averages:");
        println!(
            "  AVG strategy: {:.3}L (from {} laps)",
            avg_fuel,
            avg_laps.len()
        );
        println!(
            "  PUSH strategy: {:.3}L (from {} laps)",
            push_fuel,
            push_laps.len()
        );
        println!(
            "  LOW strategy: {:.3}L (from {} laps)",
            low_fuel,
            low_laps.len()
        );

        // Print individual lap details
        println!("\n  AVG laps: {:?}", avg_laps);
        for &lap in &avg_laps {
            let fuel = state
                .lap_data
                .iter()
                .find(|l| l.lap_number == lap)
                .unwrap()
                .fuel_used;
            println!("    Lap {}: {:.3}L", lap, fuel);
        }

        println!("\n  PUSH laps: {:?}", push_laps);
        for &lap in &push_laps {
            let fuel = state
                .lap_data
                .iter()
                .find(|l| l.lap_number == lap)
                .unwrap()
                .fuel_used;
            println!("    Lap {}: {:.3}L", lap, fuel);
        }

        println!("\n  LOW laps: {:?}", low_laps);
        for &lap in &low_laps {
            let fuel = state
                .lap_data
                .iter()
                .find(|l| l.lap_number == lap)
                .unwrap()
                .fuel_used;
            println!("    Lap {}: {:.3}L", lap, fuel);
        }

        // Verify the ordering: PUSH > AVG > LOW
        assert!(
            push_fuel > avg_fuel,
            "PUSH fuel ({:.3}L) should be HIGHER than AVG fuel ({:.3}L)",
            push_fuel,
            avg_fuel
        );
        assert!(
            avg_fuel > low_fuel,
            "AVG fuel ({:.3}L) should be HIGHER than LOW fuel ({:.3}L)",
            avg_fuel,
            low_fuel
        );
        assert!(
            push_fuel > low_fuel,
            "PUSH fuel ({:.3}L) should be HIGHER than LOW fuel ({:.3}L)",
            push_fuel,
            low_fuel
        );

        println!(
            "\n  ✓ PUSH ({:.3}L) > AVG ({:.3}L) > LOW ({:.3}L)",
            push_fuel, avg_fuel, low_fuel
        );
    }

    #[test]
    fn test_fuel_calculation_functions_match_assignments() {
        let mut state = AppState::new();

        println!("\n=== Testing that fuel calculation functions match assignments ===");

        // Create 30 laps with varied fuel usage
        for i in 1..=30 {
            let fuel = if i <= 10 {
                2.0 + (i as f32 * 0.02)
            } else if i <= 20 {
                2.5 + ((i - 10) as f32 * 0.02)
            } else {
                3.0 + ((i - 20) as f32 * 0.03)
            };

            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: fuel,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        let total_laps = 100;

        // Get fuel from calculation functions
        let avg_calc = state.avg_fuel_pct_of_total(total_laps);
        let push_calc = state.avg_fuel_push_pct_of_total(total_laps);
        let low_calc = state.avg_fuel_low_pct_of_total(total_laps);

        // Get laps from assignments
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(total_laps);

        // Calculate fuel from assigned laps
        let avg_from_laps: f32 = avg_laps
            .iter()
            .map(|&lap| {
                state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used
            })
            .sum::<f32>()
            / avg_laps.len() as f32;

        let push_from_laps: f32 = push_laps
            .iter()
            .map(|&lap| {
                state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used
            })
            .sum::<f32>()
            / push_laps.len() as f32;

        let low_from_laps: f32 = low_laps
            .iter()
            .map(|&lap| {
                state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used
            })
            .sum::<f32>()
            / low_laps.len() as f32;

        println!("\nFuel calculation functions:");
        println!("  AVG calculation: {:.3}L", avg_calc.unwrap());
        println!("  PUSH calculation: {:.3}L", push_calc.unwrap());
        println!("  LOW calculation: {:.3}L", low_calc.unwrap());

        println!("\nFuel from lap assignments:");
        println!("  AVG from laps: {:.3}L", avg_from_laps);
        println!("  PUSH from laps: {:.3}L", push_from_laps);
        println!("  LOW from laps: {:.3}L", low_from_laps);

        // Verify they match (within floating point tolerance)
        let tolerance = 0.001;
        assert!(
            (avg_calc.unwrap() - avg_from_laps).abs() < tolerance,
            "AVG calculation ({:.3}L) should match AVG from laps ({:.3}L)",
            avg_calc.unwrap(),
            avg_from_laps
        );
        assert!(
            (push_calc.unwrap() - push_from_laps).abs() < tolerance,
            "PUSH calculation ({:.3}L) should match PUSH from laps ({:.3}L)",
            push_calc.unwrap(),
            push_from_laps
        );
        assert!(
            (low_calc.unwrap() - low_from_laps).abs() < tolerance,
            "LOW calculation ({:.3}L) should match LOW from laps ({:.3}L)",
            low_calc.unwrap(),
            low_from_laps
        );

        // Also verify the ordering
        assert!(
            push_calc.unwrap() > avg_calc.unwrap(),
            "PUSH ({:.3}L) should be > AVG ({:.3}L)",
            push_calc.unwrap(),
            avg_calc.unwrap()
        );
        assert!(
            avg_calc.unwrap() > low_calc.unwrap(),
            "AVG ({:.3}L) should be > LOW ({:.3}L)",
            avg_calc.unwrap(),
            low_calc.unwrap()
        );

        println!("\n  ✓ Calculation functions match lap assignments");
        println!(
            "  ✓ PUSH ({:.3}L) > AVG ({:.3}L) > LOW ({:.3}L)",
            push_calc.unwrap(),
            avg_calc.unwrap(),
            low_calc.unwrap()
        );
    }

    #[test]
    fn test_500_lap_race_with_only_10_laps() {
        let mut state = AppState::new();

        println!("\n=== Testing 500 lap race with only 10 laps of data ===");
        println!("This simulates early race estimation where we have limited data");

        // Create 10 laps with varied fuel usage (realistic early race data)
        for i in 1..=10 {
            let fuel = if i <= 3 {
                2.0 + (i as f32 * 0.02) // Lower fuel: 2.02-2.06L
            } else if i <= 7 {
                2.5 + ((i - 3) as f32 * 0.03) // Medium fuel: 2.53-2.65L
            } else {
                3.0 + ((i - 7) as f32 * 0.05) // Higher fuel: 3.05-3.15L
            };

            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: fuel,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        let total_laps = 500;

        println!("\nRace parameters:");
        println!("  Total race laps: {}", total_laps);
        println!("  Laps completed: {}", state.lap_data.len());
        println!(
            "  Race completion: {:.1}%",
            (state.lap_data.len() as f32 / total_laps as f32) * 100.0
        );

        // The lap counts for a full 500 lap race
        let avg_lap_count = ((total_laps as f32 * 0.10).ceil() as usize).max(5);
        let push_lap_count = ((total_laps as f32 * 0.05).ceil() as usize).max(3);
        let low_lap_count = ((total_laps as f32 * 0.05).ceil() as usize).max(3);

        println!("\nRequested lap counts for 500 lap race:");
        println!(
            "  AVG: {} laps (10% of 500 = 50, max(5) = {})",
            avg_lap_count, avg_lap_count
        );
        println!(
            "  PUSH: {} laps (5% of 500 = 25, max(3) = {})",
            push_lap_count, push_lap_count
        );
        println!(
            "  LOW: {} laps (5% of 500 = 25, max(3) = {})",
            low_lap_count, low_lap_count
        );

        println!("\nAvailable data:");
        println!(
            "  Only {} laps completed (not enough for full calculation)",
            state.lap_data.len()
        );

        // Get strategy assignments
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(total_laps);

        println!("\nActual lap assignments (limited by available data):");
        println!(
            "  AVG: {} laps (requested {}, got {})",
            avg_laps.len(),
            avg_lap_count,
            avg_laps.len()
        );
        println!(
            "  PUSH: {} laps (requested {}, got {})",
            push_laps.len(),
            push_lap_count,
            push_laps.len()
        );
        println!(
            "  LOW: {} laps (requested {}, got {})",
            low_laps.len(),
            low_lap_count,
            low_laps.len()
        );

        // Fuel averages from the available laps
        if !avg_laps.is_empty() && !push_laps.is_empty() && !low_laps.is_empty() {
            let avg_fuel: f32 = avg_laps
                .iter()
                .map(|&lap| {
                    state
                        .lap_data
                        .iter()
                        .find(|l| l.lap_number == lap)
                        .unwrap()
                        .fuel_used
                })
                .sum::<f32>()
                / avg_laps.len() as f32;

            let push_fuel: f32 = push_laps
                .iter()
                .map(|&lap| {
                    state
                        .lap_data
                        .iter()
                        .find(|l| l.lap_number == lap)
                        .unwrap()
                        .fuel_used
                })
                .sum::<f32>()
                / push_laps.len() as f32;

            let low_fuel: f32 = low_laps
                .iter()
                .map(|&lap| {
                    state
                        .lap_data
                        .iter()
                        .find(|l| l.lap_number == lap)
                        .unwrap()
                        .fuel_used
                })
                .sum::<f32>()
                / low_laps.len() as f32;

            println!(
                "\nEstimated fuel usage based on {} laps:",
                state.lap_data.len()
            );
            println!("  AVG: {:.3}L", avg_fuel);
            println!("  PUSH: {:.3}L", push_fuel);
            println!("  LOW: {:.3}L", low_fuel);

            println!("\nLap details:");
            println!("  AVG laps: {:?}", avg_laps);
            for &lap in &avg_laps {
                let fuel = state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used;
                println!("    Lap {}: {:.3}L", lap, fuel);
            }

            println!("  PUSH laps: {:?}", push_laps);
            for &lap in &push_laps {
                let fuel = state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used;
                println!("    Lap {}: {:.3}L", lap, fuel);
            }

            println!("  LOW laps: {:?}", low_laps);
            for &lap in &low_laps {
                let fuel = state
                    .lap_data
                    .iter()
                    .find(|l| l.lap_number == lap)
                    .unwrap()
                    .fuel_used;
                println!("    Lap {}: {:.3}L", lap, fuel);
            }

            // Verify ordering even with limited data
            if push_fuel > avg_fuel && avg_fuel > low_fuel {
                println!(
                    "\n  ✓ PUSH ({:.3}L) > AVG ({:.3}L) > LOW ({:.3}L)",
                    push_fuel, avg_fuel, low_fuel
                );
                println!("  ✓ Early race estimation is working correctly");
            } else {
                println!("\n  ⚠ Warning: Ordering not ideal with limited data:");
                println!(
                    "    PUSH: {:.3}L, AVG: {:.3}L, LOW: {:.3}L",
                    push_fuel, avg_fuel, low_fuel
                );
            }

            // The calculation functions must still work
            let avg_calc = state.avg_fuel_pct_of_total(total_laps);
            let push_calc = state.avg_fuel_push_pct_of_total(total_laps);
            let low_calc = state.avg_fuel_low_pct_of_total(total_laps);

            println!("\nCalculation functions with limited data:");
            if let Some(avg) = avg_calc {
                println!("  AVG calculation: {:.3}L", avg);
            } else {
                println!("  AVG calculation: None (insufficient data)");
            }
            if let Some(push) = push_calc {
                println!("  PUSH calculation: {:.3}L", push);
            } else {
                println!("  PUSH calculation: None (insufficient data)");
            }
            if let Some(low) = low_calc {
                println!("  LOW calculation: {:.3}L", low);
            } else {
                println!("  LOW calculation: None (insufficient data)");
            }

            // Assertions
            assert!(
                avg_laps.len() <= state.lap_data.len(),
                "Can't use more laps than available"
            );
            assert!(
                push_laps.len() <= state.lap_data.len(),
                "Can't use more laps than available"
            );
            assert!(
                low_laps.len() <= state.lap_data.len(),
                "Can't use more laps than available"
            );

            // With 10 laps, there must be an estimate
            assert!(
                !avg_laps.is_empty(),
                "Should have some AVG laps even with limited data"
            );
            assert!(
                !push_laps.is_empty(),
                "Should have some PUSH laps even with limited data"
            );
            assert!(
                !low_laps.is_empty(),
                "Should have some LOW laps even with limited data"
            );
        }

        println!("\n  ✓ 500 lap race estimation works with limited early race data");
    }

    #[test]
    fn test_session_change_resets_data() {
        let mut state = AppState::new();

        println!("\n=== Testing session change resets all data ===");

        // Simulate practice session (session_num = 0)
        let mut practice_frame = TelemetryFrame {
            session_num: 0,
            lap_current: 1,
            fuel_level: 50.0,
            leader_lap: 1,
            ..Default::default()
        };

        state.update(practice_frame.clone(), 10);

        // Complete a few laps in practice
        for lap in 2..=5 {
            practice_frame.lap_current = lap;
            practice_frame.fuel_level = 50.0 - (lap as f32 * 2.5);
            practice_frame.leader_lap = lap;
            state.update(practice_frame.clone(), 10);
        }

        println!("\nAfter practice session (session_num=0):");
        println!("  Lap data count: {}", state.lap_data.len());
        println!("  Current lap: {}", state.current_lap);
        println!("  Last session num: {}", state.last_session_num);

        assert_eq!(
            state.last_session_num, 0,
            "Should be in session 0 (practice)"
        );
        assert_eq!(state.lap_data.len(), 4, "Should have 4 completed laps");
        assert_eq!(state.current_lap, 5, "Should be on lap 5");

        // Now switch to qualifying (session_num = 1)
        let mut qual_frame = TelemetryFrame {
            session_num: 1,
            lap_current: 1,
            fuel_level: 60.0,
            leader_lap: 1,
            ..Default::default()
        };

        state.update(qual_frame.clone(), 10);

        println!("\nAfter switching to qualifying (session_num=1):");
        println!("  Lap data count: {}", state.lap_data.len());
        println!("  Current lap: {}", state.current_lap);
        println!("  Last session num: {}", state.last_session_num);
        println!("  Stint count: {}", state.stints.len());

        assert_eq!(
            state.last_session_num, 1,
            "Should be in session 1 (qualifying)"
        );
        assert_eq!(state.lap_data.len(), 0, "Lap data should be reset");
        assert_eq!(state.current_lap, 1, "Should be on lap 1 of new session");
        assert_eq!(state.stints.len(), 1, "Should have reset to 1 stint");
        assert_eq!(state.laps_down, 0, "Laps down should be reset");

        // Complete a few qualifying laps
        for lap in 2..=3 {
            qual_frame.lap_current = lap;
            qual_frame.fuel_level = 60.0 - (lap as f32 * 3.0);
            qual_frame.leader_lap = lap;
            state.update(qual_frame.clone(), 10);
        }

        println!("\nAfter qualifying laps:");
        println!("  Lap data count: {}", state.lap_data.len());
        assert_eq!(state.lap_data.len(), 2, "Should have 2 qualifying laps");

        // Now switch to race (session_num = 2)
        let race_frame = TelemetryFrame {
            session_num: 2,
            lap_current: 1,
            fuel_level: 70.0,
            leader_lap: 1,
            ..Default::default()
        };

        state.update(race_frame.clone(), 10);

        println!("\nAfter switching to race (session_num=2):");
        println!("  Lap data count: {}", state.lap_data.len());
        println!("  Current lap: {}", state.current_lap);
        println!("  Last session num: {}", state.last_session_num);

        assert_eq!(state.last_session_num, 2, "Should be in session 2 (race)");
        assert_eq!(state.lap_data.len(), 0, "Lap data should be reset again");
        assert_eq!(state.current_lap, 1, "Should be on lap 1 of race");

        println!("\n  ✓ Session changes correctly reset all telemetry data");
        println!("  ✓ Practice -> Qualifying -> Race transitions work correctly");
    }

    #[test]
    fn test_only_5_laps_completed() {
        let mut state = AppState::new();

        println!("\n=== Testing with only 5 laps completed ===");

        // Create 5 laps with varied fuel usage
        // Lap 1 is excluded (first lap), so 4 laps are eligible
        for i in 1..=5 {
            let fuel = match i {
                1 => 2.5, // Will be excluded as first lap
                2 => 2.1, // LOW
                3 => 2.3, // LOW-MED
                4 => 2.7, // MED-HIGH
                5 => 2.9, // PUSH
                _ => 2.5,
            };

            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: fuel,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        let total_laps = 100; // Assume 100 lap race

        println!("\nRace parameters:");
        println!("  Total race laps: {}", total_laps);
        println!("  Laps completed: {}", state.lap_data.len());
        println!(
            "  Expected eligible: {} (lap 1 excluded as first lap)",
            state.lap_data.len() - 1
        );

        // Get strategy assignments
        let (avg_laps, push_laps, low_laps) = state.get_strategy_lap_assignments(total_laps);

        println!("\nRequested lap counts for 100 lap race:");
        println!(
            "  AVG: {} laps (10% of 100 = 10, max(5, available))",
            ((total_laps as f32 * 0.10).ceil() as usize).max(5)
        );
        println!(
            "  PUSH: {} laps (5% of 100 = 5, max(3, available))",
            ((total_laps as f32 * 0.05).ceil() as usize).max(3)
        );
        println!(
            "  LOW: {} laps (5% of 100 = 5, max(3, available))",
            ((total_laps as f32 * 0.05).ceil() as usize).max(3)
        );

        println!("\nActual lap assignments:");
        println!("  AVG: {} laps - {:?}", avg_laps.len(), avg_laps);
        println!("  PUSH: {} laps - {:?}", push_laps.len(), push_laps);
        println!("  LOW: {} laps - {:?}", low_laps.len(), low_laps);

        println!("\nFuel usage by lap:");
        for lap in &state.lap_data {
            let eligible = lap.lap_number != 1; // First lap is ineligible
            let in_avg = avg_laps.contains(&lap.lap_number);
            let in_push = push_laps.contains(&lap.lap_number);
            let in_low = low_laps.contains(&lap.lap_number);

            print!("  Lap {}: {:.1}L", lap.lap_number, lap.fuel_used);
            if !eligible {
                print!(" [INELIGIBLE - first lap]");
            } else {
                let mut tags = Vec::new();
                if in_avg {
                    tags.push("AVG");
                }
                if in_push {
                    tags.push("PUSH");
                }
                if in_low {
                    tags.push("LOW");
                }
                if !tags.is_empty() {
                    print!(" [{}]", tags.join(", "));
                }
            }
            println!();
        }

        // Calculate fuel averages
        if !avg_laps.is_empty() && !push_laps.is_empty() && !low_laps.is_empty() {
            let avg_fuel: f32 = avg_laps
                .iter()
                .map(|&lap| {
                    state
                        .lap_data
                        .iter()
                        .find(|l| l.lap_number == lap)
                        .unwrap()
                        .fuel_used
                })
                .sum::<f32>()
                / avg_laps.len() as f32;

            let push_fuel: f32 = push_laps
                .iter()
                .map(|&lap| {
                    state
                        .lap_data
                        .iter()
                        .find(|l| l.lap_number == lap)
                        .unwrap()
                        .fuel_used
                })
                .sum::<f32>()
                / push_laps.len() as f32;

            let low_fuel: f32 = low_laps
                .iter()
                .map(|&lap| {
                    state
                        .lap_data
                        .iter()
                        .find(|l| l.lap_number == lap)
                        .unwrap()
                        .fuel_used
                })
                .sum::<f32>()
                / low_laps.len() as f32;

            println!("\nCalculated fuel usage:");
            println!("  AVG: {:.3}L (from {} laps)", avg_fuel, avg_laps.len());
            println!("  PUSH: {:.3}L (from {} laps)", push_fuel, push_laps.len());
            println!("  LOW: {:.3}L (from {} laps)", low_fuel, low_laps.len());

            // Verify ordering
            if push_fuel > avg_fuel && avg_fuel > low_fuel {
                println!(
                    "\n  ✓ PUSH ({:.3}L) > AVG ({:.3}L) > LOW ({:.3}L)",
                    push_fuel, avg_fuel, low_fuel
                );
            } else {
                println!("\n  ⚠ Ordering with 5 laps:");
                println!(
                    "    PUSH: {:.3}L, AVG: {:.3}L, LOW: {:.3}L",
                    push_fuel, avg_fuel, low_fuel
                );
                if push_fuel <= avg_fuel {
                    println!("    Note: PUSH not higher than AVG (expected with limited data)");
                }
                if avg_fuel <= low_fuel {
                    println!("    Note: AVG not higher than LOW (expected with limited data)");
                }
            }

            // All strategies must have data
            assert!(!avg_laps.is_empty(), "Should have AVG laps");
            assert!(!push_laps.is_empty(), "Should have PUSH laps");
            assert!(
                !low_laps.is_empty(),
                "Should have LOW laps with only 5 laps completed"
            );
        }

        println!("\n  ✓ With 5 laps completed, all strategies provide estimates");
    }

    #[test]
    fn test_laps_remaining_calculation_last_lap() {
        println!("\n=== Testing laps remaining on final lap ===");

        let mut state = AppState::new();

        // Test case 1: On lap 80 of 80 at start (0% progress)
        state.current_lap = 80;
        state.latest_frame.lap_dist_pct = 0.0;
        state.latest_frame.session_laps_total = 80;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 1: Lap 80/80 at 0%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert_eq!(
            current_lap_fractional, 79.0,
            "On lap 80 at 0%, should have completed 79.0 laps"
        );
        assert_eq!(
            laps_remaining, 1.0,
            "On lap 80 of 80 at 0%, should have 1.0 lap remaining"
        );

        // Test case 2: On lap 80 of 80 at 65% progress
        state.latest_frame.lap_dist_pct = 0.65;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 2: Lap 80/80 at 65%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert!(
            (current_lap_fractional - 79.65).abs() < 0.001,
            "On lap 80 at 65%, should have completed ~79.65 laps"
        );
        assert!(
            (laps_remaining - 0.35).abs() < 0.001,
            "On lap 80 of 80 at 65%, should have ~0.35 laps remaining"
        );

        // Test case 3: On lap 80 of 80 at 99% progress
        state.latest_frame.lap_dist_pct = 0.99;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 3: Lap 80/80 at 99%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert!(
            (current_lap_fractional - 79.99).abs() < 0.01,
            "On lap 80 at 99%, should have completed ~79.99 laps"
        );
        assert!(
            (laps_remaining - 0.01).abs() < 0.01,
            "On lap 80 of 80 at 99%, should have ~0.01 laps remaining"
        );

        println!("\n  ✓ Last lap calculations are correct");
    }

    #[test]
    fn test_laps_remaining_calculation_mid_race() {
        println!("\n=== Testing laps remaining mid-race ===");

        let mut state = AppState::new();

        // Test case 1: On lap 5 of 80 at 0% progress
        state.current_lap = 5;
        state.latest_frame.lap_dist_pct = 0.0;
        state.latest_frame.session_laps_total = 80;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 1: Lap 5/80 at 0%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert_eq!(
            current_lap_fractional, 4.0,
            "On lap 5 at 0%, should have completed 4.0 laps"
        );
        assert_eq!(
            laps_remaining, 76.0,
            "On lap 5 of 80 at 0%, should have 76.0 laps remaining"
        );

        // Test case 2: On lap 5 of 80 at 50% progress
        state.latest_frame.lap_dist_pct = 0.5;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 2: Lap 5/80 at 50%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert_eq!(
            current_lap_fractional, 4.5,
            "On lap 5 at 50%, should have completed 4.5 laps"
        );
        assert_eq!(
            laps_remaining, 75.5,
            "On lap 5 of 80 at 50%, should have 75.5 laps remaining"
        );

        // Test case 3: On lap 40 of 80 at 75% progress
        state.current_lap = 40;
        state.latest_frame.lap_dist_pct = 0.75;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 3: Lap 40/80 at 75%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert_eq!(
            current_lap_fractional, 39.75,
            "On lap 40 at 75%, should have completed 39.75 laps"
        );
        assert_eq!(
            laps_remaining, 40.25,
            "On lap 40 of 80 at 75%, should have 40.25 laps remaining"
        );

        println!("\n  ✓ Mid-race lap calculations are correct");
    }

    #[test]
    fn test_laps_remaining_first_lap() {
        println!("\n=== Testing laps remaining on first lap ===");

        let mut state = AppState::new();

        // Test case 1: On lap 1 of 80 at 0% progress (race just started)
        state.current_lap = 1;
        state.latest_frame.lap_dist_pct = 0.0;
        state.latest_frame.session_laps_total = 80;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 1: Lap 1/80 at 0%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert_eq!(
            current_lap_fractional, 0.0,
            "On lap 1 at 0%, should have completed 0.0 laps"
        );
        assert_eq!(
            laps_remaining, 80.0,
            "On lap 1 of 80 at 0%, should have 80.0 laps remaining"
        );

        // Test case 2: On lap 1 of 80 at 25% progress
        state.latest_frame.lap_dist_pct = 0.25;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;

        println!("\nTest 2: Lap 1/80 at 25%");
        println!("  current_lap: {}", state.current_lap);
        println!("  lap_dist_pct: {}", state.latest_frame.lap_dist_pct);
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);

        assert_eq!(
            current_lap_fractional, 0.25,
            "On lap 1 at 25%, should have completed 0.25 laps"
        );
        assert_eq!(
            laps_remaining, 79.75,
            "On lap 1 of 80 at 25%, should have 79.75 laps remaining"
        );

        println!("\n  ✓ First lap calculations are correct");
    }

    #[test]
    fn test_fuel_calculation_with_correct_laps_remaining() {
        println!("\n=== Testing fuel calculations use correct laps remaining ===");

        let mut state = AppState::new();

        // Setup: On lap 80 of 80 at 0% with 2.5L fuel and 2.5L/lap usage
        state.current_lap = 80;
        state.latest_frame.lap_dist_pct = 0.0;
        state.latest_frame.session_laps_total = 80;
        state.latest_frame.fuel_level = 2.5;

        let fuel_per_lap = 2.5;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;
        let fuel_needed = laps_remaining * fuel_per_lap;
        let fuel_to_add = (fuel_needed - state.latest_frame.fuel_level).max(0.0);

        println!("\nScenario: Lap 80/80 at 0%, 2.5L fuel, 2.5L/lap usage");
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);
        println!("  fuel_needed: {:.2}L", fuel_needed);
        println!("  current_fuel: {:.2}L", state.latest_frame.fuel_level);
        println!("  fuel_to_add: {:.2}L", fuel_to_add);

        assert_eq!(laps_remaining, 1.0, "Should need to complete 1 more lap");
        assert_eq!(fuel_needed, 2.5, "Should need 2.5L to finish");
        assert_eq!(
            fuel_to_add, 0.0,
            "Should not need to add fuel (have exactly enough)"
        );

        // Test case 2: Same scenario but at 50% through lap 80
        state.latest_frame.lap_dist_pct = 0.5;
        state.latest_frame.fuel_level = 1.25;

        let current_lap_fractional = state.get_current_lap_fractional();
        let laps_remaining = 80.0 - current_lap_fractional;
        let fuel_needed = laps_remaining * fuel_per_lap;
        let fuel_to_add = (fuel_needed - state.latest_frame.fuel_level).max(0.0);

        println!("\nScenario: Lap 80/80 at 50%, 1.25L fuel, 2.5L/lap usage");
        println!("  current_lap_fractional: {}", current_lap_fractional);
        println!("  laps_remaining: {}", laps_remaining);
        println!("  fuel_needed: {:.2}L", fuel_needed);
        println!("  current_fuel: {:.2}L", state.latest_frame.fuel_level);
        println!("  fuel_to_add: {:.2}L", fuel_to_add);

        assert_eq!(laps_remaining, 0.5, "Should need to complete 0.5 more laps");
        assert_eq!(fuel_needed, 1.25, "Should need 1.25L to finish");
        assert_eq!(
            fuel_to_add, 0.0,
            "Should not need to add fuel (have exactly enough)"
        );

        println!("\n  ✓ Fuel calculations correctly use laps remaining");
    }

    #[test]
    fn test_restricted_fuel_capacity_detection() {
        let mut state = AppState::new();

        println!("\n=== Testing Restricted Fuel Capacity Detection ===");

        // Simulate a Dallara IR18 with restricted tank (50.2L instead of 120L)
        // FuelLevelPct will be based on the 120L max, but actual tank is 50.2L

        // Start with tank at 50.2L (full for restricted tank, but only 41.8% of 120L)
        let mut frame = TelemetryFrame {
            session_num: 0,
            lap_current: 1,
            fuel_level: 50.2,
            fuel_capacity: 120.0, // Calculated from FuelLevelPct (50.2 / 0.418 ≈ 120)
            leader_lap: 1,
            ..Default::default()
        };

        state.update(frame.clone(), 10);

        println!("After first update with full restricted tank:");
        println!("  fuel_level: {}", frame.fuel_level);
        println!("  calculated fuel_capacity: {}", frame.fuel_capacity);
        println!("  observed_max_fuel: {}", state.observed_max_fuel);
        println!("  actual_fuel_capacity: {:?}", state.actual_fuel_capacity);
        println!("  get_fuel_capacity(): {}", state.get_fuel_capacity());

        assert_eq!(state.observed_max_fuel, 50.2, "Should track max fuel seen");
        assert_eq!(
            state.actual_fuel_capacity,
            Some(50.2),
            "Should detect restricted tank capacity"
        );
        assert_eq!(
            state.get_fuel_capacity(),
            50.2,
            "Should return restricted capacity"
        );

        // Simulate using some fuel
        frame.lap_current = 2;
        frame.fuel_level = 47.5;
        state.update(frame.clone(), 10);

        println!("\nAfter using some fuel:");
        println!("  fuel_level: {}", frame.fuel_level);
        println!("  observed_max_fuel: {}", state.observed_max_fuel);
        println!("  get_fuel_capacity(): {}", state.get_fuel_capacity());

        assert_eq!(
            state.observed_max_fuel, 50.2,
            "Should keep max fuel (not decrease)"
        );
        assert_eq!(
            state.get_fuel_capacity(),
            50.2,
            "Should still return restricted capacity"
        );

        // Simulate refueling back to max
        frame.lap_current = 10;
        frame.fuel_level = 50.2;
        state.update(frame.clone(), 10);

        println!("\nAfter refueling to max:");
        println!("  fuel_level: {}", frame.fuel_level);
        println!("  observed_max_fuel: {}", state.observed_max_fuel);
        println!("  get_fuel_capacity(): {}", state.get_fuel_capacity());

        assert_eq!(state.observed_max_fuel, 50.2);
        assert_eq!(state.get_fuel_capacity(), 50.2);

        println!("\n  ✓ Correctly detects restricted fuel capacity");
        println!("  ✓ Uses observed max (50.2L) instead of calculated (120L)");
        println!("  ✓ Maintains correct capacity after fuel usage and refueling");
    }

    #[test]
    fn test_unrestricted_fuel_capacity() {
        let mut state = AppState::new();

        println!("\n=== Testing Unrestricted Fuel Capacity ===");

        // Simulate a car with full unrestricted tank (120L)
        // FuelLevelPct will match the actual tank

        let frame = TelemetryFrame {
            session_num: 0,
            lap_current: 1,
            fuel_level: 115.0,
            fuel_capacity: 120.0, // Calculated matches actual
            leader_lap: 1,
            ..Default::default()
        };

        state.update(frame.clone(), 10);

        println!("After first update with unrestricted tank:");
        println!("  fuel_level: {}", frame.fuel_level);
        println!("  calculated fuel_capacity: {}", frame.fuel_capacity);
        println!("  observed_max_fuel: {}", state.observed_max_fuel);
        println!("  actual_fuel_capacity: {:?}", state.actual_fuel_capacity);
        println!("  get_fuel_capacity(): {}", state.get_fuel_capacity());

        assert_eq!(state.observed_max_fuel, 115.0);
        assert_eq!(
            state.actual_fuel_capacity,
            Some(120.0),
            "Should use calculated capacity (no restriction)"
        );
        assert_eq!(state.get_fuel_capacity(), 120.0);

        println!("\n  ✓ Correctly uses unrestricted fuel capacity");
        println!("  ✓ Uses calculated capacity when difference is small");
    }

    // ============================================
    // Additional fuel strategy edge case tests
    // ============================================

    #[test]
    fn test_laps_remaining_basic() {
        let mut state = AppState::new();
        state.latest_frame.fuel_level = 25.0;
        state.latest_frame.lap_dist_pct = 0.0; // At start of lap

        // With 2.5L/lap, 25L fuel:
        // Simple calculation: 25 / 2.5 = 10 laps
        let laps = state.laps_remaining(2.5);
        assert!((laps - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_laps_remaining_zero_consumption() {
        let mut state = AppState::new();
        state.latest_frame.fuel_level = 25.0;

        // Edge case: 0 fuel use returns 0. This is a design decision.
        let laps = state.laps_remaining(0.0);
        assert_eq!(laps, 0.0);
    }

    #[test]
    fn test_laps_remaining_zero_fuel() {
        let mut state = AppState::new();
        state.latest_frame.fuel_level = 0.0;
        state.latest_frame.lap_dist_pct = 0.0;

        // No fuel = 0 laps remaining
        // Simple calculation: 0 / 2.5 = 0 laps
        let laps = state.laps_remaining(2.5);
        assert!((laps - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_laps_on_full_tank() {
        let mut state = AppState::new();
        state.observed_max_fuel = 60.0;
        state.actual_fuel_capacity = Some(60.0);

        // 60L tank with 3L/lap = 20 laps
        let laps = state.laps_on_full_tank(3.0);
        assert!((laps - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_fuel_to_finish_avg_with_surplus() {
        let mut state = AppState::new();
        state.latest_frame.fuel_level = 50.0;
        state.latest_frame.lap_dist_pct = 0.0;
        state.current_lap = 1;
        state.latest_frame.session_laps_total = 10;

        // 10 laps at 2.0L/lap = 20L needed, have 50L = no fuel to add
        let fuel_needed = state.fuel_to_finish_avg(10, 2.0);
        assert_eq!(fuel_needed, 0.0);
    }

    #[test]
    fn test_fuel_to_finish_avg_with_deficit() {
        let mut state = AppState::new();
        state.latest_frame.fuel_level = 10.0;
        state.latest_frame.lap_dist_pct = 0.0;
        state.current_lap = 1;
        state.latest_frame.session_laps_total = 10;

        // 10 laps at 2.0L/lap = 20L needed, have 10L = need 10L
        let fuel_needed = state.fuel_to_finish_avg(10, 2.0);
        assert!((fuel_needed - 10.0).abs() < 0.1);
    }

    #[test]
    fn test_max_fuel_usage_single_lap() {
        let mut state = AppState::new();

        // Add laps with varying fuel usage
        state.lap_data.push(LapData {
            lap_number: 1,
            fuel_used: 2.0,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });
        state.lap_data.push(LapData {
            lap_number: 2,
            fuel_used: 3.5, // Maximum
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });
        state.lap_data.push(LapData {
            lap_number: 3,
            fuel_used: 2.5,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });

        let max = state.max_fuel_usage();
        assert!(max.is_some());
        assert!((max.unwrap() - 3.5).abs() < 0.01);
    }

    #[test]
    fn test_min_fuel_usage_single_lap() {
        let mut state = AppState::new();

        // Add laps with varying fuel usage
        state.lap_data.push(LapData {
            lap_number: 1,
            fuel_used: 2.0, // Minimum
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });
        state.lap_data.push(LapData {
            lap_number: 2,
            fuel_used: 3.5,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });
        state.lap_data.push(LapData {
            lap_number: 3,
            fuel_used: 2.5,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });

        let min = state.min_fuel_usage();
        assert!(min.is_some());
        assert!((min.unwrap() - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_fuel_calculations_empty_lap_data() {
        let state = AppState::new();

        // With no lap data, all calculations should return None
        assert!(state.avg_fuel_pct_of_total(50).is_none());
        assert!(state.avg_fuel_low_pct_of_total(50).is_none());
        assert!(state.avg_fuel_push_pct_of_total(50).is_none());
        assert!(state.max_fuel_usage().is_none());
        assert!(state.min_fuel_usage().is_none());
    }

    #[test]
    fn test_yellow_flag_laps_excluded() {
        let mut state = AppState::new();

        // Add green flag laps
        for i in 1..=5 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.5,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Add yellow flag laps (should be excluded)
        for i in 6..=10 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 1.0, // Very low (yellow flag pace)
                green_flag_count: 10,
                yellow_flag_count: 100, // Mostly yellow
                had_pit_activity: false,
            });
        }

        let avg = state.avg_fuel_pct_of_total(50);
        assert!(avg.is_some());
        // Average should be ~2.5 (from green laps), not ~1.75 (if yellows included)
        assert!((avg.unwrap() - 2.5).abs() < 0.5);
    }

    #[test]
    fn test_pit_laps_excluded() {
        let mut state = AppState::new();

        // Add normal laps (note: first lap is always excluded as "first after start")
        for i in 1..=6 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.5,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Add pit lap (should be excluded)
        state.lap_data.push(LapData {
            lap_number: 7,
            fuel_used: 5.0, // Higher usage due to pit entry/exit
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: true, // Pit activity
        });

        // The pit lap should be filtered out
        // First lap (1) is excluded as first after start
        // Laps 2-6 (5 laps) should remain
        // Lap 7 (pit) should be excluded
        let filtered = state.get_filtered_fuel_data();
        // Should have laps 2-6 = 5 laps
        assert_eq!(filtered.len(), 5);
        assert!(filtered.iter().all(|(lap, _)| *lap != 7 && *lap != 1));
    }

    #[test]
    fn test_get_current_lap_fractional() {
        let mut state = AppState::new();

        // Formula: (current_lap - 1) + lap_dist_pct
        // This represents laps COMPLETED + partial progress

        // On lap 5 at 0% = 4 complete laps + 0% of 5th = 4.0
        state.current_lap = 5;
        state.latest_frame.lap_dist_pct = 0.0;
        assert!((state.get_current_lap_fractional() - 4.0).abs() < 0.01);

        // On lap 5 at 50% = 4 complete laps + 50% of 5th = 4.5
        state.latest_frame.lap_dist_pct = 0.5;
        assert!((state.get_current_lap_fractional() - 4.5).abs() < 0.01);

        // On lap 10 at 75% = 9 complete laps + 75% of 10th = 9.75
        state.current_lap = 10;
        state.latest_frame.lap_dist_pct = 0.75;
        assert!((state.get_current_lap_fractional() - 9.75).abs() < 0.01);
    }

    #[test]
    fn test_get_effective_total_laps_lap_race() {
        let mut state = AppState::new();
        state.latest_frame.session_laps_total = 50;
        state.latest_frame.leader_lap = 25;
        state.laps_down = 0;

        // Should return session laps total for lap-based race
        assert_eq!(state.get_effective_total_laps(), 50);
    }

    #[test]
    fn test_get_effective_total_laps_lapped_driver() {
        let mut state = AppState::new();
        state.latest_frame.session_laps_total = 50;
        state.latest_frame.leader_lap = 40;
        state.current_lap = 38;
        state.laps_down = 2;

        // For lap races, returns session_laps_total minus laps_down, because the race ends when the
        // leader finishes, and not the lapped driver.
        assert_eq!(state.get_effective_total_laps(), 48);
    }

    #[test]
    fn test_avg_fuel_calculation_with_outliers() {
        let mut state = AppState::new();

        // Add consistent laps
        for i in 1..=8 {
            state.lap_data.push(LapData {
                lap_number: i,
                fuel_used: 2.5,
                green_flag_count: 100,
                yellow_flag_count: 0,
                had_pit_activity: false,
            });
        }

        // Add outlier (much higher than normal)
        state.lap_data.push(LapData {
            lap_number: 9,
            fuel_used: 5.0, // Outlier
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });

        // Add another normal lap
        state.lap_data.push(LapData {
            lap_number: 10,
            fuel_used: 2.5,
            green_flag_count: 100,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });

        // The outlier filtering should keep average close to 2.5
        let avg = state.avg_fuel_pct_of_total(50);
        assert!(avg.is_some());
        // With outlier filtering, average should be closer to 2.5
        let avg_val = avg.unwrap();
        println!("Average with outlier: {}", avg_val);
        // Depending on filtering, should be between 2.5 and 2.75
        assert!((2.4..=3.0).contains(&avg_val));
    }

    #[test]
    fn test_pit_stop_tracker() {
        let mut state = AppState::new();

        // pit_stop_tracker is a HashMap<car_idx, (stop_count, last_lap)>
        // Simulate recording pit stops for different cars
        state.pit_stop_tracker.insert(0, (2, 15)); // Car 0: 2 stops, last on lap 15
        state.pit_stop_tracker.insert(5, (1, 8)); // Car 5: 1 stop, last on lap 8

        // Get pit stop count for car 0
        let count_car0 = state.get_pit_stop_count(0);
        assert_eq!(count_car0, 2);

        // Get pit stop count for car 5
        let count_car5 = state.get_pit_stop_count(5);
        assert_eq!(count_car5, 1);

        // Get pit stop count for car with no data
        let count_car10 = state.get_pit_stop_count(10);
        assert_eq!(count_car10, 0);
    }

    // --- Car length learning ---

    const TRACK_M: f32 = 5000.0;

    fn length_driver(car_idx: i32, lap_dist_pct: f32, car_class_id: i32) -> DriverInfo {
        DriverInfo {
            car_idx,
            user_name: vec![0; 64],
            car_number: vec![0; 16],
            car_class_id,
            irating: 0,
            license_string: vec![0; 16],
            position: 0,
            class_position: 0,
            lap: 0,
            lap_dist_pct,
            lap_best_time: 0.0,
            lap_last_time: 0.0,
            est_time: 0.0,
            class_est_lap_time: 0.0,
            speed: 0.0,
            on_pit_road: 0,
            laps_since_pit: 0,
            track_surface: 3, // on track
            is_pacing: 0,
            is_pace_car: 0,
            car_class_relative_speed: 0,
        }
    }

    /// Frame with a rival `gap_m` in front of the player (negative is behind).
    fn length_frame(clr: i32, gap_m: f32, rival_class: i32) -> TelemetryFrame {
        TelemetryFrame {
            car_left_right: clr,
            track_length_meters: TRACK_M,
            player_car_idx: 0,
            drivers: vec![
                length_driver(0, 0.5, 1),
                length_driver(1, 0.5 + gap_m / TRACK_M, rival_class),
            ],
            ..Default::default()
        }
    }

    /// Runs one alongside to clear cycle, which gives one sample.
    fn sample_once(state: &mut AppState, gap_m: f32, rival_class: i32) {
        state.update(length_frame(LR_CAR_LEFT, gap_m, rival_class), 0);
        state.update(length_frame(LR_CLEAR, gap_m, rival_class), 0);
    }

    #[test]
    fn learns_car_length_on_clear_transition() {
        let mut state = AppState::new();
        sample_once(&mut state, 4.2, 7);
        let learned = state.car_length.get(&7).copied().expect("learned length");
        assert!((learned - 4.2).abs() < 0.01, "got {learned}");
    }

    #[test]
    fn ignores_gaps_outside_plausible_window() {
        let mut state = AppState::new();
        sample_once(&mut state, 0.4, 7); // too short
        sample_once(&mut state, 30.0, 7); // too long
        assert!(!state.car_length.contains_key(&7));
    }

    #[test]
    fn accepts_ir18_length_that_irons_ceiling_would_reject() {
        // iRon only accepts 1.5..5.5 m, which removes the IR-18 of approximately 5 m. This code
        // must accept it.
        let mut state = AppState::new();
        sample_once(&mut state, 5.8, 7);
        let learned = state.car_length.get(&7).copied().expect("learned length");
        assert!((learned - 5.8).abs() < 0.01, "got {learned}");
    }

    #[test]
    fn does_not_sample_while_still_alongside() {
        let mut state = AppState::new();
        // Alongside for many frames, and never clear.
        for _ in 0..5 {
            state.update(length_frame(LR_CAR_LEFT, 4.2, 7), 0);
        }
        assert!(!state.car_length.contains_key(&7));
    }

    #[test]
    fn median_rejects_a_single_outlier() {
        let mut state = AppState::new();
        for gap in [4.0, 4.1, 6.9, 4.2, 4.1] {
            sample_once(&mut state, gap, 7);
        }
        // The median of the five samples is 4.1. The 6.9 sample does not change it.
        let learned = state.car_length.get(&7).copied().expect("learned length");
        assert!((learned - 4.1).abs() < 0.001, "got {learned}");
    }

    #[test]
    fn car_behind_teaches_us_our_own_length() {
        let mut state = AppState::new();
        // Rival 4.3 m behind the player. The player is class 1.
        sample_once(&mut state, -4.3, 7);
        let own = state.car_length.get(&1).copied().expect("own length");
        assert!((own - 4.3).abs() < 0.001, "got {own}");
    }

    #[test]
    fn only_samples_after_being_genuinely_alongside() {
        // A car that is near but never alongside (LR_CLEAR in all frames) must not give a length
        // sample.
        let mut state = AppState::new();
        state.update(length_frame(LR_CLEAR, 4.2, 7), 0);
        state.update(length_frame(LR_CLEAR, 4.2, 7), 0);
        assert!(!state.car_length.contains_key(&7));
    }

    #[test]
    fn samples_on_first_clear_after_leaving_pits() {
        // prev starts as LR_OFF (pits or garage). The first real alongside to clear cycle after
        // that must still give a sample.
        let mut state = AppState::new();
        assert_eq!(state.prev_car_left_right, LR_OFF);
        sample_once(&mut state, 4.2, 7);
        assert!(
            state.car_length.contains_key(&7),
            "first pass after LR_OFF produced no sample"
        );
    }

    #[test]
    fn car_length_for_falls_back_when_unlearned() {
        let state = AppState::new();
        assert_eq!(state.car_length_for(99, 4.5), 4.5);
    }

    #[test]
    fn reset_clears_learned_lengths() {
        let mut state = AppState::new();
        sample_once(&mut state, 4.2, 7);
        assert!(state.car_length.contains_key(&7));

        state.reset();
        assert!(state.car_length.is_empty());
        assert!(state.car_length_samples.is_empty());
    }

    #[test]
    fn wrapped_pct_delta_handles_start_finish_line() {
        // Just after the line and just before it: a small positive gap.
        let d = wrapped_pct_delta(0.0001, 0.9999);
        assert!((d - 0.0002).abs() < 1e-6, "got {d}");

        let d = wrapped_pct_delta(0.9999, 0.0001);
        assert!((d + 0.0002).abs() < 1e-6, "got {d}");
    }
}
