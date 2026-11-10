use deku::prelude::*;

pub const PROTOCOL_VERSION: u16 = 4;

/// One driver and car in the session.
#[derive(Debug, Clone, PartialEq, DekuRead, DekuWrite)]
#[deku(ctx = "endian: deku::ctx::Endian", endian = "endian")]
pub struct DriverInfo {
    pub car_idx: i32, // Car index in the session
    #[deku(count = "64")]
    pub user_name: Vec<u8>, // Driver name (null-terminated string, max 64 bytes)
    #[deku(count = "16")]
    pub car_number: Vec<u8>, // Car number as string (max 16 bytes)
    pub car_class_id: i32, // Car class ID
    pub irating: i32, // iRating
    #[deku(count = "16")]
    pub license_string: Vec<u8>, // License (e.g., "A 4.99", max 16 bytes)
    pub position: i32, // Overall position
    pub class_position: i32, // Position in class
    pub lap: i32,     // Current lap
    pub lap_dist_pct: f32, // Fraction of the lap (0.0-1.0)
    pub lap_best_time: f32, // Best lap time
    pub lap_last_time: f32, // Last lap time
    pub est_time: f32, // Estimated time to the current track location (s)
    pub class_est_lap_time: f32, // Estimated lap time of the car class (s). est_time uses this scale.
    pub speed: f32,              // Current speed
    pub on_pit_road: i32,        // 1 if on pit road, else 0
    pub laps_since_pit: i32,     // Laps completed since last pit stop
    pub track_surface: i32,      // Track location (irsdk TrkLoc)
    pub is_pacing: i32,          // Always 0. The sender does not detect pacing.
    pub is_pace_car: i32,        // Always 0. The sender does not detect the pace car.
    pub car_class_relative_speed: i32, // Relative speed in class
}

impl DriverInfo {
    /// Returns user_name as a String.
    pub fn get_user_name(&self) -> String {
        String::from_utf8_lossy(&self.user_name)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Returns car_number as a String.
    pub fn get_car_number(&self) -> String {
        String::from_utf8_lossy(&self.car_number)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Returns license_string as a String.
    pub fn get_license_string(&self) -> String {
        String::from_utf8_lossy(&self.license_string)
            .trim_end_matches('\0')
            .to_string()
    }

    /// Makes a `DriverInfo` from strings. The car starts in first position at
    /// the start of lap 0. Use [`Self::with_track_position`] to change this.
    pub fn new(
        car_idx: i32,
        user_name: &str,
        car_number: &str,
        license_string: &str,
        irating: i32,
    ) -> Self {
        let mut user_name_bytes = vec![0u8; 64];
        let user_name_src = user_name.as_bytes();
        user_name_bytes[..user_name_src.len().min(63)]
            .copy_from_slice(&user_name_src[..user_name_src.len().min(63)]);

        let mut car_number_bytes = vec![0u8; 16];
        let car_number_src = car_number.as_bytes();
        car_number_bytes[..car_number_src.len().min(15)]
            .copy_from_slice(&car_number_src[..car_number_src.len().min(15)]);

        let mut license_bytes = vec![0u8; 16];
        let license_src = license_string.as_bytes();
        license_bytes[..license_src.len().min(15)]
            .copy_from_slice(&license_src[..license_src.len().min(15)]);

        Self {
            car_idx,
            user_name: user_name_bytes,
            car_number: car_number_bytes,
            car_class_id: 0,
            irating,
            license_string: license_bytes,
            position: 1,
            class_position: 1,
            lap: 0,
            lap_dist_pct: 0.0,
            lap_best_time: 0.0,
            lap_last_time: 0.0,
            est_time: 0.0,
            class_est_lap_time: 0.0,
            speed: 0.0,
            on_pit_road: 0,
            laps_since_pit: 0,
            track_surface: 3, // 3 = on track (irsdk TrkLoc)
            is_pacing: 0,
            is_pace_car: 0,
            car_class_relative_speed: 0,
        }
    }

    /// Sets the overall position, which is also the class position, the lap
    /// and the fraction of the lap.
    pub fn with_track_position(mut self, position: i32, lap: i32, lap_dist_pct: f32) -> Self {
        self.position = position;
        self.class_position = position;
        self.lap = lap;
        self.lap_dist_pct = lap_dist_pct;
        self
    }
}

#[derive(Debug, Clone, PartialEq, DekuRead, DekuWrite)]
#[deku(endian = "little")]
pub struct TelemetryFrame {
    pub version: u16,
    pub timestamp_ms: u64,

    // Driving inputs
    pub speed: f32,
    pub rpm: f32,
    pub gear: i32,
    pub throttle: f32,
    pub brake: f32,
    pub clutch: f32,
    pub steering_wheel_angle: f32,

    // Timing
    pub session_time: f64,
    pub lap_current: i32,
    pub lap_best_time: f32,
    pub lap_last_time: f32,
    pub lap_dist_pct: f32,
    pub leader_lap: i32,

    // Car state
    pub fuel_level: f32,
    pub fuel_level_pct: f32, // Raw FuelLevelPct from SDK (0.0-1.0)
    pub fuel_max_ltr: f32,   // DriverCarFuelMaxLtr times DriverCarMaxFuelPct: the session limit
    pub fuel_use_per_hour: f32,
    pub fuel_capacity: f32, // Calculated tank capacity in liters
    pub pit_sv_fuel: f32,   // Pit service fuel add amount in liters
    pub oil_temp: f32,
    pub water_temp: f32,
    pub oil_pressure: f32,
    pub voltage: f32,

    // Tires (FL, FR, RL, RR)
    #[deku(count = "4")]
    pub tire_temps_surface: Vec<f32>,
    #[deku(count = "4")]
    pub tire_temps_inner: Vec<f32>,
    #[deku(count = "4")]
    pub tire_temps_middle: Vec<f32>,
    #[deku(count = "4")]
    pub tire_temps_outer: Vec<f32>,
    #[deku(count = "4")]
    pub tire_pressure: Vec<f32>,
    #[deku(count = "4")]
    pub tire_wear: Vec<f32>,

    // Position & session
    pub track_temp: f32,
    pub air_temp: f32,
    pub track_length_meters: f32, // Track length in meters
    pub position: i32,
    pub class_position: i32,
    pub session_state: i32,
    pub session_flags: u32,
    pub session_laps_total: i32,
    pub session_laps_remain_ex: i32,
    pub session_time_remain: f64,
    pub session_num: i32,
    pub on_pit_road: i32,

    // Damage
    pub lr_shockdefl: f32,
    pub rr_shockdefl: f32,

    // Index of the player car. Use it to find the player in `drivers`.
    pub player_car_idx: i32,

    // Car left/right detection (irsdk_CarLeftRight bitfield)
    pub car_left_right: i32,

    // Competitor data (array with a variable length)
    #[deku(update = "self.drivers.len()")]
    pub driver_count: usize,
    #[deku(count = "driver_count")]
    pub drivers: Vec<DriverInfo>,
}

impl Default for TelemetryFrame {
    fn default() -> Self {
        Self {
            version: PROTOCOL_VERSION,
            timestamp_ms: 0,
            speed: 0.0,
            rpm: 0.0,
            gear: 0,
            throttle: 0.0,
            brake: 0.0,
            clutch: 0.0,
            steering_wheel_angle: 0.0,
            session_time: 0.0,
            lap_current: 0,
            lap_best_time: 0.0,
            lap_last_time: 0.0,
            lap_dist_pct: 0.0,
            leader_lap: 0,
            fuel_level: 0.0,
            fuel_level_pct: 0.0,
            fuel_max_ltr: 0.0,
            fuel_use_per_hour: 0.0,
            fuel_capacity: 0.0,
            pit_sv_fuel: 0.0,
            oil_temp: 0.0,
            water_temp: 0.0,
            oil_pressure: 0.0,
            voltage: 0.0,
            tire_temps_surface: vec![0.0; 4],
            tire_temps_inner: vec![0.0; 4],
            tire_temps_middle: vec![0.0; 4],
            tire_temps_outer: vec![0.0; 4],
            tire_pressure: vec![0.0; 4],
            tire_wear: vec![0.0; 4],
            track_temp: 0.0,
            air_temp: 0.0,
            track_length_meters: 0.0,
            position: 0,
            class_position: 0,
            session_state: 0,
            session_flags: 0,
            session_laps_total: 0,
            session_laps_remain_ex: 0,
            session_time_remain: 0.0,
            session_num: 0,
            on_pit_road: 0,
            lr_shockdefl: 0.0,
            rr_shockdefl: 0.0,
            player_car_idx: 0,
            car_left_right: 0,
            driver_count: 0,
            drivers: vec![],
        }
    }
}

impl TelemetryFrame {
    /// Makes a mock telemetry frame for tests.
    pub fn mock_data(iteration: u64) -> Self {
        let player_idx = 5;
        let player_lap_dist = (iteration % 3600) as f32 / 3600.0;

        let drivers = vec![
            DriverInfo::new(0, "John Smith", "1", "A 4.50", 3500).with_track_position(
                1,
                10,
                player_lap_dist + 0.15,
            ),
            DriverInfo::new(1, "Sarah Johnson", "22", "B 3.80", 2800).with_track_position(
                2,
                10,
                player_lap_dist + 0.08,
            ),
            DriverInfo::new(2, "Mike Wilson", "7", "A 4.20", 3200).with_track_position(
                3,
                10,
                player_lap_dist + 0.03,
            ),
            DriverInfo::new(3, "Emma Brown", "45", "C 2.50", 1900).with_track_position(
                4,
                10,
                player_lap_dist - 0.01,
            ),
            DriverInfo::new(4, "Chris Davis", "88", "B 3.40", 2500).with_track_position(
                5,
                10,
                player_lap_dist - 0.05,
            ),
            DriverInfo::new(player_idx, "YOU", "99", "A 4.99", 4200).with_track_position(
                6,
                10,
                player_lap_dist,
            ),
            DriverInfo::new(6, "Alex Miller", "13", "A 4.75", 3800).with_track_position(
                7,
                10,
                player_lap_dist - 0.10,
            ),
            DriverInfo::new(7, "Lisa Anderson", "33", "B 3.60", 2700).with_track_position(
                8,
                10,
                player_lap_dist - 0.15,
            ),
            DriverInfo::new(8, "David Thomas", "5", "C 2.90", 2100).with_track_position(
                9,
                10,
                player_lap_dist - 0.22,
            ),
            DriverInfo::new(9, "Jessica Martinez", "77", "A 4.30", 3300).with_track_position(
                10,
                10,
                player_lap_dist - 0.30,
            ),
        ];

        Self {
            version: PROTOCOL_VERSION,
            timestamp_ms: current_time_ms(),
            speed: 50.0 + ((iteration as f32 * 0.5) % 150.0),
            rpm: 3000.0 + ((iteration as f32 * 10.0) % 5000.0),
            gear: ((iteration / 100) % 6) as i32,
            throttle: ((iteration as f32 % 100.0) / 100.0),
            brake: if iteration % 200 < 20 { 1.0 } else { 0.0 },
            clutch: if iteration % 300 < 10 { 1.0 } else { 0.0 },
            steering_wheel_angle: ((iteration as f32 % 360.0) - 180.0),
            session_time: iteration as f64 * 0.016,
            lap_current: (iteration / 3600) as i32,
            lap_best_time: 87.5,
            lap_last_time: 88.2,
            lap_dist_pct: player_lap_dist,
            leader_lap: (iteration / 3600) as i32, // Mock: the player is on the lead lap
            on_pit_road: 0,
            fuel_level: 20.0 - ((iteration as f32 / 3600.0) * 0.5),
            fuel_level_pct: (20.0 - ((iteration as f32 / 3600.0) * 0.5)) / 60.0, // Mock: 60 L tank
            fuel_max_ltr: 60.0, // Mock: 60 L tank from the SDK
            fuel_use_per_hour: 3.2,
            fuel_capacity: 60.0, // Mock: 60 L tank (calculated)
            pit_sv_fuel: 0.0,    // Mock: no pit service fuel
            oil_temp: 95.0 + ((iteration as f32 % 100.0) / 100.0 * 5.0),
            water_temp: 85.0 + ((iteration as f32 % 100.0) / 100.0 * 3.0),
            oil_pressure: 65.0,
            voltage: 13.8,
            tire_temps_surface: vec![80.0, 81.0, 79.0, 80.5],
            tire_temps_inner: vec![78.0, 79.0, 77.0, 78.5],
            tire_temps_middle: vec![81.0, 82.0, 80.0, 81.5],
            tire_temps_outer: vec![79.0, 80.0, 78.0, 79.5],
            tire_pressure: vec![28.0, 28.1, 27.9, 28.0],
            tire_wear: vec![1.0, 1.5, 1.2, 1.3],
            track_temp: 32.0,
            air_temp: 24.0,
            track_length_meters: 3500.0, // Mock: 3.5 km road course
            position: 6,
            class_position: 3,
            session_state: 4,
            session_flags: 0x00000001,
            session_laps_total: 100,
            session_laps_remain_ex: 50,
            session_time_remain: 3000.0,
            session_num: 0,
            lr_shockdefl: 0.05,
            rr_shockdefl: 0.04,
            player_car_idx: player_idx,
            car_left_right: 0,
            driver_count: drivers.len(),
            drivers,
        }
    }
}

/// Returns the current time in milliseconds since the Unix epoch.
pub fn current_time_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time is before the Unix epoch")
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_roundtrip() {
        let original = TelemetryFrame {
            version: 1,
            timestamp_ms: 1234567890,
            speed: 123.45,
            rpm: 6500.0,
            gear: 4,
            throttle: 0.85,
            brake: 0.0,
            clutch: 0.0,
            steering_wheel_angle: 45.5,
            session_time: 456.789,
            lap_current: 5,
            lap_best_time: 87.5,
            lap_last_time: 88.2,
            lap_dist_pct: 0.65,
            leader_lap: 7,
            fuel_level: 15.5,
            fuel_level_pct: 0.258, // 15.5 / 60.0
            fuel_max_ltr: 60.0,
            fuel_use_per_hour: 3.2,
            fuel_capacity: 60.0,
            pit_sv_fuel: 0.0,
            oil_temp: 95.0,
            water_temp: 85.0,
            oil_pressure: 65.0,
            voltage: 13.8,
            tire_temps_surface: vec![80.0, 81.0, 79.0, 80.5],
            tire_temps_inner: vec![78.0, 79.0, 77.0, 78.5],
            tire_temps_middle: vec![81.0, 82.0, 80.0, 81.5],
            tire_temps_outer: vec![79.0, 80.0, 78.0, 79.5],
            tire_pressure: vec![28.0, 28.1, 27.9, 28.0],
            tire_wear: vec![1.0, 1.5, 1.2, 1.3],
            track_temp: 32.0,
            air_temp: 24.0,
            track_length_meters: 3500.0,
            position: 5,
            class_position: 3,
            session_state: 4,
            session_flags: 0x00000001,
            session_laps_total: 100,
            session_laps_remain_ex: 50,
            session_time_remain: 3000.0,
            session_num: 0,
            on_pit_road: 0,
            lr_shockdefl: 0.05,
            rr_shockdefl: 0.04,
            player_car_idx: 0,
            car_left_right: 0,
            driver_count: 0,
            drivers: vec![],
        };

        let bytes = original.to_bytes().unwrap();

        let (_, decoded) = TelemetryFrame::from_bytes((&bytes, 0)).unwrap();

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_mock_data_creates_valid_frame() {
        let frame = TelemetryFrame::mock_data(100);
        assert_eq!(frame.version, PROTOCOL_VERSION);
        assert!(frame.speed >= 0.0);
        assert!(frame.rpm >= 0.0);
        assert_eq!(frame.tire_temps_surface.len(), 4);
        assert!(!frame.drivers.is_empty());
        assert_eq!(frame.driver_count, frame.drivers.len());
    }

    #[test]
    fn test_default_frame() {
        let frame = TelemetryFrame::default();
        assert_eq!(frame.version, PROTOCOL_VERSION);
        assert_eq!(frame.speed, 0.0);
        assert_eq!(frame.tire_pressure.len(), 4);
    }
}
