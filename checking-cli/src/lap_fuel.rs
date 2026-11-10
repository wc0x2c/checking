//! Values for the lap fuel scale: the fuel that the last lap used, and each lap that counts toward
//! the strategy, between the LOW, AVG, PUSH and MAX rates.
//!
//! The app records the four rates at the end of each lap in `AppState::lap_snapshots`. The laps
//! that count toward the strategy are the laps that `AppState::get_strategy_lap_assignments` gives
//! to LOW, AVG or PUSH. The scale shows them from dim, the oldest, to bright, the newest, so that
//! the driver sees which laps make the rates.

use checking_fuel::{AppState, LapSnapshot};

/// Brightness of the line of the oldest strategy lap.
const OLDEST_LAP_BRIGHTNESS: f32 = 0.15;

/// Brightness of the line of the newest strategy lap.
const NEWEST_LAP_BRIGHTNESS: f32 = 0.85;

/// The four fuel rate strategies, from the lowest fuel use to the highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Low,
    Avg,
    Push,
    Max,
}

impl Strategy {
    pub const ALL: [Self; 4] = [Self::Low, Self::Avg, Self::Push, Self::Max];
}

/// The rate of each strategy at the end of one lap, in the order of `Strategy::ALL`. `None` when
/// the rate has no data yet.
pub type LapRates = [Option<f32>; 4];

fn rates_of(snapshot: &LapSnapshot) -> LapRates {
    [
        snapshot.low_estimate,
        snapshot.avg_estimate,
        snapshot.push_estimate,
        snapshot.max_estimate,
    ]
}

/// The fuel use of the last completed lap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LastLap {
    pub fuel_used: f32,
    /// The strategy whose rate is nearest to `fuel_used`.
    pub strategy: Option<Strategy>,
}

/// One lap that counts toward the strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrategyLap {
    pub fuel_used: f32,
    /// The strategy whose rate is nearest to `fuel_used`.
    pub strategy: Option<Strategy>,
    /// From `OLDEST_LAP_BRIGHTNESS` for the oldest lap to `NEWEST_LAP_BRIGHTNESS` for the newest.
    pub brightness: f32,
}

/// A linear scale of liters per lap, from the lowest value to the highest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    pub min: f32,
    pub max: f32,
}

impl Scale {
    /// Makes the scale from the lowest to the highest value. Returns `None` when there is no
    /// value.
    fn fitting(values: impl Iterator<Item = f32>) -> Option<Self> {
        values
            .filter(|v| v.is_finite())
            .fold(None, |scale: Option<Self>, v| match scale {
                None => Some(Self { min: v, max: v }),
                Some(Self { min, max }) => Some(Self {
                    min: min.min(v),
                    max: max.max(v),
                }),
            })
    }

    /// The position of `value` on the scale, from 0 at `min` to 1 at `max`. A value outside the
    /// scale is on the nearest end. When all values are the same, they are in the middle.
    pub fn fraction(&self, value: f32) -> f32 {
        let span = self.max - self.min;
        if span > 0.0 {
            ((value - self.min) / span).clamp(0.0, 1.0)
        } else {
            0.5
        }
    }
}

/// All values that the overlay shows.
#[derive(Debug, Clone, PartialEq)]
pub struct LapFuelView {
    pub last_lap: LastLap,
    /// The rates at the end of the last lap.
    pub rates: LapRates,
    /// The laps that count toward the strategy, the oldest first.
    pub strategy_laps: Vec<StrategyLap>,
    pub scale: Scale,
}

/// Returns the strategy whose rate is nearest to `fuel_used`. When two rates are at the same
/// distance, the lower strategy is returned.
pub fn nearest_strategy(fuel_used: f32, rates: &LapRates) -> Option<Strategy> {
    Strategy::ALL
        .into_iter()
        .zip(rates)
        .filter_map(|(strategy, rate)| rate.map(|r| (strategy, (r - fuel_used).abs())))
        .filter(|(_, distance)| distance.is_finite())
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(strategy, _)| strategy)
}

/// Returns the brightness of lap `index` of `count` laps, from the oldest at index 0 to the newest.
pub fn lap_brightness(index: usize, count: usize) -> f32 {
    if count <= 1 {
        return NEWEST_LAP_BRIGHTNESS;
    }
    let age_fraction = index.min(count - 1) as f32 / (count - 1) as f32;
    OLDEST_LAP_BRIGHTNESS + (NEWEST_LAP_BRIGHTNESS - OLDEST_LAP_BRIGHTNESS) * age_fraction
}

/// Returns the fuel of each lap that counts toward the LOW, AVG or PUSH rate, the oldest first.
/// MAX is the highest PUSH lap, so it adds no lap.
pub fn strategy_lap_fuel(state: &AppState) -> Vec<f32> {
    let (avg, push, low) = state.get_strategy_lap_assignments(state.get_effective_total_laps());
    state
        .lap_data
        .iter()
        .filter(|lap| {
            [&avg, &push, &low]
                .iter()
                .any(|laps| laps.contains(&lap.lap_number))
        })
        .map(|lap| lap.fuel_used)
        .collect()
}

/// Makes the scale values from the lap snapshots and the fuel of the strategy laps, the oldest
/// first. Returns `None` before the first complete lap.
pub fn analyze_lap_fuel(
    snapshots: &[LapSnapshot],
    strategy_lap_fuel: &[f32],
) -> Option<LapFuelView> {
    let last = snapshots.last()?;
    let fuel_used = last.fuel_used?;
    let rates = rates_of(last);
    let strategy_laps: Vec<StrategyLap> = strategy_lap_fuel
        .iter()
        .enumerate()
        .map(|(index, fuel)| StrategyLap {
            fuel_used: *fuel,
            strategy: nearest_strategy(*fuel, &rates),
            brightness: lap_brightness(index, strategy_lap_fuel.len()),
        })
        .collect();

    let values = core::iter::once(fuel_used)
        .chain(rates.iter().flatten().copied())
        .chain(strategy_lap_fuel.iter().copied());
    let scale = Scale::fitting(values)?;

    Some(LapFuelView {
        last_lap: LastLap {
            fuel_used,
            strategy: nearest_strategy(fuel_used, &rates),
        },
        rates,
        strategy_laps,
        scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // An explicit import wins over the glob of the proptest prelude, which has a trait of the same
    // name.
    use super::Strategy;
    use proptest::prelude::*;

    fn snapshot(lap_number: i32, fuel_used: f32, rates: LapRates) -> LapSnapshot {
        let [low, avg, push, max] = rates;
        LapSnapshot {
            lap_number,
            fuel_level_at_start: 50.0,
            fuel_used: Some(fuel_used),
            avg_estimate: avg,
            push_estimate: push,
            low_estimate: low,
            max_estimate: max,
            was_yellow_lap: false,
            fuel_level_pct: 0.5,
            fuel_max_ltr: 100.0,
            fuel_capacity_calculated: 100.0,
        }
    }

    const RATES: LapRates = [Some(2.4), Some(2.5), Some(2.6), Some(2.8)];

    /// Drives the laps of `fuel_per_lap` through `AppState::update`, from lap 1.
    fn drive(state: &mut checking_fuel::AppState, fuel_per_lap: &[f32]) {
        let mut frame = checking_protocol::TelemetryFrame {
            lap_current: 1,
            fuel_level: 60.0,
            leader_lap: 1,
            ..Default::default()
        };
        state.update(frame.clone(), 0);

        let mut fuel = 60.0;
        for (lap, fuel_used) in (2..).zip(fuel_per_lap) {
            for step in 1..=9 {
                frame.lap_dist_pct = step as f32 / 10.0;
                frame.fuel_level = fuel - fuel_used * step as f32 / 10.0;
                state.update(frame.clone(), 0);
            }
            fuel -= fuel_used;
            frame.lap_current = lap;
            frame.leader_lap = lap;
            frame.lap_dist_pct = 0.0;
            frame.fuel_level = fuel;
            state.update(frame.clone(), 0);
        }
    }

    #[test]
    fn no_view_before_the_first_lap() {
        assert_eq!(analyze_lap_fuel(&[], &[]), None);
    }

    #[test]
    fn strategy_laps_skip_the_first_lap_and_the_outliers() {
        let mut state = checking_fuel::AppState::new();
        // Each lap has a different value, so that the test can find the order of the laps.
        let laps = [
            2.9, 2.50, 2.52, 2.48, 2.51, 2.49, 2.53, 2.47, 2.54, 2.46, 4.0, 2.505,
        ];
        drive(&mut state, &laps);

        let fuel = strategy_lap_fuel(&state);

        assert!(
            !fuel.contains(&2.9),
            "the first lap is not a strategy lap: {fuel:?}"
        );
        assert!(
            !fuel.contains(&4.0),
            "the outlier is not a strategy lap: {fuel:?}"
        );
        assert!(!fuel.is_empty());
        // The oldest first, as the laps were driven.
        let order: Vec<usize> = fuel
            .iter()
            .map(|f| laps.iter().position(|l| (l - f).abs() < 1e-4).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    }

    #[test]
    fn the_oldest_lap_is_dim_and_the_newest_is_bright() {
        assert_eq!(lap_brightness(0, 1), NEWEST_LAP_BRIGHTNESS);
        assert_eq!(lap_brightness(0, 5), OLDEST_LAP_BRIGHTNESS);
        assert_eq!(lap_brightness(4, 5), NEWEST_LAP_BRIGHTNESS);
        assert!(lap_brightness(1, 5) < lap_brightness(2, 5));
    }

    #[test]
    fn strategy_laps_take_the_color_of_the_nearest_rate() {
        let view = analyze_lap_fuel(&[snapshot(1, 2.5, RATES)], &[2.41, 2.79]).unwrap();

        let colors: Vec<_> = view.strategy_laps.iter().map(|l| l.strategy).collect();
        assert_eq!(colors, [Some(Strategy::Low), Some(Strategy::Max)]);
        assert_eq!(view.strategy_laps[0].brightness, OLDEST_LAP_BRIGHTNESS);
        assert_eq!(view.strategy_laps[1].brightness, NEWEST_LAP_BRIGHTNESS);
    }

    #[test]
    fn the_last_lap_takes_the_color_of_the_nearest_rate() {
        assert_eq!(nearest_strategy(2.39, &RATES), Some(Strategy::Low));
        assert_eq!(nearest_strategy(2.52, &RATES), Some(Strategy::Avg));
        assert_eq!(nearest_strategy(2.62, &RATES), Some(Strategy::Push));
        assert_eq!(nearest_strategy(3.5, &RATES), Some(Strategy::Max));
    }

    #[test]
    fn a_tie_goes_to_the_lower_strategy() {
        assert_eq!(
            nearest_strategy(2.45, &[Some(2.4), Some(2.5), None, None]),
            Some(Strategy::Low)
        );
    }

    #[test]
    fn rates_without_data_are_skipped() {
        assert_eq!(
            nearest_strategy(2.4, &[None, Some(2.5), None, Some(3.0)]),
            Some(Strategy::Avg)
        );
        assert_eq!(nearest_strategy(2.4, &[None; 4]), None);
    }

    #[test]
    fn the_scale_ends_at_the_lowest_and_highest_values() {
        let view = analyze_lap_fuel(&[snapshot(1, 2.55, RATES)], &[]).unwrap();

        assert_eq!(view.scale, Scale { min: 2.4, max: 2.8 });
        assert_eq!(view.scale.fraction(2.4), 0.0);
        assert_eq!(view.scale.fraction(2.8), 1.0);
    }

    #[test]
    fn the_last_lap_can_be_an_end_of_the_scale() {
        let view = analyze_lap_fuel(&[snapshot(1, 1.2, RATES)], &[]).unwrap();

        assert_eq!(view.scale.min, 1.2);
        assert_eq!(view.last_lap.strategy, Some(Strategy::Low));
    }

    #[test]
    fn equal_values_are_in_the_middle() {
        let view = analyze_lap_fuel(&[snapshot(1, 2.5, [Some(2.5); 4])], &[]).unwrap();
        assert_eq!(view.scale.fraction(2.5), 0.5);
    }

    #[test]
    fn values_outside_the_scale_are_on_its_ends() {
        let scale = Scale { min: 2.0, max: 3.0 };
        assert_eq!(scale.fraction(1.0), 0.0);
        assert_eq!(scale.fraction(4.0), 1.0);
        assert_eq!(scale.fraction(2.25), 0.25);
    }

    /// The view must show a lap in the same frame in which the car crosses the line at its end,
    /// and not one lap later.
    #[test]
    fn the_view_shows_the_lap_that_just_ended() {
        let laps = [2.6, 2.4, 2.9];
        for count in 1..=laps.len() {
            let mut state = checking_fuel::AppState::new();
            drive(&mut state, &laps[..count]);

            let view = analyze_lap_fuel(&state.lap_snapshots, &[]).unwrap();
            let expected = laps[count - 1];
            assert!(
                (view.last_lap.fuel_used - expected).abs() < 1e-4,
                "lap {count}: {} and not {expected}",
                view.last_lap.fuel_used
            );
        }
    }

    fn rate() -> impl proptest::strategy::Strategy<Value = Option<f32>> {
        proptest::option::of(0.5f32..6.0)
    }

    proptest! {
        /// The scale holds all values of the view, and its ends are the lowest and the highest
        /// value.
        #[test]
        fn the_scale_fits_the_values(
            fuel in 0.5f32..6.0,
            rates in [rate(), rate(), rate(), rate()],
            strategy_laps in proptest::collection::vec(0.5f32..6.0, 0..20),
        ) {
            let view = analyze_lap_fuel(&[snapshot(1, fuel, rates)], &strategy_laps).unwrap();

            let values: Vec<f32> = core::iter::once(view.last_lap.fuel_used)
                .chain(view.rates.iter().flatten().copied())
                .chain(view.strategy_laps.iter().map(|l| l.fuel_used))
                .collect();
            for value in &values {
                prop_assert!(view.scale.min <= *value && *value <= view.scale.max, "{value} {:?}", view.scale);
            }
            prop_assert!(values.contains(&view.scale.min));
            prop_assert!(values.contains(&view.scale.max));
        }

        /// The nearest strategy has no rate farther away than any other rate.
        #[test]
        fn nearest_strategy_is_the_nearest(
            fuel in 0.5f32..6.0,
            rates in [rate(), rate(), rate(), rate()],
        ) {
            match nearest_strategy(fuel, &rates) {
                None => prop_assert!(rates.iter().all(Option::is_none)),
                Some(found) => {
                    let index = Strategy::ALL.iter().position(|s| *s == found).unwrap();
                    let best = (rates[index].unwrap() - fuel).abs();
                    for rate in rates.iter().flatten() {
                        prop_assert!(best <= (rate - fuel).abs());
                    }
                }
            }
        }
    }
}
