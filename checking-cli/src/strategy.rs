//! Strategy engine: the derived numbers for a crew chief.
//!
//! `checking-fuel` answers the question "with this fuel per lap, what occurs?". A crew chief asks the
//! opposite question: "what must I do to get a better result?" This module calculates those
//! answers: the fuel per lap that keeps the stop count, and the lap window for a stop.
//!
//! The Fuel tab and the overlays call this module directly. It has no UI code, so the unit tests
//! run the same code as the GUI. The tab and the overlays all read from here, so they always
//! agree. All functions are pure: `&AppState` in, plain data out.

use checking_fuel::AppState;

/// Minimum fuel per lap rate that the code can divide by. It covers the `unknown estimate` and
/// `zero rate` cases, so that the code does not need `> 0.0` checks in many locations.
const MIN_RATE: f32 = 0.001;

/// Largest fraction above the current rate to report as push room. Use it only when there is no
/// measured PUSH rate for this driver.
///
/// The stop count is a step function, so the rate at which the next stop occurs can be much higher than any pace that a
/// driver can hold. A one-stop plan with a large tank reports "+35% push room" because the second
/// stop does not occur until that rate. A driver cannot use 35% more fuel per lap by pushing. Above
/// this limit, the correct report is "the plan does not limit you", and not a number that tells the
/// driver to use fuel that the stint does not have.
///
/// Use the PUSH rate and not this constant when PUSH is available. PUSH is the average of the green
/// laps of this driver with the highest fuel use, so it is measured data. This constant is only an
/// estimate.
const MAX_REPORTABLE_HEADROOM_FRAC: f32 = 0.15;

/// The result of one fuel per lap rate for the remainder of the race.
#[derive(Debug, Clone)]
pub struct StrategyOutcome {
    /// Name of the source rate: "LOW", "AVG", "PUSH", "MAX" or "custom".
    pub label: String,
    pub fuel_per_lap: f32,
    /// Liters remaining (negative is short) if the car goes to the flag on the fuel in the car,
    /// without a stop. The LAPS LEFT cards show this value.
    pub margin_without_stopping: f32,
    /// True if the fuel in the car is sufficient to the finish without a stop.
    pub finishes_without_stopping: bool,
    /// Number of laps for this tank at this rate.
    pub laps_on_current_fuel: f32,
    /// Liters to add now to get to the finish (0 if the fuel is sufficient). In a race with more
    /// than one stop, this is the total that is still necessary, and it can be more than the tank
    /// capacity. For the quantity at the next stop, use `fuel_at_first_stop`.
    pub fuel_to_add_now: f32,
    /// Liters to add at the first stop, with the tank capacity as the limit.
    pub fuel_at_first_stop: Option<f32>,
    /// Lap of the stop, or `None` if no stop is necessary.
    pub pit_lap: Option<i32>,
}

/// The lap range for a stop, in which the car does not run out of fuel and does not need an extra
/// splash. This shows the difference between "pit now" and "you have a window of five laps, so use
/// the caution if it occurs".
#[derive(Debug, Clone)]
pub struct PitWindow {
    /// First useful lap for the stop. A refuel before this lap wastes tank capacity, because the
    /// tank cannot hold enough fuel to get to the end.
    pub earliest_lap: i32,
    /// Last lap on which a stop is possible. After this lap, the fuel runs out.
    pub latest_lap: i32,
}

/// The highest rate that keeps a given stop count. It answers "how much harder can I push before it
/// costs a stop".
///
/// It tells what extra fuel the driver can use before a stop is added.
#[derive(Debug, Clone)]
pub struct MaxSustainableRate {
    /// The highest fuel per lap that still finishes with the stop count of the reference rate.
    pub fuel_per_lap: f32,
    /// True when the step edge is above the fuel use that this driver can get. This limit is the
    /// measured PUSH pace, or the drivable fraction fallback before there are sufficient green
    /// laps. Then `fuel_per_lap` is this limit and not the edge, and the stop count stays the same
    /// at full push. Show "fuel is not the limit" and not the raw edge, because the raw edge tells
    /// the driver to use fuel that the stint does not have.
    pub unconstrained: bool,
}

/// All derived results together.
#[derive(Debug, Clone)]
pub struct StrategyAnalysis {
    pub laps_to_go: f32,
    pub outcomes: Vec<StrategyOutcome>,
    /// The highest rate that keeps the stop count of the reference strategy.
    pub max_sustainable: Option<MaxSustainableRate>,
}

/// The four named fuel per lap estimates, from the lowest fuel use to the highest.
pub struct Rates {
    pub low: Option<f32>,
    pub avg: Option<f32>,
    pub push: Option<f32>,
    pub max: Option<f32>,
}

impl Rates {
    pub fn from_state(state: &AppState, total_laps: i32) -> Self {
        Self {
            low: state.avg_fuel_low_pct_of_total(total_laps),
            avg: state.avg_fuel_pct_of_total(total_laps),
            push: state.avg_fuel_push_pct_of_total(total_laps),
            max: state.max_fuel_usage(),
        }
    }

    /// The rate that the code uses as the current pace of the driver. AVG is the default. If it is
    /// not available, use the next rate in the list, so that a session with little data can still
    /// be analyzed.
    pub fn reference(&self) -> Option<f32> {
        self.avg.or(self.push).or(self.low).or(self.max)
    }

    fn labeled(&self) -> Vec<(&'static str, f32)> {
        [
            ("LOW", self.low),
            ("AVG", self.avg),
            ("PUSH", self.push),
            ("MAX", self.max),
        ]
        .into_iter()
        .filter_map(|(name, v)| v.filter(|v| *v >= MIN_RATE).map(|v| (name, v)))
        .collect()
    }
}

/// Runs one rate through the same `checking-fuel` calls that the Fuel tab uses, so that the overlays
/// and the tab always agree.
pub fn evaluate_rate(
    state: &AppState,
    label: &str,
    fuel_per_lap: f32,
    capacity: f32,
    extra_liters: f32,
) -> StrategyOutcome {
    let (pit_lap, pit_fill) = state.calculate_pit_strategy(fuel_per_lap, capacity, extra_liters);
    let (finishes_without_stopping, margin_without_stopping) =
        state.calculate_fuel_margin(fuel_per_lap, extra_liters);

    StrategyOutcome {
        label: label.to_string(),
        fuel_per_lap,
        margin_without_stopping,
        finishes_without_stopping,
        laps_on_current_fuel: state.laps_remaining(fuel_per_lap),
        fuel_to_add_now: state.fuel_to_add(fuel_per_lap, extra_liters),
        // Use `calculate_pit_strategy` and not the simulation. Both use the tank capacity as the
        // limit, but only `calculate_pit_strategy` includes `extra_liters`. The driver sets this
        // number in the pit menu, so it must include the safety margin. The simulation refuels to
        // the exact remaining distance.
        fuel_at_first_stop: (pit_lap > 0).then_some(pit_fill),
        pit_lap: (pit_lap > 0).then_some(pit_lap),
    }
}

/// Number of stops at a given rate. Nothing else is included.
fn stops_at(state: &AppState, fuel_per_lap: f32, capacity: f32) -> i32 {
    checking_fuel::simulate_race_strategy(
        state.latest_frame.fuel_level,
        state.get_current_lap_fractional(),
        state.get_effective_total_laps(),
        fuel_per_lap,
        capacity,
    )
    .num_stops
}

/// Finds the highest fuel per lap that still gives `target_stops`, with a binary search on the
/// rate.
///
/// The function searches and does not solve. The stop count is a step function of the rate (each
/// step is one more tank), and `simulate_race_strategy` already has the tank limit and the partial
/// lap rules. A closed form inverse must copy those rules, and the copy can become different. 40
/// bisections cost almost nothing and are always consistent with the simulation. The driver wants
/// the highest such rate: the most fuel that the driver can use with the plan.
fn max_rate_for_stops(
    state: &AppState,
    target_stops: i32,
    capacity: f32,
    upper: f32,
) -> Option<f32> {
    // A rate that is low enough to be useful must give the target.
    let mut lo = MIN_RATE;
    if stops_at(state, lo, capacity) > target_stops {
        return None;
    }
    let mut hi = upper;
    if stops_at(state, hi, capacity) <= target_stops {
        return Some(hi);
    }

    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if stops_at(state, mid, capacity) <= target_stops {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

/// The highest rate that still finishes with the current number of stops.
///
/// The function searches above the reference for the fuel budget that the current plan allows. If a driver saves
/// fuel and the stop count does not change, the driver loses lap time and gets nothing. The stop
/// count alone does not show this. Only the difference between the two rates shows it.
///
/// Returns `None` when the reference rate cannot finish, because then the push margin has no
/// meaning.
///
/// `push` is the measured PUSH rate, if the session has one. It is the upper limit of the report,
/// because it is the highest fuel use of this driver in this session. A margin above it is for a
/// lap that the driver did not drive. Before there are sufficient green laps, pass `None`, and the
/// drivable fraction constant replaces it.
pub fn compute_max_sustainable(
    state: &AppState,
    reference: f32,
    push: Option<f32>,
    capacity: f32,
) -> Option<MaxSustainableRate> {
    if reference < MIN_RATE {
        return None;
    }
    let stops = stops_at(state, reference, capacity);

    // The search goes up to two times the reference. This is much higher than any real push lap, so
    // the range always contains the real answer. The bisection gets to the step edge for all upper
    // bounds.
    let rate = max_rate_for_stops(state, stops, capacity, reference * 2.0)?;

    // The fuel use that this driver can get. PUSH is the average of the green laps of the driver
    // with the highest fuel use, so it is measured data. But it is a limit only when it is above
    // the reference. A driver who saved fuel in all of the session can have PUSH at or below AVG. A
    // clamp to PUSH would then report "no margin" because of a small sample, and not because of the
    // fuel.
    let drivable = push
        .filter(|p| *p > reference)
        .unwrap_or(reference * (1.0 + MAX_REPORTABLE_HEADROOM_FRAC));

    // Limit the reported value. Above the limit, the step edge is real but it is not a useful
    // target. It tells that the next stop is far away, not that the driver can hold this pace.
    // Report the limited rate, so that no later code shows a number that the car cannot get. Set
    // the flag, so that the UI can tell why.
    let unconstrained = rate > drivable;
    let rate = if unconstrained { drivable } else { rate };

    Some(MaxSustainableRate {
        fuel_per_lap: rate,
        unconstrained,
    })
}

/// The lap range for one stop.
///
/// Latest: the lap on which the fuel runs out. Earliest: the first lap from which a full tank is
/// sufficient for the remaining distance. A stop before that lap fills a tank that cannot hold
/// sufficient fuel, so a second stop is necessary. This is useful only when there is one stop from
/// here. Races with more than one stop return `None`, and the pit details of the simulation
/// describe them.
pub fn compute_pit_window(
    state: &AppState,
    reference: f32,
    capacity: f32,
    extra_liters: f32,
) -> Option<PitWindow> {
    if reference < MIN_RATE {
        return None;
    }
    let laps_to_go = state.laps_to_go();
    let current = state.get_current_lap_fractional();

    // Dry lap: the lap on which the current fuel runs out.
    let laps_on_fuel = state.laps_remaining(reference);
    if laps_on_fuel >= laps_to_go {
        return None; // No stop needed at this rate.
    }
    let latest = (current + laps_on_fuel).floor() as i32;

    // Number of laps for a full tank, minus the safety margin.
    let laps_on_full = ((capacity - extra_liters).max(0.0) / reference).floor();
    let total_laps = state.get_effective_total_laps();

    // First useful lap for the stop. If one more tank can finish the race, a stop before this point
    // wastes capacity that the tank cannot hold. Thus the window opens where a full refuel gets
    // exactly to the flag. If the race is longer than one more tank, more stops are necessary, and
    // each lap up to the dry lap is a valid stop. Then the window opens now.
    let one_more_tank_finishes = laps_on_full >= laps_to_go;
    let earliest = if one_more_tank_finishes {
        ((total_laps as f32 - laps_on_full).ceil() as i32).max(current.ceil() as i32)
    } else {
        current.ceil() as i32
    };
    let earliest = earliest.min(latest);

    Some(PitWindow {
        earliest_lap: earliest,
        latest_lap: latest,
    })
}

/// Makes the full analysis for the current state.
pub fn analyze(state: &AppState, extra_liters: f32, capacity: f32) -> StrategyAnalysis {
    let total_laps = state.get_effective_total_laps();
    let rates = Rates::from_state(state, total_laps);
    let reference = rates.reference();

    let outcomes: Vec<StrategyOutcome> = rates
        .labeled()
        .into_iter()
        .map(|(label, rate)| evaluate_rate(state, label, rate, capacity, extra_liters))
        .collect();

    StrategyAnalysis {
        laps_to_go: state.laps_to_go(),
        max_sustainable: reference
            .and_then(|r| compute_max_sustainable(state, r, rates.push, capacity)),
        outcomes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    const CAP: f32 = 60.0;
    const MARGIN: f32 = 0.5;

    /// The first refuel must fit in the tank, for all totals.
    #[test]
    fn first_stop_fill_never_exceeds_tank_capacity() {
        for scenario in [Scenario::MidRace, Scenario::Marginal, Scenario::LongRun] {
            let a = analyze(&scenario.build(), MARGIN, CAP);
            for o in &a.outcomes {
                if let Some(fill) = o.fuel_at_first_stop {
                    assert!(
                        fill <= CAP + 0.01,
                        "{} {}: first fill {fill:.1} L exceeds the {CAP:.0} L tank",
                        scenario.name(),
                        o.label,
                    );
                }
            }
        }
    }

    #[test]
    fn pit_window_is_ordered_and_within_the_race() {
        let state = Scenario::LongRun.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let w = compute_pit_window(&state, reference, CAP, MARGIN).unwrap();

        assert!(w.earliest_lap <= w.latest_lap, "{w:?}");
        assert!(w.latest_lap <= state.get_effective_total_laps(), "{w:?}");
    }

    /// Fuel for approximately one more lap: the stop cannot wait, and the window must show that. It
    /// must not show free laps.
    #[test]
    fn urgent_scenario_reports_an_immediate_stop() {
        let state = Scenario::Urgent.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let w = compute_pit_window(&state, reference, CAP, MARGIN).unwrap();

        let current_lap = state.get_current_lap_fractional().ceil() as i32;
        assert!(w.latest_lap <= current_lap, "{w:?}");
        assert_eq!(
            w.earliest_lap, w.latest_lap,
            "an urgent stop has no freedom: {w:?}"
        );
    }

    /// A timed race has no fixed lap count. The analysis must still have a result, from
    /// `session_laps_remain_ex` and not from `session_laps_total`.
    #[test]
    fn timed_session_still_produces_a_strategy() {
        let state = Scenario::Timed.build();
        let a = analyze(&state, MARGIN, CAP);
        let total_laps = state.get_effective_total_laps();

        // The value 32767 must never be in the output as a lap count.
        assert!(total_laps > 0 && total_laps < 1000, "{total_laps}");
        assert!(!a.outcomes.is_empty());
        assert!(a.laps_to_go > 0.0);
    }

    /// Caution laps use much less fuel. They must not be in the rate estimates. If they are, the
    /// strategy uses a number that the driver cannot hold after the race goes green again.
    #[test]
    fn yellow_laps_do_not_drag_down_the_rate_estimates() {
        let state = Scenario::Yellow.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();

        // The two yellow laps used approximately 1.1 L and the green laps approximately 2.7 L. The
        // reference must use the green laps.
        assert!(
            reference > 2.0,
            "yellow laps leaked into the estimate: {reference:.3} L/lap",
        );
    }

    /// `margin_without_stopping` is relative to a target that already includes the reserve. Thus a
    /// car that uses part of the reserve reports a small negative margin and still gets to the
    /// flag. The UI uses this to show the difference between "uses the reserve" and "runs out of
    /// fuel". If the sign convention changes, the UI shows the opposite without a warning.
    #[test]
    fn margin_is_net_of_the_reserve() {
        let state = Scenario::MidRace.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();

        let no_reserve = evaluate_rate(&state, "AVG", reference, CAP, 0.0);
        let with_reserve = evaluate_rate(&state, "AVG", reference, CAP, MARGIN);

        // A reserve decreases the reported margin by exactly the reserve.
        assert!(
            (no_reserve.margin_without_stopping - with_reserve.margin_without_stopping - MARGIN)
                .abs()
                < 0.01,
            "{} vs {}",
            no_reserve.margin_without_stopping,
            with_reserve.margin_without_stopping,
        );
    }

    #[test]
    fn analysis_survives_a_session_with_no_lap_data() {
        let state = Scenario::NoData.build();
        let a = analyze(&state, MARGIN, CAP);
        // There are no estimates, so there is no advice. The function must not panic or return
        // incorrect data.
        assert!(a.outcomes.is_empty());
        assert!(a.max_sustainable.is_none());
    }

    /// The budget rate must give the stop count that it reports, and it must be the edge of that
    /// step: a rate just above it must cost a stop. Without the second check, the function can
    /// return any low rate and pass.
    #[test]
    fn max_sustainable_is_the_edge_of_the_stop_count() {
        let state = Scenario::MidRace.build();
        let rates = Rates::from_state(&state, state.get_effective_total_laps());
        let reference = rates.reference().unwrap();

        let budget = compute_max_sustainable(&state, reference, rates.push, CAP)
            .expect("a running session has a stop count to hold");
        let stops = stops_at(&state, reference, CAP);

        // In both branches, the reported rate must keep the stop count.
        assert_eq!(stops_at(&state, budget.fuel_per_lap, CAP), stops);

        if budget.unconstrained {
            // Limited: the rate is the drivable limit and not the step edge, so a higher rate does
            // not have to cost a stop. The rate must be exactly the limit: the measured PUSH pace
            // if there is one, else the drivable fraction fallback.
            let expected = rates
                .push
                .filter(|p| *p > reference)
                .unwrap_or(reference * (1.0 + MAX_REPORTABLE_HEADROOM_FRAC));
            assert!(
                (budget.fuel_per_lap - expected).abs() < 0.01,
                "capped budget should sit at the drivable ceiling {:.4}, got {:.4}",
                expected,
                budget.fuel_per_lap,
            );
        } else {
            // Not limited: this is the edge, so a rate just above it costs a stop.
            assert!(
                stops_at(&state, budget.fuel_per_lap * 1.02, CAP) > stops,
                "{:.4} should be the highest rate holding {} stops",
                budget.fuel_per_lap,
                stops,
            );
        }
    }

    /// Regression test. MidRace is a
    /// one-stop plan. Its second stop occurs only at 3.50 L/lap, and the pace is 2.60 L/lap. A
    /// report of the raw +34.6% tells the driver to use fuel that no driver can use by pushing.
    #[test]
    fn reported_push_room_is_drivable() {
        for scenario in [
            Scenario::MidRace,
            Scenario::Marginal,
            Scenario::LongRun,
            Scenario::Timed,
            Scenario::Yellow,
            Scenario::Urgent,
        ] {
            let state = scenario.build();
            let rates = Rates::from_state(&state, state.get_effective_total_laps());
            let Some(reference) = rates.reference() else {
                continue;
            };
            let Some(budget) = compute_max_sustainable(&state, reference, rates.push, CAP) else {
                continue;
            };

            // The limit is the measured PUSH pace of this driver if there is one, else the drivable
            // fraction fallback. The report must not be more than this limit.
            let ceiling = rates
                .push
                .filter(|p| *p > reference)
                .unwrap_or(reference * (1.0 + MAX_REPORTABLE_HEADROOM_FRAC));
            assert!(
                budget.fuel_per_lap <= ceiling + 0.01,
                "{:?}: reported {:.4} L/lap of push room, above the drivable ceiling {:.4}",
                scenario,
                budget.fuel_per_lap,
                ceiling,
            );
        }
    }

    /// The budget is an upper limit, so it can never be below the rate that it came from. If it is,
    /// the current pace keeps a stop count that the "maximum" for that count cannot keep.
    #[test]
    fn max_sustainable_is_never_below_the_reference() {
        for scenario in [Scenario::MidRace, Scenario::LongRun] {
            let state = scenario.build();
            let rates = Rates::from_state(&state, state.get_effective_total_laps());
            let reference = rates.reference().unwrap();

            let budget = compute_max_sustainable(&state, reference, rates.push, CAP).unwrap();
            assert!(
                budget.fuel_per_lap >= reference - 1e-4,
                "{:?}: budget {:.4} < reference {:.4}",
                scenario,
                budget.fuel_per_lap,
                reference,
            );
        }
    }

    /// The stint timeline draws one segment for each pit lap, so the pit laps must be in sequence
    /// and in the race. A pit lap after the flag or out of sequence makes a segment with zero or
    /// negative width. The GUI does not show it, but it is not correct.
    #[test]
    fn pit_laps_are_ordered_and_inside_the_race() {
        for scenario in [
            Scenario::MidRace,
            Scenario::Marginal,
            Scenario::LongRun,
            Scenario::Timed,
            Scenario::Yellow,
            Scenario::Urgent,
        ] {
            let state = scenario.build();
            let total = state.get_effective_total_laps();
            let a = analyze(&state, MARGIN, CAP);

            for o in &a.outcomes {
                let sim = checking_fuel::simulate_race_strategy(
                    state.latest_frame.fuel_level,
                    state.get_current_lap_fractional(),
                    total,
                    o.fuel_per_lap,
                    CAP,
                );
                if !sim.can_finish {
                    continue;
                }
                let laps: Vec<i32> = sim.pit_details.iter().map(|(l, _)| *l).collect();
                assert_eq!(
                    laps.len(),
                    sim.num_stops as usize,
                    "{:?}/{}: {} stops but {} pit laps",
                    scenario,
                    o.label,
                    sim.num_stops,
                    laps.len(),
                );
                for w in laps.windows(2) {
                    assert!(
                        w[0] <= w[1],
                        "{:?}/{}: {laps:?} out of order",
                        scenario,
                        o.label
                    );
                }
                for l in &laps {
                    assert!(
                        *l >= 0 && *l <= total,
                        "{:?}/{}: pit lap {l} outside 0..={total}",
                        scenario,
                        o.label,
                    );
                }
            }
        }
    }

    #[test]
    fn outcomes_are_ordered_by_consumption() {
        let state = Scenario::MidRace.build();
        let a = analyze(&state, MARGIN, CAP);
        assert!(!a.outcomes.is_empty());
        for w in a.outcomes.windows(2) {
            assert!(
                w[0].fuel_per_lap <= w[1].fuel_per_lap,
                "{} > {}",
                w[0].label,
                w[1].label,
            );
        }
    }
}
