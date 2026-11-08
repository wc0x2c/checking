//! Radar geometry. Uses the iRacing CarLeftRight flag and the lap distance of
//! each car to put a marker on each side, with the length of the car.
//!
//! iRacing decides if a car is alongside, because it knows the real car widths.
//! This module finds only the longitudinal values: where the car is alongside,
//! and the car length.

use checking_fuel::{
    AppState, LR_2_CARS_LEFT, LR_2_CARS_RIGHT, LR_CAR_LEFT, LR_CAR_LEFT_RIGHT, LR_CAR_RIGHT,
    wrapped_pct_delta,
};

/// TrkLoc value for a car on the track surface.
const TRK_LOC_ON_TRACK: i32 = 3;

/// A car alongside on one side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadarTarget {
    /// Signed longitudinal offset in metres. A positive value is in front of
    /// the player.
    pub offset_m: f32,
    /// Car length in metres. This is the learned length if it is known, else
    /// the fallback length from the config.
    pub length_m: f32,
    /// Number of cars that iRacing reports on this side (1 or 2).
    pub count: usize,
}

/// Finds the left and right radar targets for the current frame.
///
/// Returns `(left, right)`. `None` on a side means that no car is alongside on
/// that side.
pub fn analyze_radar(
    state: &AppState,
    fallback_car_length_m: f32,
) -> (Option<RadarTarget>, Option<RadarTarget>) {
    let frame = &state.latest_frame;
    let clr = frame.car_left_right;

    let has_left = clr == LR_CAR_LEFT || clr == LR_CAR_LEFT_RIGHT || clr == LR_2_CARS_LEFT;
    let has_right = clr == LR_CAR_RIGHT || clr == LR_CAR_LEFT_RIGHT || clr == LR_2_CARS_RIGHT;

    if !has_left && !has_right {
        return (None, None);
    }

    let track_length = frame.track_length_meters;
    let Some(player) = frame
        .drivers
        .iter()
        .find(|d| d.car_idx == frame.player_car_idx)
    else {
        return (None, None);
    };

    // Candidate cars, nearest first. iRacing reports that a car is alongside,
    // but not which car. Thus sort by the absolute longitudinal distance and
    // take the nearest cars.
    let mut candidates: Vec<(f32, i32)> = Vec::new();
    if track_length > 0.0 {
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
            candidates.push((delta_m, driver.car_class_id));
        }
        candidates.sort_by(|a, b| {
            a.0.abs()
                .partial_cmp(&b.0.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // When there are cars on both sides, the two sides must show different
    // cars. If not, the two bars are the same. Take the two nearest cars and
    // give one to each side.
    let mut take = candidates.into_iter();
    let build = |slot: Option<(f32, i32)>, count: usize| {
        // A marker in the center keeps the indicator visible when the code
        // cannot identify the car but iRacing reports one.
        let (offset_m, class_id) = slot.unwrap_or((0.0, player.car_class_id));
        RadarTarget {
            offset_m,
            length_m: state.car_length_for(class_id, fallback_car_length_m),
            count,
        }
    };

    let left_count = if clr == LR_2_CARS_LEFT { 2 } else { 1 };
    let right_count = if clr == LR_2_CARS_RIGHT { 2 } else { 1 };

    match (has_left, has_right) {
        (true, true) => {
            let first = take.next();
            let second = take.next();
            // The car in front can go to either side. Keep the two cars
            // different.
            (
                Some(build(first, left_count)),
                Some(build(second.or(first), right_count)),
            )
        }
        (true, false) => (Some(build(take.next(), left_count)), None),
        (false, true) => (None, Some(build(take.next(), right_count))),
        (false, false) => (None, None),
    }
}

/// Returns the position of the bar for a target in a strip of `height_px`, as
/// `(top_px, bottom_px)` from the top of the strip.
///
/// The strip goes from `range_m` in front (top) to `range_m` behind (bottom).
/// The car is from `offset_m - length_m / 2` to `offset_m + length_m / 2`. The
/// same linear metres to pixels transform maps both edges, and the result is
/// clipped to the strip. Thus a car that is partly in range shows partly, and
/// it moves off the edge when it leaves.
///
/// Returns `None` when the car is fully outside the strip.
pub fn bar_bounds_px(target: &RadarTarget, range_m: f32, height_px: f32) -> Option<(f32, f32)> {
    if range_m <= 0.0 || height_px <= 0.0 {
        return None;
    }

    // +range_m maps to the top (0), and -range_m maps to the bottom
    // (height_px).
    let to_px = |m: f32| (range_m - m) / (2.0 * range_m) * height_px;

    let half = target.length_m.max(0.0) / 2.0;
    let top = to_px(target.offset_m + half);
    let bottom = to_px(target.offset_m - half);

    // Fully outside the strip: there is nothing to draw.
    if bottom <= 0.0 || top >= height_px {
        return None;
    }

    let clipped_top = top.max(0.0);
    let clipped_bottom = bottom.min(height_px);

    // Show a line of minimum height for a car that is just in range, so that it
    // does not flicker at widths of less than one pixel.
    let visible = (clipped_bottom - clipped_top).max(2.0).min(height_px);
    let anchored_top = clipped_top.min(height_px - visible);
    Some((anchored_top, anchored_top + visible))
}

#[cfg(test)]
mod tests {
    use super::*;
    use checking_protocol::{DriverInfo, TelemetryFrame};

    fn driver(car_idx: i32, lap_dist_pct: f32, car_class_id: i32) -> DriverInfo {
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
            track_surface: TRK_LOC_ON_TRACK,
            is_pacing: 0,
            is_pace_car: 0,
            car_class_relative_speed: 0,
        }
    }

    fn state_with(clr: i32, drivers: Vec<DriverInfo>) -> AppState {
        let mut state = AppState::new();
        state.latest_frame = TelemetryFrame {
            car_left_right: clr,
            track_length_meters: 5000.0,
            player_car_idx: 0,
            drivers,
            ..Default::default()
        };
        state
    }

    #[test]
    fn reports_nothing_when_clear() {
        let state = state_with(checking_fuel::LR_CLEAR, vec![driver(0, 0.5, 1)]);
        assert_eq!(analyze_radar(&state, 4.5), (None, None));
    }

    #[test]
    fn reports_left_target_with_fallback_length() {
        // Rival 2 m in front of the player on a 5000 m track.
        let state = state_with(
            LR_CAR_LEFT,
            vec![driver(0, 0.5, 1), driver(1, 0.5 + 2.0 / 5000.0, 1)],
        );
        let (left, right) = analyze_radar(&state, 4.5);
        assert!(right.is_none());
        let left = left.expect("left target");
        assert!((left.offset_m - 2.0).abs() < 0.01, "got {}", left.offset_m);
        assert_eq!(left.length_m, 4.5);
        assert_eq!(left.count, 1);
    }

    #[test]
    fn two_cars_left_reports_count_two() {
        let state = state_with(
            LR_2_CARS_LEFT,
            vec![driver(0, 0.5, 1), driver(1, 0.5002, 1)],
        );
        let (left, _) = analyze_radar(&state, 4.5);
        assert_eq!(left.expect("left target").count, 2);
    }

    #[test]
    fn three_wide_reports_both_sides() {
        let state = state_with(
            LR_CAR_LEFT_RIGHT,
            vec![driver(0, 0.5, 1), driver(1, 0.5002, 1)],
        );
        let (left, right) = analyze_radar(&state, 4.5);
        assert!(left.is_some() && right.is_some());
    }

    #[test]
    fn three_wide_uses_two_different_cars() {
        // Two rivals alongside: one 1 m in front, one 3 m behind. Each side
        // must show a different car. If not, the two bars are the same.
        let state = state_with(
            LR_CAR_LEFT_RIGHT,
            vec![
                driver(0, 0.5, 1),
                driver(1, 0.5 + 1.0 / 5000.0, 1),
                driver(2, 0.5 - 3.0 / 5000.0, 1),
            ],
        );
        let (left, right) = analyze_radar(&state, 4.5);
        let left = left.expect("left target");
        let right = right.expect("right target");
        assert!(
            (left.offset_m - right.offset_m).abs() > 0.5,
            "both sides tracked the same car: left={} right={}",
            left.offset_m,
            right.offset_m
        );
    }

    #[test]
    fn uses_learned_length_when_available() {
        let mut state = state_with(LR_CAR_LEFT, vec![driver(0, 0.5, 1), driver(1, 0.5002, 7)]);
        state.car_length.insert(7, 5.1);
        let (left, _) = analyze_radar(&state, 4.5);
        assert_eq!(left.expect("left target").length_m, 5.1);
    }

    #[test]
    fn offset_wraps_across_start_finish() {
        // The player is just before the line, and the rival is just after it.
        let state = state_with(
            LR_CAR_LEFT,
            vec![driver(0, 0.9999, 1), driver(1, 0.0001, 1)],
        );
        let (left, _) = analyze_radar(&state, 4.5);
        let offset = left.expect("left target").offset_m;
        assert!(offset > 0.0 && offset < 10.0, "got {offset}");
    }

    // --- Bar geometry ---

    const H: f32 = 120.0;

    fn target(offset_m: f32, length_m: f32) -> RadarTarget {
        RadarTarget {
            offset_m,
            length_m,
            count: 1,
        }
    }

    #[test]
    fn alongside_car_sits_centred() {
        let (top, bottom) = bar_bounds_px(&target(0.0, 4.5), 8.0, H).expect("visible");
        let center = (top + bottom) / 2.0;
        assert!((center - H / 2.0).abs() < 0.01, "got {center}");
    }

    #[test]
    fn car_ahead_renders_above_centre() {
        let (top, bottom) = bar_bounds_px(&target(4.0, 4.5), 8.0, H).expect("visible");
        assert!((top + bottom) / 2.0 < H / 2.0);
    }

    #[test]
    fn car_behind_renders_below_centre() {
        let (top, bottom) = bar_bounds_px(&target(-4.0, 4.5), 8.0, H).expect("visible");
        assert!((top + bottom) / 2.0 > H / 2.0);
    }

    #[test]
    fn bar_height_matches_car_length_to_scale() {
        // A 120 px strip for ±8 m is 7.5 px/m, so a 5 m car must be 37.5 px.
        let (top, bottom) = bar_bounds_px(&target(0.0, 5.0), 8.0, H).expect("visible");
        assert!((bottom - top - 37.5).abs() < 0.01, "got {}", bottom - top);
    }

    #[test]
    fn longer_car_gets_a_taller_bar() {
        // This is the purpose of the learned lengths. An IR-18 of approximately
        // 5 m must show longer than a short car at the same position.
        let (st, sb) = bar_bounds_px(&target(0.0, 4.0), 8.0, H).expect("visible");
        let (it, ib) = bar_bounds_px(&target(0.0, 5.0), 8.0, H).expect("visible");
        assert!((ib - it) > (sb - st), "IR-18 bar was not taller");
    }

    #[test]
    fn bar_keeps_moving_across_the_whole_range() {
        // Regression test. When the code clamped the bar fully in the strip,
        // the bar stopped at approximately ±6 m on a ±8 m range. The last 2 m
        // did not move the bar.
        let mut previous = f32::NEG_INFINITY;
        for step in 0..=32 {
            let offset = 8.0 - (step as f32) * 0.5; // +8 m to -8 m
            let (top, bottom) = bar_bounds_px(&target(offset, 5.0), 8.0, H).expect("visible");
            let center = (top + bottom) / 2.0;
            assert!(
                center > previous,
                "bar stopped moving at offset {offset}: centre {center} did not advance"
            );
            previous = center;
        }
    }

    #[test]
    fn car_partly_in_range_renders_partly() {
        // Half of a 5 m car is past the +8 m edge. The bar is shorter than a
        // fully visible bar, and it touches the top.
        let (top, bottom) = bar_bounds_px(&target(9.0, 5.0), 8.0, H).expect("visible");
        assert!(top < 0.01, "should be flush to the top edge, got {top}");
        assert!(bottom < 37.5, "should be clipped shorter, got {bottom}");
    }

    #[test]
    fn car_fully_out_of_range_is_hidden() {
        assert!(bar_bounds_px(&target(20.0, 5.0), 8.0, H).is_none());
        assert!(bar_bounds_px(&target(-20.0, 5.0), 8.0, H).is_none());
    }

    #[test]
    fn bar_stays_inside_the_strip_at_extremes() {
        for offset in [-10.0, -8.0, -4.0, 0.0, 4.0, 8.0, 10.0] {
            let (top, bottom) = bar_bounds_px(&target(offset, 5.0), 8.0, H).expect("visible");
            assert!(top >= -0.01, "top {top} escaped at offset {offset}");
            assert!(
                bottom <= H + 0.01,
                "bottom {bottom} escaped at offset {offset}"
            );
            assert!(bottom > top, "degenerate bar at offset {offset}");
        }
    }

    #[test]
    fn bar_never_exceeds_strip_height() {
        // A very long car on a very short range must stay in the strip.
        let (top, bottom) = bar_bounds_px(&target(0.0, 7.0), 1.0, H).expect("visible");
        assert!(top >= -0.01 && bottom <= H + 0.01, "{top}..{bottom}");
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert!(bar_bounds_px(&target(0.0, 5.0), 0.0, H).is_none());
        assert!(bar_bounds_px(&target(0.0, 5.0), 8.0, 0.0).is_none());
        // A car with zero length still shows a line of minimum height.
        assert!(bar_bounds_px(&target(0.0, 0.0), 8.0, H).is_some());
    }

    #[test]
    fn ignores_pace_car_and_cars_off_track() {
        let mut pace = driver(1, 0.5001, 1);
        pace.is_pace_car = 1;
        let mut off = driver(2, 0.5002, 1);
        off.track_surface = 0;
        // Only car 3, which is far away, is eligible.
        let state = state_with(
            LR_CAR_LEFT,
            vec![
                driver(0, 0.5, 1),
                pace,
                off,
                driver(3, 0.5 + 4.0 / 5000.0, 1),
            ],
        );
        let (left, _) = analyze_radar(&state, 4.5);
        let offset = left.expect("left target").offset_m;
        assert!((offset - 4.0).abs() < 0.01, "got {offset}");
    }
}
