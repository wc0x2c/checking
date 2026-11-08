//! Synthetic race scenarios for the `strategy` unit tests.
//!
//! Makes an `AppState` that looks like a real race. It has `lap_data`, so that
//! the LOW, AVG, PUSH and MAX estimates have numbers. It also has a
//! `latest_frame` with the fuel and lap counts, and capacity fields that agree.

use checking_fuel::{AppState, LapData};
use checking_protocol::TelemetryFrame;

/// A prepared race situation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Race at mid-distance with a large margin. It needs one stop.
    MidRace,
    /// Near the end with a small margin. The car finishes with almost no fuel,
    /// or it needs a short stop.
    Marginal,
    /// Long race. One tank is not sufficient for the distance, so the car needs
    /// more than one stop.
    LongRun,
    /// Early in the race with almost no lap history. The estimates are
    /// `unknown`.
    NoData,
    /// Timed race. The lap count is an estimate from the clock and not a fixed
    /// number, so the strategy must be different.
    Timed,
    /// Caution period with yellow laps in the history. The fuel use is much
    /// lower than the green flag rate, and a stop here costs little time.
    Yellow,
    /// A stop becomes necessary in one more lap. This tests the urgent pit
    /// window code.
    Urgent,
}

impl Scenario {
    pub fn name(&self) -> &'static str {
        match self {
            Self::MidRace => "midrace",
            Self::Marginal => "marginal",
            Self::LongRun => "longrun",
            Self::NoData => "nodata",
            Self::Timed => "timed",
            Self::Yellow => "yellow",
            Self::Urgent => "urgent",
        }
    }

    pub fn build(&self) -> AppState {
        match self {
            Self::MidRace => build_state(RaceSetup {
                total_laps: 40,
                current_lap: 12,
                lap_dist_pct: 0.35,
                fuel_level: 42.0,
                capacity: 60.0,
                leader_lap: 12,
                // Approximately 2.5 to 2.7 L/lap with some variation, so that
                // LOW, AVG and PUSH are different.
                fuel_used: vec![2.52, 2.61, 2.48, 2.70, 2.55, 2.64, 2.58, 2.66, 2.50, 2.62],
            }),
            Self::Marginal => build_state(RaceSetup {
                total_laps: 25,
                current_lap: 21,
                lap_dist_pct: 0.80,
                fuel_level: 9.5,
                capacity: 60.0,
                leader_lap: 21,
                fuel_used: vec![2.55, 2.68, 2.61, 2.74, 2.59, 2.71, 2.63, 2.66],
            }),
            Self::LongRun => build_state(RaceSetup {
                total_laps: 120,
                current_lap: 8,
                lap_dist_pct: 0.10,
                fuel_level: 55.0,
                capacity: 60.0,
                leader_lap: 9,
                fuel_used: vec![2.90, 3.05, 2.96, 3.12, 2.99, 3.08],
            }),
            Self::NoData => build_state(RaceSetup {
                total_laps: 30,
                current_lap: 1,
                lap_dist_pct: 0.20,
                fuel_level: 58.0,
                capacity: 60.0,
                leader_lap: 1,
                fuel_used: vec![],
            }),
            // 45 minute race, with approximately 28 minutes done. `total_laps`
            // is the value that iRacing uses for a timed session. The lap count
            // comes from `session_laps_remain_ex`.
            Self::Timed => {
                let mut s = build_state(RaceSetup {
                    total_laps: TIMED_SESSION_LAPS,
                    current_lap: 18,
                    lap_dist_pct: 0.45,
                    fuel_level: 24.0,
                    capacity: 60.0,
                    leader_lap: 18,
                    fuel_used: vec![2.44, 2.51, 2.47, 2.58, 2.49, 2.55, 2.46, 2.53],
                });
                s.latest_frame.session_laps_remain_ex = 12;
                s.latest_frame.session_time_remain = 17.0 * 60.0;
                s.latest_frame.session_time = 28.0 * 60.0;
                s
            }
            // Caution period: the last laps used much less fuel, and the flag
            // state shows that a stop now costs little time.
            Self::Yellow => {
                let mut s = build_state(RaceSetup {
                    total_laps: 50,
                    current_lap: 27,
                    lap_dist_pct: 0.60,
                    fuel_level: 31.0,
                    capacity: 60.0,
                    leader_lap: 27,
                    fuel_used: vec![2.71, 2.66, 2.74, 2.69, 2.63, 2.70, 1.12, 1.08],
                });
                // Mark the last two laps as yellow flag laps. This agrees with
                // the low fuel use above.
                for lap in s.lap_data.iter_mut().rev().take(2) {
                    lap.yellow_flag_count = 55;
                    lap.green_flag_count = 5;
                }
                s.is_yellow_flag = true;
                s.latest_frame.session_flags = YELLOW_FLAG;
                s
            }
            // Fuel for approximately one more lap, and many laps to go. The car
            // must stop, and it cannot wait.
            Self::Urgent => build_state(RaceSetup {
                total_laps: 60,
                current_lap: 34,
                lap_dist_pct: 0.30,
                fuel_level: 3.1,
                capacity: 60.0,
                leader_lap: 34,
                fuel_used: vec![2.61, 2.70, 2.65, 2.58, 2.72, 2.63, 2.67, 2.60],
            }),
        }
    }
}

/// The lap total that iRacing reports for a session that ends on the clock.
const TIMED_SESSION_LAPS: i32 = 32767;

/// `irsdk_Flags::yellow`, one of the bits in the `checking-fuel` caution mask.
const YELLOW_FLAG: u32 = 0x0008;

struct RaceSetup {
    total_laps: i32,
    current_lap: i32,
    lap_dist_pct: f32,
    fuel_level: f32,
    capacity: f32,
    leader_lap: i32,
    fuel_used: Vec<f32>,
}

fn build_state(s: RaceSetup) -> AppState {
    let mut state = AppState::new();

    state.latest_frame = TelemetryFrame {
        lap_current: s.current_lap,
        lap_dist_pct: s.lap_dist_pct,
        leader_lap: s.leader_lap,
        fuel_level: s.fuel_level,
        fuel_capacity: s.capacity,
        fuel_max_ltr: s.capacity,
        fuel_level_pct: if s.capacity > 0.0 {
            s.fuel_level / s.capacity
        } else {
            0.0
        },
        session_laps_total: s.total_laps,
        session_laps_remain_ex: (s.total_laps - s.current_lap).max(0),
        position: 4,
        class_position: 4,
        // Green flag, so that no lap is a caution lap.
        session_flags: 0x0004,
        ..Default::default()
    };
    state.connected = true;
    state.current_lap = s.current_lap;
    state.laps_down = (s.leader_lap - s.current_lap).max(0);
    state.lap_start_fuel = s.fuel_level;
    state.last_fuel_level = s.fuel_level;

    // Usually the app detects the capacity from the telemetry. Set it here, so
    // that `get_fuel_capacity()` returns the correct value.
    state.observed_max_fuel = s.capacity;
    state.actual_fuel_capacity = Some(s.capacity);

    // Add the history of completed laps, so that the fuel estimators have data.
    let first_lap = (s.current_lap - s.fuel_used.len() as i32).max(1);
    for (i, used) in s.fuel_used.iter().enumerate() {
        state.lap_data.push(LapData {
            lap_number: first_lap + i as i32,
            fuel_used: *used,
            green_flag_count: 60,
            yellow_flag_count: 0,
            had_pit_activity: false,
        });
    }

    state
}
