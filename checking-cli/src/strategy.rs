//! Strategy engine: the derived numbers for a crew chief.
//!
//! `checking-fuel` answers the question "with this fuel per lap, what occurs?". A crew chief asks the
//! opposite question: "what must I do to get a better result?" This module calculates those
//! answers: the fuel per lap that removes a stop, the liters to save, and the lap window for a
//! stop.
//!
//! The Fuel tab and the overlays call this module directly. It has no UI code, so the unit tests
//! run the same code as the GUI. The tab and the overlays all read from here, so they always
//! agree. All functions are pure: `&AppState` in, plain data out.

use checking_fuel::AppState;

/// A saving target below this number of liters costs nothing. It is in the noise of the fuel
/// measurement from lap to lap, so do not report it as a lift and coast target.
const NEGLIGIBLE_SAVING: f32 = 0.02;

/// Minimum fuel per lap rate that the code can divide by. It covers the `unknown estimate` and
/// `zero rate` cases, so that the code does not need `> 0.0` checks in many locations.
const MIN_RATE: f32 = 0.001;

/// Largest fraction of the fuel use that a driver can save and still race. Hard lift and coast with
/// short shifts saves approximately 10-15%. Above that, the lap time loss is larger than the time
/// of the stop that it saves. Targets above this value are rejected and not reported. The search
/// always finds a rate that balances the fuel arithmetic, also an impossible rate, and an
/// impossible target is worse than no target.
const MAX_REALISTIC_SAVING_FRAC: f32 = 0.15;

/// Largest fraction above the current rate to report as push room. Use it only when there is no
/// measured PUSH rate for this driver.
///
/// This is the opposite of `MAX_REALISTIC_SAVING_FRAC`, for the same reason. The stop count is a
/// step function, so the rate at which the next stop occurs can be much higher than any pace that a
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
    pub stops: i32,
    pub can_finish: bool,
    /// Liters remaining at the flag after the planned stops. The simulation refuels to the exact
    /// quantity for the remaining distance, so this is approximately 0 when `stops > 0`. It means
    /// that the refuel quantity is correct, not that the fuel almost runs out. It is a useful
    /// margin only when there is no stop. For the question "can I finish on this tank", use
    /// `margin_without_stopping`.
    pub fuel_margin: f32,
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

/// The answer to "can I save fuel to remove a stop?". This is the main strategy question, and the
/// `checking-fuel` API cannot answer it directly.
#[derive(Debug, Clone)]
pub struct SavingTarget {
    /// Number of stops at the reference (current) rate.
    pub stops_now: i32,
    /// Number of stops if the driver holds `target_fuel_per_lap`.
    pub stops_if_saving: i32,
    /// The rate to hold for the remainder of the race to remove one stop.
    pub target_fuel_per_lap: f32,
    /// Difference per lap between this rate and the reference rate.
    pub saving_per_lap: f32,
    /// The same difference, as a percentage of the reference rate.
    pub saving_pct: f32,
    /// Total liters to save in the remaining laps.
    pub total_saving: f32,
}

/// Tells if the current tank alone can get to the finish, and the cost.
#[derive(Debug, Clone)]
pub struct FinishOnThisTank {
    pub possible: bool,
    /// Rate at which the tank is empty exactly at the flag (no safety margin).
    pub required_fuel_per_lap: f32,
    /// The rate for the comparison.
    pub reference_fuel_per_lap: f32,
    /// Liters per lap to save from the reference rate. 0 if the tank is already sufficient.
    pub saving_per_lap: f32,
    pub laps_to_go: f32,
    pub fuel_in_tank: f32,
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
    /// Number of laps between the two.
    pub window_laps: i32,
    /// True when the stop must occur now.
    pub is_urgent: bool,
}

/// The result if the driver pits at the end of the current lap.
///
/// The pit window tells when a stop is possible. This tells the cost of a stop now. The driver
/// needs this under a caution, or when the car in front goes into the pits and there is very little
/// time to decide.
#[derive(Debug, Clone)]
pub struct PitNow {
    /// The lap of the stop (the current lap, when it is complete).
    pub pit_lap: i32,
    /// Liters to add: sufficient to get to the flag with the reserve, with the tank capacity as the
    /// limit.
    pub fuel_to_add: f32,
    /// True when one refuel from here is sufficient to the finish.
    pub reaches_finish: bool,
    /// Total stops for the race if this stop occurs now, including this stop.
    pub total_stops: i32,
    /// Total stops if the driver waits for the optimal window.
    pub stops_if_waiting: i32,
    /// Laps of fuel lost because of an early refuel. The tank cannot hold its current fuel and a
    /// full refuel, so the fuel in the tank takes space from the refuel.
    pub laps_wasted: f32,
    /// True when a stop now costs one more stop than a stop later.
    pub costs_a_stop: bool,
}

/// Fuel in the car relative to the fuel that the remainder of the race needs, without a stop.
///
/// A negative delta is a problem only when no stop is planned. In a race with a planned stop, it
/// only shows that the stop is necessary. Read `stop_planned` before you show a shortfall as a
/// warning.
#[derive(Debug, Clone)]
pub struct FuelDelta {
    /// Liters to get to the finish at the reference rate, with the margin.
    pub fuel_needed: f32,
    pub fuel_in_tank: f32,
    /// A positive value is a surplus, and a negative value is a shortfall.
    pub delta: f32,
    /// The surplus or shortfall in laps.
    pub delta_laps: f32,
    /// True when the strategy has one or more stops. Then a negative `delta` is expected and is not
    /// a risk.
    pub stop_planned: bool,
}

/// The highest rate that keeps a given stop count. It answers "how much harder can I push before it
/// costs a stop".
///
/// This is the opposite of `SavingTarget`. `SavingTarget` tells what to save to remove a stop. This
/// tells what extra fuel the driver can use before a stop is added. It uses the same bisection in
/// the other direction.
#[derive(Debug, Clone)]
pub struct MaxSustainableRate {
    /// The stop count that this rate keeps.
    pub stops: i32,
    /// The highest fuel per lap that still finishes with `stops` stops.
    pub fuel_per_lap: f32,
    /// Margin above the reference rate: the extra liters per lap available to push. A negative
    /// value means that the current pace already uses more than this budget.
    pub headroom_per_lap: f32,
    /// The same margin, as a percentage of the reference rate.
    pub headroom_pct: f32,
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
    pub total_laps: i32,
    pub current_lap: i32,
    pub fuel_in_tank: f32,
    pub tank_capacity: f32,
    pub safety_margin: f32,
    /// The rate that all comparisons use as the current pace.
    pub reference_fuel_per_lap: Option<f32>,
    pub outcomes: Vec<StrategyOutcome>,
    /// Set when the arithmetic allows one less stop.
    pub saving_target: Option<SavingTarget>,
    pub finish_on_this_tank: Option<FinishOnThisTank>,
    pub pit_window: Option<PitWindow>,
    /// The result of a stop at the end of the current lap.
    pub pit_now: Option<PitNow>,
    pub fuel_delta: Option<FuelDelta>,
    /// The fewest stops of all rates, and the rate that gives them.
    pub best_case_stops: Option<i32>,
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
    let sim = checking_fuel::simulate_race_strategy(
        state.latest_frame.fuel_level,
        state.get_current_lap_fractional(),
        state.get_effective_total_laps(),
        fuel_per_lap,
        capacity,
    );
    let (pit_lap, pit_fill) = state.calculate_pit_strategy(fuel_per_lap, capacity, extra_liters);
    let (finishes_without_stopping, margin_without_stopping) =
        state.calculate_fuel_margin(fuel_per_lap, extra_liters);

    StrategyOutcome {
        label: label.to_string(),
        fuel_per_lap,
        stops: sim.num_stops,
        can_finish: sim.can_finish,
        fuel_margin: sim.fuel_margin,
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

/// Tells if fuel saving can remove one stop, and the cost per lap.
///
/// Returns `None` when the saving is too small, or when no realistic rate gives the lower stop
/// count. The second case is important. When the remaining distance is longer than a full tank, the
/// arithmetic for zero stops has a solution with a very low rate. No driver can hold that rate, and
/// a report of it would hide the useful advice.
pub fn compute_saving_target(
    state: &AppState,
    reference: f32,
    capacity: f32,
) -> Option<SavingTarget> {
    let stops_now = stops_at(state, reference, capacity);
    if stops_now == 0 {
        return None; // Already no stops; nothing to save toward.
    }

    let target_stops = stops_now - 1;
    let target = max_rate_for_stops(state, target_stops, capacity, reference)?;
    let saving = reference - target;
    if saving <= NEGLIGIBLE_SAVING {
        return None;
    }

    // Reject targets that a driver cannot hold. The bisection only tells if the fuel arithmetic
    // balances, and it always does. At zero stops, it divides the fuel in the car by the remaining
    // laps, which always balances, also when the rate is impossible. A target is real only if a
    // driver can hold it. Lift and coast saves a few percent, not a third of the fuel use. Without
    // this check, a race with a remaining distance longer than a full tank reports "save 44% and
    // skip the stop".
    if saving / reference > MAX_REALISTIC_SAVING_FRAC {
        return None;
    }

    Some(SavingTarget {
        stops_now,
        stops_if_saving: target_stops,
        target_fuel_per_lap: target,
        saving_per_lap: saving,
        saving_pct: 100.0 * saving / reference,
        total_saving: saving * state.laps_to_go(),
    })
}

/// The highest rate that still finishes with the current number of stops.
///
/// `compute_saving_target` searches below the reference for a rate that removes a stop. This
/// function searches above it for the fuel budget that the current plan allows. If a driver saves
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
    let headroom = rate - reference;

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
    let (rate, headroom) = if unconstrained {
        (drivable, drivable - reference)
    } else {
        (rate, headroom)
    };

    Some(MaxSustainableRate {
        stops,
        fuel_per_lap: rate,
        headroom_per_lap: headroom,
        headroom_pct: 100.0 * headroom / reference,
        unconstrained,
    })
}

/// The rate that gets this tank to the flag, and its difference from the current pace.
///
/// Returns `None` when the necessary rate is so far below the current pace that no driver can hold
/// it. In a long race, "you need 0.49 L/lap" is only arithmetic, not advice, and it hides the
/// important result.
pub fn compute_finish_on_tank(state: &AppState, reference: f32) -> Option<FinishOnThisTank> {
    let laps_to_go = state.laps_to_go();
    if laps_to_go <= 0.0 {
        return None;
    }
    let fuel = state.latest_frame.fuel_level;
    let required = fuel / laps_to_go;

    // A cut larger than a driver can hold is not a way to make the stint longer. It means that the
    // race needs one more stop.
    if required < reference * (1.0 - MAX_REALISTIC_SAVING_FRAC) {
        return None;
    }

    Some(FinishOnThisTank {
        possible: required >= reference,
        required_fuel_per_lap: required,
        reference_fuel_per_lap: reference,
        saving_per_lap: (reference - required).max(0.0),
        laps_to_go,
        fuel_in_tank: fuel,
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
        window_laps: (latest - earliest).max(0),
        is_urgent: latest <= current.ceil() as i32,
        earliest_lap: earliest,
        latest_lap: latest,
    })
}

/// The result of a stop at the end of this lap.
///
/// Returns `None` when the race is over. The function answers for the current lap and not for
/// "now", because the car must complete the lap to get to the pit lane. The earliest real stop is
/// at the end of the current lap.
pub fn compute_pit_now(
    state: &AppState,
    reference: f32,
    capacity: f32,
    extra_liters: f32,
) -> Option<PitNow> {
    if reference < MIN_RATE {
        return None;
    }
    let laps_to_go = state.laps_to_go();
    if laps_to_go <= 0.0 {
        return None;
    }

    // The car uses the fuel for the remainder of the current lap. Then it is at the start/finish
    // line with a whole number of laps remaining.
    let lap_dist_pct = state.latest_frame.lap_dist_pct;
    let fuel_at_stop = (state.latest_frame.fuel_level - reference * (1.0 - lap_dist_pct)).max(0.0);
    let laps_after_stop = (laps_to_go - (1.0 - lap_dist_pct)).max(0.0);
    // The stop is at the end of the current lap, which is `current_lap`. Do not use the floor of
    // `get_current_lap_fractional` here, because it is zero-based (11.35 on lap 12).
    let pit_lap = state.current_lap;

    // Refuel to get to the flag, with the tank capacity as the limit.
    let want = laps_after_stop * reference + extra_liters;
    let fuel_to_add = (want - fuel_at_stop).max(0.0).min(capacity - fuel_at_stop);
    let fuel_after_fill = (fuel_at_stop + fuel_to_add).min(capacity);
    let reaches_finish = fuel_after_fill >= laps_after_stop * reference + extra_liters;

    // Stops for the remainder of the race after this stop: simulate from the pit exit, and add this
    // stop.
    let further_stops = checking_fuel::simulate_race_strategy(
        fuel_after_fill,
        state.get_effective_total_laps() as f32 - laps_after_stop,
        state.get_effective_total_laps(),
        reference,
        capacity,
    )
    .num_stops;
    let total_stops = 1 + further_stops;

    // The cost of an early stop. The tank can take only `capacity - fuel_at_stop`, so each liter in
    // the tank is one liter less refuel. If the driver used that fuel first, the space was
    // available. The value is in laps, because "you lose 2.3 laps of stint" is what the driver uses
    // to decide.
    //
    // This is a real loss only when the tank capacity limits the refuel. If the driver needs only a
    // splash, the fuel in the tank costs nothing.
    let filled_to_brim = fuel_after_fill >= capacity - 0.01;
    let laps_wasted = if filled_to_brim {
        fuel_at_stop / reference
    } else {
        0.0
    };

    let stops_if_waiting = stops_at(state, reference, capacity);

    Some(PitNow {
        pit_lap,
        fuel_to_add,
        reaches_finish,
        total_stops,
        stops_if_waiting,
        laps_wasted,
        costs_a_stop: total_stops > stops_if_waiting,
    })
}

/// Surplus or shortfall relative to the fuel that the remainder of the race needs.
pub fn compute_fuel_delta(
    state: &AppState,
    reference: f32,
    extra_liters: f32,
    capacity: f32,
) -> Option<FuelDelta> {
    if reference < MIN_RATE {
        return None;
    }
    let needed = state.laps_to_go() * reference + extra_liters;
    let fuel = state.latest_frame.fuel_level;
    let delta = fuel - needed;

    Some(FuelDelta {
        fuel_needed: needed,
        fuel_in_tank: fuel,
        delta,
        delta_laps: delta / reference,
        stop_planned: stops_at(state, reference, capacity) > 0,
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

    let best_case_stops = outcomes
        .iter()
        .filter(|o| o.can_finish)
        .map(|o| o.stops)
        .min();

    StrategyAnalysis {
        laps_to_go: state.laps_to_go(),
        total_laps,
        current_lap: state.current_lap,
        fuel_in_tank: state.latest_frame.fuel_level,
        tank_capacity: capacity,
        safety_margin: extra_liters,
        reference_fuel_per_lap: reference,
        saving_target: reference.and_then(|r| compute_saving_target(state, r, capacity)),
        finish_on_this_tank: reference.and_then(|r| compute_finish_on_tank(state, r)),
        pit_window: reference.and_then(|r| compute_pit_window(state, r, capacity, extra_liters)),
        pit_now: reference.and_then(|r| compute_pit_now(state, r, capacity, extra_liters)),
        fuel_delta: reference.and_then(|r| compute_fuel_delta(state, r, extra_liters, capacity)),
        max_sustainable: reference
            .and_then(|r| compute_max_sustainable(state, r, rates.push, capacity)),
        outcomes,
        best_case_stops,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    const CAP: f32 = 60.0;
    const MARGIN: f32 = 0.5;

    #[test]
    fn saving_target_drops_exactly_one_stop() {
        let state = Scenario::LongRun.build();
        let rates = Rates::from_state(&state, state.get_effective_total_laps());
        let reference = rates.reference().unwrap();

        let target = compute_saving_target(&state, reference, CAP)
            .expect("a long race at this pace should have a stop to save");

        assert_eq!(target.stops_if_saving, target.stops_now - 1);
        // The target rate must give the stop count that it reports.
        assert_eq!(
            stops_at(&state, target.target_fuel_per_lap, CAP),
            target.stops_if_saving,
        );
        assert!(target.saving_per_lap > 0.0);
    }

    /// Regression test. MidRace has 28.6 laps remaining, but a full 60 L tank is sufficient for
    /// only approximately 23 laps. Thus zero stops is not possible with any fuel saving. The
    /// bisection can still find a rate that balances the arithmetic (approximately 1.47 L/lap, a
    /// 44% cut), but no driver can hold it.
    #[test]
    fn no_saving_target_when_the_tank_cannot_cover_the_distance() {
        let state = Scenario::MidRace.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();

        // Precondition: the remaining distance is longer than a full tank.
        assert!(state.laps_to_go() * reference > CAP);

        assert!(
            compute_saving_target(&state, reference, CAP).is_none(),
            "proposed saving out of a stop that the tank size makes impossible",
        );
    }

    /// A reported saving target must be one that a driver can hold. A target of a third of the fuel
    /// use is only arithmetic and not strategy.
    #[test]
    fn reported_saving_targets_are_drivable() {
        for scenario in [
            Scenario::MidRace,
            Scenario::Marginal,
            Scenario::LongRun,
            Scenario::NoData,
        ] {
            let state = scenario.build();
            let Some(reference) =
                Rates::from_state(&state, state.get_effective_total_laps()).reference()
            else {
                continue;
            };
            let Some(t) = compute_saving_target(&state, reference, CAP) else {
                continue;
            };

            assert!(
                t.saving_pct <= MAX_REALISTIC_SAVING_FRAC * 100.0,
                "{}: reported an undrivable {:.1}% saving target",
                scenario.name(),
                t.saving_pct,
            );
            // It must also give the stop that it reports.
            assert_eq!(
                stops_at(&state, t.target_fuel_per_lap, CAP),
                t.stops_if_saving,
                "{}: target does not deliver the claimed stop count",
                scenario.name(),
            );
        }
    }

    #[test]
    fn no_saving_target_when_no_stops_needed() {
        // Marginal is a short run to the flag on the fuel in the car.
        let state = Scenario::Marginal.build();
        let rates = Rates::from_state(&state, state.get_effective_total_laps());
        let reference = rates.reference().unwrap();
        if stops_at(&state, reference, CAP) == 0 {
            assert!(compute_saving_target(&state, reference, CAP).is_none());
        }
    }

    #[test]
    fn target_rate_is_the_highest_that_works() {
        let state = Scenario::LongRun.build();
        let rates = Rates::from_state(&state, state.get_effective_total_laps());
        let reference = rates.reference().unwrap();
        let target = compute_saving_target(&state, reference, CAP).unwrap();

        // The target is the boundary. At the target the stop is removed, and just above it the stop
        // comes back. The bisection gets very near the step, so test a small absolute amount past
        // it and not a ratio.
        assert_eq!(
            stops_at(&state, target.target_fuel_per_lap, CAP),
            target.stops_if_saving,
        );
        let above = target.target_fuel_per_lap + 0.01;
        assert!(
            stops_at(&state, above, CAP) > target.stops_if_saving,
            "target {:.4} was not the boundary rate: {:.4} still gives {} stops",
            target.target_fuel_per_lap,
            above,
            stops_at(&state, above, CAP),
        );
    }

    #[test]
    fn finish_on_tank_agrees_with_fuel_arithmetic() {
        // Move Marginal to less than one lap from the flag, where the fuel in the car is sufficient
        // and the calculation applies. (Marginal as built is just at the drivable limit, so it is
        // not a stable test case for the arithmetic.)
        let mut state = Scenario::Marginal.build();
        state.latest_frame.lap_current = 24;
        state.current_lap = 24;

        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let f = compute_finish_on_tank(&state, reference)
            .expect("a lap from home on 9.5 L is comfortably makeable");

        // The necessary rate multiplied by the remaining laps equals the fuel in the car.
        assert!((f.required_fuel_per_lap * f.laps_to_go - f.fuel_in_tank).abs() < 0.01);
        assert_eq!(f.possible, f.required_fuel_per_lap >= reference);
        assert!(f.possible, "should reach the flag with fuel to spare");
    }

    /// When the fuel in the car gets to the flag only with a cut that no driver can hold, report
    /// nothing, and not a number such as "you need 0.49 L/lap".
    #[test]
    fn finish_on_tank_is_silent_when_the_cut_is_undrivable() {
        let state = Scenario::LongRun.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        assert!(compute_finish_on_tank(&state, reference).is_none());
    }

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
        assert_eq!(w.window_laps, w.latest_lap - w.earliest_lap);
        assert!(w.latest_lap <= state.get_effective_total_laps(), "{w:?}");
    }

    #[test]
    fn fuel_delta_sign_matches_can_finish() {
        let state = Scenario::MidRace.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let d = compute_fuel_delta(&state, reference, MARGIN, CAP).unwrap();
        let (can_finish, margin) = state.calculate_fuel_margin(reference, MARGIN);

        assert_eq!(d.delta >= 0.0, can_finish);
        assert!((d.delta - margin).abs() < 0.01);
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

        assert!(w.is_urgent, "{w:?}");
        assert_eq!(w.window_laps, 0, "an urgent stop has no freedom: {w:?}");
    }

    /// A timed race has no fixed lap count. The analysis must still have a result, from
    /// `session_laps_remain_ex` and not from `session_laps_total`.
    #[test]
    fn timed_session_still_produces_a_strategy() {
        let state = Scenario::Timed.build();
        let a = analyze(&state, MARGIN, CAP);

        // The value 32767 must never be in the output as a lap count.
        assert!(a.total_laps > 0 && a.total_laps < 1000, "{}", a.total_laps);
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

    /// A stop now must add sufficient fuel to get to the flag when the tank allows it, and never
    /// more than the tank can hold.
    #[test]
    fn pit_now_fill_fits_the_tank_and_reaches_the_flag() {
        for scenario in [Scenario::MidRace, Scenario::Marginal, Scenario::Urgent] {
            let state = scenario.build();
            let reference = Rates::from_state(&state, state.get_effective_total_laps())
                .reference()
                .unwrap();
            let p = compute_pit_now(&state, reference, CAP, MARGIN).unwrap();

            // The refuel plus the fuel in the tank can never be more than the tank capacity.
            let at_stop = (state.latest_frame.fuel_level
                - reference * (1.0 - state.latest_frame.lap_dist_pct))
                .max(0.0);
            assert!(
                at_stop + p.fuel_to_add <= CAP + 0.01,
                "{}: {:.1} + {:.1} exceeds {CAP:.0} L tank",
                scenario.name(),
                at_stop,
                p.fuel_to_add,
            );
            assert!(p.fuel_to_add >= 0.0);
            assert!(p.total_stops >= 1, "boxing now is itself a stop");
        }
    }

    /// A stop that is too early costs a stop, and the result must show this.
    ///
    /// MidRace is a one-stop race if the driver waits. After a stop now, 27 laps remain, which need
    /// 70.7 L, but the tank holds 60 L. Thus a stop on this lap cannot get to the flag and causes a
    /// second stop. The question "what occurs if I pit on this lap" is for this case.
    #[test]
    fn pit_now_reports_the_extra_stop_when_boxing_too_early() {
        let state = Scenario::MidRace.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let p = compute_pit_now(&state, reference, CAP, MARGIN).unwrap();

        assert!(!p.reaches_finish, "a full tank cannot cover 27 laps: {p:?}");
        assert_eq!(p.stops_if_waiting, 1, "waiting is a one-stop race: {p:?}");
        assert_eq!(p.total_stops, 2, "boxing now needs two visits: {p:?}");
        assert!(p.costs_a_stop, "{p:?}");
    }

    /// The stop is on the current lap, not on the lap that is complete.
    #[test]
    fn pit_now_reports_the_current_lap() {
        let state = Scenario::MidRace.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let p = compute_pit_now(&state, reference, CAP, MARGIN).unwrap();
        assert_eq!(p.pit_lap, state.current_lap, "{p:?}");
    }

    /// Near the flag, one splash is sufficient.
    #[test]
    fn pit_now_finishes_the_job_when_close_to_home() {
        let state = Scenario::Marginal.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let p = compute_pit_now(&state, reference, CAP, MARGIN).unwrap();

        assert!(p.reaches_finish, "{p:?}");
        assert_eq!(p.total_stops, 1, "{p:?}");
        assert!(!p.costs_a_stop, "{p:?}");
    }

    /// An early stop with a lot of fuel in the tank loses tank space. The loss counts only when the
    /// tank capacity limits the refuel.
    #[test]
    fn pit_now_reports_waste_only_when_the_tank_caps_the_fill() {
        // LongRun: 55 L in the tank, and it needs much more than one tank. The refuel fills the
        // tank, so the fuel in the tank takes space from the refuel.
        let state = Scenario::LongRun.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let p = compute_pit_now(&state, reference, CAP, MARGIN).unwrap();
        assert!(
            p.laps_wasted > 0.0,
            "filling a nearly-full tank to the brim wastes the fuel aboard: {p:?}",
        );

        // Marginal: only a splash is necessary, so the fuel in the tank costs nothing.
        let state = Scenario::Marginal.build();
        let reference = Rates::from_state(&state, state.get_effective_total_laps())
            .reference()
            .unwrap();
        let p = compute_pit_now(&state, reference, CAP, MARGIN).unwrap();
        assert_eq!(p.laps_wasted, 0.0, "a splash wastes nothing: {p:?}");
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
        assert!(a.saving_target.is_none());
        assert!(a.best_case_stops.is_none());
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

        // In both branches, the reported rate must keep the stop count.
        assert_eq!(stops_at(&state, budget.fuel_per_lap, CAP), budget.stops);

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
                stops_at(&state, budget.fuel_per_lap * 1.02, CAP) > budget.stops,
                "{:.4} should be the highest rate holding {} stops",
                budget.fuel_per_lap,
                budget.stops,
            );
        }
    }

    /// Regression test, the opposite of `reported_saving_targets_are_drivable`. MidRace is a
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
            assert!(budget.headroom_per_lap >= -1e-4);
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
