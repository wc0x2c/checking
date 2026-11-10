//! The most fuel that the car can hold in the session.
//!
//! A league or series can limit the fuel to a fraction of the tank. iRacing gives the tank size in
//! `DriverCarFuelMaxLtr` and the limit in `DriverCarMaxFuelPct`, both in the `DriverInfo` section
//! of the session YAML. `DriverCarMaxFuelPct` is a fraction, thus 1.000 is a full tank.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SessionFuel {
    #[serde(rename = "DriverInfo")]
    driver_info: DriverFuel,
}

#[derive(Debug, Deserialize)]
struct DriverFuel {
    #[serde(rename = "DriverCarFuelMaxLtr")]
    fuel_max_ltr: Option<f32>,
    #[serde(rename = "DriverCarMaxFuelPct")]
    max_fuel_fraction: Option<f32>,
}

/// Returns the most fuel in liters that the car can hold in the session: the tank size times the
/// limit of the session. Returns `None` when the YAML has no tank size. A limit that is not there,
/// or that is not more than 0 and not more than 1, is a full tank.
pub fn session_max_fuel_ltr(yaml: &str) -> Option<f32> {
    let fuel = serde_yaml::from_str::<SessionFuel>(yaml).ok()?.driver_info;
    let tank = fuel.fuel_max_ltr.filter(|v| v.is_finite() && *v > 0.0)?;
    let fraction = fuel
        .max_fuel_fraction
        .filter(|f| *f > 0.0 && *f <= 1.0)
        .unwrap_or(1.0);
    Some(tank * fraction)
}

/// Returns the fuel capacity that the app uses for the fill and the stint length.
///
/// `max_fuel_ltr` is the session limit from `session_max_fuel_ltr`, or 0 when it is not known.
/// `FuelLevel / FuelLevelPct` also gives a capacity. When it differs by more than 10 L from the
/// session limit, the tank was probably filled to a level that the session info does not show, so
/// that value is used. The capacity is never more than the session limit, because the car cannot
/// hold more. Thus the result is correct if iRacing calculates `FuelLevelPct` from the full tank or
/// from the limit.
pub fn fuel_capacity(max_fuel_ltr: f32, fuel_level: f32, fuel_level_pct: f32) -> f32 {
    let known_max = max_fuel_ltr >= 1.0;
    if fuel_level_pct <= 0.001 || fuel_level <= 0.0 {
        return max_fuel_ltr.max(0.0);
    }
    let calculated = fuel_level / fuel_level_pct;
    if !known_max {
        return calculated;
    }
    if calculated > 10.0 && (max_fuel_ltr - calculated).abs() > 10.0 {
        calculated.min(max_fuel_ltr)
    } else {
        max_fuel_ltr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(driver_info: &str) -> String {
        format!(
            "---\nWeekendInfo:\n TrackName: roadamerica full\n TrackLength: 6.43 km\n\
             DriverInfo:\n DriverCarIdx: 5\n{driver_info} Drivers:\n - CarIdx: 0\n   UserName: Pace Car\n...\n"
        )
    }

    #[test]
    fn a_full_tank_has_no_limit() {
        let text = yaml(" DriverCarFuelMaxLtr: 70.030\n DriverCarMaxFuelPct: 1.000\n");
        assert_eq!(session_max_fuel_ltr(&text), Some(70.03));
    }

    #[test]
    fn the_league_limit_reduces_the_tank() {
        let text = yaml(" DriverCarFuelMaxLtr: 70.000\n DriverCarMaxFuelPct: 0.750\n");
        assert_eq!(session_max_fuel_ltr(&text), Some(52.5));
    }

    #[test]
    fn no_limit_in_the_yaml_is_a_full_tank() {
        let text = yaml(" DriverCarFuelMaxLtr: 70.000\n");
        assert_eq!(session_max_fuel_ltr(&text), Some(70.0));
    }

    #[test]
    fn a_limit_out_of_range_is_a_full_tank() {
        for limit in ["0.000", "-0.5", "1.500"] {
            let text = yaml(&format!(
                " DriverCarFuelMaxLtr: 70.000\n DriverCarMaxFuelPct: {limit}\n"
            ));
            assert_eq!(session_max_fuel_ltr(&text), Some(70.0), "{limit}");
        }
    }

    #[test]
    fn capacity_is_the_limit_if_the_percentage_is_of_the_full_tank() {
        // 75% of a 70 L tank, with FuelLevelPct of the full tank: 35 L is 50%.
        assert_eq!(fuel_capacity(52.5, 35.0, 0.5), 52.5);
    }

    #[test]
    fn capacity_is_the_limit_if_the_percentage_is_of_the_limit() {
        // 75% of a 70 L tank, with FuelLevelPct of the limit: 35 L is 66.7%.
        let capacity = fuel_capacity(52.5, 35.0, 35.0 / 52.5);
        assert!((capacity - 52.5).abs() < 1e-3, "{capacity}");
    }

    #[test]
    fn a_partial_fill_that_the_session_does_not_show_is_used() {
        // A 70 L tank that the league fills to 50 L only.
        let capacity = fuel_capacity(70.0, 25.0, 0.5);
        assert!((capacity - 50.0).abs() < 1e-3, "{capacity}");
    }

    #[test]
    fn capacity_without_fuel_data_is_the_limit() {
        assert_eq!(fuel_capacity(52.5, 0.0, 0.0), 52.5);
        assert_eq!(fuel_capacity(0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn capacity_without_a_limit_is_calculated() {
        assert_eq!(fuel_capacity(0.0, 30.0, 0.5), 60.0);
    }

    #[test]
    fn no_tank_size_gives_no_value() {
        assert_eq!(
            session_max_fuel_ltr(&yaml(" DriverCarMaxFuelPct: 0.750\n")),
            None
        );
        assert_eq!(
            session_max_fuel_ltr(&yaml(" DriverCarFuelMaxLtr: 0.000\n")),
            None
        );
        assert_eq!(session_max_fuel_ltr("not yaml: ["), None);
    }
}
