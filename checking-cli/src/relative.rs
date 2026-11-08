//! Gap calculation for the relative overlay.
//!
//! iRacing gives `CarIdxEstTime`: the estimated time from the start/finish line
//! to the car's track position. The scale of this value is the estimated lap
//! time of the car class (`CarClassEstLapTime`). Thus, when two cars are on
//! different sides of the line, the gap must be corrected by one class
//! estimated lap time, and not by a lap time that a driver set.

use checking_protocol::DriverInfo;

/// Lap time in seconds that the gap uses when no other lap time is known.
const FALLBACK_LAP_TIME_S: f32 = 90.0;

/// The position of the start/finish line between the player and a driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineCrossing {
    /// The line is not between the two cars.
    None,
    /// The driver crossed the line and the player did not. The driver is in
    /// front of the player.
    DriverCrossed,
    /// The player crossed the line and the driver did not. The driver is
    /// behind the player.
    PlayerCrossed,
}

/// Gap from the player to one driver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelativeGap {
    /// Track distance in laps, in the range -0.5 to +0.5. A positive value is
    /// in front of the player.
    pub track_delta: f32,
    /// Time gap in seconds. A positive value is in front of the player.
    pub time_delta: f32,
    /// Real lap difference, for the lapped and lapping indicator.
    pub lap_diff: i32,
}

/// Calculates the gap from `player` to `driver`.
pub fn relative_gap(player: &DriverInfo, driver: &DriverInfo) -> RelativeGap {
    let raw_track_delta = driver.lap_dist_pct - player.lap_dist_pct;

    // When the track positions are more than half a lap apart, the line is
    // between the two cars.
    let crossing = if raw_track_delta < -0.5 {
        LineCrossing::DriverCrossed
    } else if raw_track_delta > 0.5 {
        LineCrossing::PlayerCrossed
    } else {
        LineCrossing::None
    };

    let (track_delta, lap_diff) = match crossing {
        LineCrossing::None => (raw_track_delta, driver.lap - player.lap),
        LineCrossing::DriverCrossed => (raw_track_delta + 1.0, driver.lap - player.lap - 1),
        LineCrossing::PlayerCrossed => (raw_track_delta - 1.0, driver.lap - player.lap + 1),
    };

    let lap_time = player_reference_lap_time(player);
    let time_delta = if driver.est_time > 0.0 && player.est_time > 0.0 {
        let est_time_delta = driver_est_time_in_player_scale(player, driver) - player.est_time;
        match crossing {
            LineCrossing::None => est_time_delta,
            LineCrossing::DriverCrossed => est_time_delta + lap_time,
            LineCrossing::PlayerCrossed => est_time_delta - lap_time,
        }
    } else {
        track_delta * lap_time
    };

    RelativeGap {
        track_delta,
        time_delta,
        lap_diff,
    }
}

/// Returns the lap time that agrees with the scale of the player's `est_time`.
///
/// The class estimated lap time is the correct value. The other values are
/// for data that does not have it.
fn player_reference_lap_time(player: &DriverInfo) -> f32 {
    [
        player.class_est_lap_time,
        player.lap_last_time,
        player.lap_best_time,
    ]
    .into_iter()
    .find(|&time| time > 0.0)
    .unwrap_or(FALLBACK_LAP_TIME_S)
}

/// Converts the driver's `est_time` to the scale of the player's car class.
///
/// A car in a faster class has a smaller `est_time` at the same track
/// position. Without this conversion, the gap to a car in a different class
/// includes the difference between the two class lap times.
fn driver_est_time_in_player_scale(player: &DriverInfo, driver: &DriverInfo) -> f32 {
    if player.class_est_lap_time > 0.0 && driver.class_est_lap_time > 0.0 {
        driver.est_time * player.class_est_lap_time / driver.class_est_lap_time
    } else {
        driver.est_time
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const CLASS_LAP_TIME_S: f32 = 120.0;
    const TOLERANCE_S: f32 = 1e-3;

    /// Makes a car that moves at a constant speed, thus `est_time` is linear
    /// with `lap_dist_pct`.
    fn car(car_idx: i32, lap: i32, lap_dist_pct: f32, class_est_lap_time: f32) -> DriverInfo {
        let mut driver = DriverInfo::new(car_idx, "", "", "", 0, 0, lap, lap_dist_pct);
        driver.class_est_lap_time = class_est_lap_time;
        driver.est_time = lap_dist_pct * class_est_lap_time;
        driver
    }

    #[test]
    fn same_side_of_line_uses_est_time_difference() {
        let player = car(0, 3, 0.40, CLASS_LAP_TIME_S);
        let driver = car(1, 3, 0.45, CLASS_LAP_TIME_S);

        let gap = relative_gap(&player, &driver);

        assert!((gap.time_delta - 6.0).abs() < TOLERANCE_S);
        assert!((gap.track_delta - 0.05).abs() < 1e-6);
        assert_eq!(gap.lap_diff, 0);
    }

    // This is the start of the race. The player crossed the line and has no
    // lap times. The driver did not cross the line and is 1% of a lap behind.
    #[test]
    fn player_crossed_line_on_first_lap_uses_class_lap_time() {
        let player = car(0, 1, 0.005, CLASS_LAP_TIME_S);
        let driver = car(1, 0, 0.995, CLASS_LAP_TIME_S);

        let gap = relative_gap(&player, &driver);

        assert!((gap.time_delta - -1.2).abs() < TOLERANCE_S, "{gap:?}");
        assert!((gap.track_delta - -0.01).abs() < 1e-5);
        assert_eq!(gap.lap_diff, 0);
    }

    #[test]
    fn driver_crossed_line_uses_class_lap_time_and_not_last_lap() {
        let mut player = car(0, 4, 0.995, CLASS_LAP_TIME_S);
        player.lap_last_time = 125.0;
        let driver = car(1, 5, 0.005, CLASS_LAP_TIME_S);

        let gap = relative_gap(&player, &driver);

        assert!((gap.time_delta - 1.2).abs() < TOLERANCE_S, "{gap:?}");
        assert!((gap.track_delta - 0.01).abs() < 1e-5);
        assert_eq!(gap.lap_diff, 0);
    }

    #[test]
    fn lapped_car_across_line_keeps_lap_difference() {
        let player = car(0, 6, 0.005, CLASS_LAP_TIME_S);
        let driver = car(1, 4, 0.995, CLASS_LAP_TIME_S);

        let gap = relative_gap(&player, &driver);

        assert_eq!(gap.lap_diff, -1);
    }

    #[test]
    fn other_class_est_time_is_scaled_to_player_class() {
        let player = car(0, 2, 0.50, CLASS_LAP_TIME_S);
        let faster_class_driver = car(1, 2, 0.50, 100.0);

        let gap = relative_gap(&player, &faster_class_driver);

        assert!(gap.time_delta.abs() < TOLERANCE_S, "{gap:?}");
    }

    #[test]
    fn missing_class_lap_time_uses_last_lap_time() {
        let mut player = car(0, 1, 0.005, 0.0);
        player.est_time = 0.6;
        player.lap_last_time = 120.0;
        let mut driver = car(1, 0, 0.995, 0.0);
        driver.est_time = 119.4;

        let gap = relative_gap(&player, &driver);

        assert!((gap.time_delta - -1.2).abs() < TOLERANCE_S, "{gap:?}");
    }

    #[test]
    fn missing_est_time_uses_track_delta() {
        let mut player = car(0, 1, 0.005, CLASS_LAP_TIME_S);
        player.est_time = 0.0;
        let driver = car(1, 0, 0.995, CLASS_LAP_TIME_S);

        let gap = relative_gap(&player, &driver);

        assert!((gap.time_delta - -1.2).abs() < TOLERANCE_S, "{gap:?}");
    }

    #[test]
    fn no_lap_times_use_fallback_lap_time() {
        let mut player = car(0, 1, 0.005, 0.0);
        player.est_time = 0.0;
        let driver = car(1, 0, 0.995, 0.0);

        let gap = relative_gap(&player, &driver);

        assert!((gap.time_delta - -0.01 * FALLBACK_LAP_TIME_S).abs() < TOLERANCE_S);
    }

    proptest! {
        // For two cars in the same class at a constant speed, the time gap
        // agrees with the track gap, on each side of the line.
        #[test]
        fn time_gap_agrees_with_track_gap(
            player_pct in 0.001f32..0.999,
            driver_pct in 0.001f32..0.999,
            lap in 1i32..50,
        ) {
            let player = car(0, lap, player_pct, CLASS_LAP_TIME_S);
            let driver = car(1, lap, driver_pct, CLASS_LAP_TIME_S);

            let gap = relative_gap(&player, &driver);

            prop_assert!((-0.5..=0.5).contains(&gap.track_delta));
            prop_assert!((gap.time_delta - gap.track_delta * CLASS_LAP_TIME_S).abs() < 0.01);
        }
    }
}
