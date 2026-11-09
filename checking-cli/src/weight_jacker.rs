//! Weight jacker buttons: one press of a wheel button sends an arrow key a set number of times.
//!
//! In iRacing, each arrow key press moves the weight jacker one step. A large change takes many key
//! presses, which the driver cannot do at speed. This module does what a Pulover Macro Creator
//! macro did before: it reads the joystick buttons, and on each new press it sends the key.
//!
//! The joystick numbers are the same as in AutoHotkey and Pulover, thus `2Joy8` is joystick 2,
//! button 8. The keys go to the window that has the focus, as with the Pulover macro.

use core::time::Duration;
use serde::{Deserialize, Serialize};

/// Highest button number that the Windows joystick API reports.
pub const MAX_BUTTON: u32 = 32;

/// Highest joystick number that the Windows joystick API reports.
pub const MAX_JOYSTICK: u32 = 16;

/// Time between two reads of the buttons. A button press on a wheel is longer than 50 ms, so this
/// does not miss a press.
const POLL_PERIOD: Duration = Duration::from_millis(10);

/// Time between two checks of the settings when the function is off.
const DISABLED_PERIOD: Duration = Duration::from_millis(250);

/// Settings of the weight jacker buttons. The defaults are the values of the Pulover macro that this
/// function replaces.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WeightJackerConfig {
    pub enabled: bool,
    /// Joystick number, from 1. Windows numbers the joysticks in the order that they connect.
    pub joystick: u32,
    /// Button number, from 1, that sends the up arrow key.
    pub up_button: u32,
    /// Button number, from 1, that sends the down arrow key.
    pub down_button: u32,
    /// Number of key presses for one button press.
    pub presses: u32,
    /// Time in milliseconds between two key presses.
    pub delay_ms: u32,
}

impl Default for WeightJackerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            joystick: 2,
            up_button: 8,
            down_button: 3,
            presses: 5,
            delay_ms: 50,
        }
    }
}

/// The arrow key to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    /// The keyboard scan code of the arrow key. iRacing reads the keyboard with DirectInput, which
    /// sees scan codes and not virtual keys. The arrow keys are extended keys.
    pub fn scan_code(self) -> u16 {
        match self {
            Self::Up => 0x48,
            Self::Down => 0x50,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ButtonState {
    #[default]
    Released,
    Pressed,
}

impl ButtonState {
    /// Reads one button from the button mask, in which bit 0 is button 1. A button number outside
    /// 1 to `MAX_BUTTON` is always released.
    fn from_mask(mask: u32, button: u32) -> Self {
        let bit = button
            .checked_sub(1)
            .and_then(|shift| 1u32.checked_shl(shift));
        match bit {
            Some(bit) if mask & bit != 0 => Self::Pressed,
            _ => Self::Released,
        }
    }
}

/// Finds the new presses of the up and down buttons. A held button gives one press only.
#[derive(Debug, Default)]
pub struct PressDetector {
    up: ButtonState,
    down: ButtonState,
}

impl PressDetector {
    /// Returns the direction of each button that changed from released to pressed since the last
    /// call.
    pub fn update(
        &mut self,
        buttons: u32,
        config: &WeightJackerConfig,
    ) -> impl Iterator<Item = Direction> + use<> {
        let up = Self::is_new_press(&mut self.up, buttons, config.up_button);
        let down = Self::is_new_press(&mut self.down, buttons, config.down_button);
        [up.then_some(Direction::Up), down.then_some(Direction::Down)]
            .into_iter()
            .flatten()
    }

    fn is_new_press(state: &mut ButtonState, buttons: u32, button: u32) -> bool {
        let new_state = ButtonState::from_mask(buttons, button);
        let is_new_press = *state == ButtonState::Released && new_state == ButtonState::Pressed;
        *state = new_state;
        is_new_press
    }
}

/// Reads the buttons of a joystick.
pub trait Joystick {
    /// Returns the button mask of the joystick, in which bit 0 is button 1, or `None` when the
    /// joystick is not connected.
    fn buttons(&mut self, joystick: u32) -> Option<u32>;
}

/// Sends key presses.
pub trait Keyboard {
    /// Presses and releases the arrow key one time.
    fn tap(&mut self, direction: Direction);
}

/// Reads the buttons and sends the keys. `sleep` waits for a duration; the tests give a function
/// that only records the duration.
pub struct Worker<J, K, S> {
    joystick: J,
    keyboard: K,
    sleep: S,
    detector: PressDetector,
}

impl<J: Joystick, K: Keyboard, S: FnMut(Duration)> Worker<J, K, S> {
    pub fn new(joystick: J, keyboard: K, sleep: S) -> Self {
        Self {
            joystick,
            keyboard,
            sleep,
            detector: PressDetector::default(),
        }
    }

    /// Reads the buttons one time, sends the keys for each new press, and then waits until the
    /// next read.
    pub fn poll(&mut self, config: &WeightJackerConfig) {
        if !config.enabled {
            // Forget the buttons, so that a button held when the function goes on is not a press.
            self.detector = PressDetector::default();
            (self.sleep)(DISABLED_PERIOD);
            return;
        }

        // A disconnected joystick has no pressed buttons. Thus a button held while the joystick
        // connects again counts as a new press.
        let buttons = self.joystick.buttons(config.joystick).unwrap_or(0);
        for direction in self.detector.update(buttons, config) {
            self.send(direction, config);
        }
        (self.sleep)(POLL_PERIOD);
    }

    fn send(&mut self, direction: Direction, config: &WeightJackerConfig) {
        let delay = Duration::from_millis(u64::from(config.delay_ms));
        for press in 0..config.presses {
            if press > 0 {
                (self.sleep)(delay);
            }
            self.keyboard.tap(direction);
        }
    }
}

/// Starts the worker on its own thread. The worker reads `config` before each poll, so a change in
/// the settings applies at once.
#[cfg(windows)]
pub fn spawn(config: std::sync::Arc<std::sync::Mutex<WeightJackerConfig>>) {
    std::thread::spawn(move || {
        let mut worker = Worker::new(
            windows_input::WinmmJoystick,
            windows_input::ScanCodeKeyboard,
            std::thread::sleep,
        );
        loop {
            let current = *config.lock().unwrap();
            worker.poll(&current);
        }
    });
}

#[cfg(windows)]
mod windows_input {
    use super::{Direction, Joystick, Keyboard};
    use core::mem::size_of;
    use windows::Win32::Media::Multimedia::{
        JOY_RETURNBUTTONS, JOYERR_NOERROR, JOYINFOEX, joyGetPosEx,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
        KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY,
    };

    /// Reads the joystick with the winmm API, which has the same joystick numbers as AutoHotkey.
    pub struct WinmmJoystick;

    impl Joystick for WinmmJoystick {
        fn buttons(&mut self, joystick: u32) -> Option<u32> {
            // winmm numbers the joysticks from 0.
            let id = joystick.checked_sub(1)?;
            let mut info = JOYINFOEX {
                dwSize: size_of::<JOYINFOEX>() as u32,
                dwFlags: JOY_RETURNBUTTONS as u32,
                ..Default::default()
            };
            // SAFETY: `info` is a valid JOYINFOEX with the correct `dwSize`.
            let result = unsafe { joyGetPosEx(id, &mut info) };
            (result == JOYERR_NOERROR).then_some(info.dwButtons)
        }
    }

    /// Sends the keys as scan codes, because iRacing does not see virtual keys.
    pub struct ScanCodeKeyboard;

    impl Keyboard for ScanCodeKeyboard {
        fn tap(&mut self, direction: Direction) {
            let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: direction.scan_code(),
                        dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            };
            let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
            // SAFETY: The inputs are plain keyboard events, and `cbsize` is the size of INPUT.
            unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const UP_MASK: u32 = 1 << 7; // Button 8.
    const DOWN_MASK: u32 = 1 << 2; // Button 3.

    fn enabled() -> WeightJackerConfig {
        WeightJackerConfig {
            enabled: true,
            ..WeightJackerConfig::default()
        }
    }

    /// One event that the fake devices record, in order.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Event {
        Tap(Direction),
        Sleep(Duration),
    }

    /// A joystick that gives the button masks of a list, one for each read. `None` is a
    /// disconnected joystick.
    struct FakeJoystick {
        reads: std::vec::IntoIter<Option<u32>>,
        joysticks_read: Vec<u32>,
    }

    impl Joystick for FakeJoystick {
        fn buttons(&mut self, joystick: u32) -> Option<u32> {
            self.joysticks_read.push(joystick);
            self.reads.next().flatten()
        }
    }

    struct FakeKeyboard<'a>(&'a std::cell::RefCell<Vec<Event>>);

    impl Keyboard for FakeKeyboard<'_> {
        fn tap(&mut self, direction: Direction) {
            self.0.borrow_mut().push(Event::Tap(direction));
        }
    }

    /// Polls one time for each read, and returns the events and the joysticks that were read.
    fn run(config: &WeightJackerConfig, reads: Vec<Option<u32>>) -> (Vec<Event>, Vec<u32>) {
        let events = std::cell::RefCell::new(Vec::new());
        let polls = reads.len();
        let joystick = FakeJoystick {
            reads: reads.into_iter(),
            joysticks_read: Vec::new(),
        };
        let mut worker = Worker::new(joystick, FakeKeyboard(&events), |duration| {
            events.borrow_mut().push(Event::Sleep(duration));
        });
        for _ in 0..polls {
            worker.poll(config);
        }
        let joysticks_read = worker.joystick.joysticks_read.clone();
        drop(worker);
        (events.into_inner(), joysticks_read)
    }

    fn taps(events: &[Event]) -> Vec<Direction> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Tap(d) => Some(*d),
                Event::Sleep(_) => None,
            })
            .collect()
    }

    #[test]
    fn defaults_are_the_values_of_the_pulover_macro() {
        let config = WeightJackerConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.joystick, 2);
        assert_eq!(config.up_button, 8);
        assert_eq!(config.down_button, 3);
        assert_eq!(config.presses, 5);
        assert_eq!(config.delay_ms, 50);
    }

    #[test]
    fn button_numbers_start_at_one() {
        assert_eq!(ButtonState::from_mask(0b1, 1), ButtonState::Pressed);
        assert_eq!(ButtonState::from_mask(0b1, 2), ButtonState::Released);
        assert_eq!(ButtonState::from_mask(1 << 31, 32), ButtonState::Pressed);
    }

    #[test]
    fn button_numbers_outside_the_range_are_never_pressed() {
        assert_eq!(ButtonState::from_mask(u32::MAX, 0), ButtonState::Released);
        assert_eq!(
            ButtonState::from_mask(u32::MAX, MAX_BUTTON + 1),
            ButtonState::Released
        );
        assert_eq!(
            ButtonState::from_mask(u32::MAX, u32::MAX),
            ButtonState::Released
        );
    }

    #[test]
    fn a_held_button_gives_one_press() {
        let config = enabled();
        let mut detector = PressDetector::default();

        assert_eq!(
            detector.update(UP_MASK, &config).collect::<Vec<_>>(),
            [Direction::Up]
        );
        assert_eq!(detector.update(UP_MASK, &config).count(), 0);
        assert_eq!(detector.update(0, &config).count(), 0);
        assert_eq!(
            detector.update(UP_MASK, &config).collect::<Vec<_>>(),
            [Direction::Up]
        );
    }

    #[test]
    fn both_buttons_at_the_same_time_give_both_presses() {
        let config = enabled();
        let mut detector = PressDetector::default();

        assert_eq!(
            detector
                .update(UP_MASK | DOWN_MASK, &config)
                .collect::<Vec<_>>(),
            [Direction::Up, Direction::Down]
        );
    }

    #[test]
    fn other_buttons_give_no_press() {
        let config = enabled();
        let mut detector = PressDetector::default();

        assert_eq!(detector.update(!(UP_MASK | DOWN_MASK), &config).count(), 0);
    }

    #[test]
    fn one_press_sends_the_key_with_the_delay_between_presses() {
        let (events, joysticks_read) = run(&enabled(), vec![Some(UP_MASK)]);

        let delay = Event::Sleep(Duration::from_millis(50));
        let tap = Event::Tap(Direction::Up);
        assert_eq!(
            events,
            [
                tap,
                delay,
                tap,
                delay,
                tap,
                delay,
                tap,
                delay,
                tap,
                Event::Sleep(POLL_PERIOD),
            ]
        );
        assert_eq!(joysticks_read, [2]);
    }

    #[test]
    fn the_down_button_sends_the_down_key() {
        let (events, _) = run(&enabled(), vec![Some(DOWN_MASK)]);
        assert_eq!(taps(&events), [Direction::Down; 5]);
    }

    #[test]
    fn nothing_is_sent_when_the_function_is_off() {
        let (events, joysticks_read) = run(
            &WeightJackerConfig::default(),
            vec![Some(UP_MASK), Some(0), Some(UP_MASK)],
        );

        assert!(taps(&events).is_empty());
        assert!(joysticks_read.is_empty());
        assert_eq!(events, [Event::Sleep(DISABLED_PERIOD); 3]);
    }

    #[test]
    fn a_disconnected_joystick_releases_its_buttons() {
        let (events, _) = run(&enabled(), vec![Some(UP_MASK), None, Some(UP_MASK)]);
        assert_eq!(taps(&events), [Direction::Up; 10]);
    }

    #[test]
    fn zero_presses_sends_nothing() {
        let config = WeightJackerConfig {
            presses: 0,
            ..enabled()
        };
        let (events, _) = run(&config, vec![Some(UP_MASK)]);
        assert_eq!(events, [Event::Sleep(POLL_PERIOD)]);
    }

    #[test]
    fn config_without_the_key_loads_the_defaults() {
        let config: WeightJackerConfig = toml::from_str("enabled = true").unwrap();
        assert_eq!(config, enabled());
    }

    proptest! {
        /// For each list of button masks, each direction gets exactly one burst for each change of
        /// its button from released to pressed.
        #[test]
        fn one_burst_for_each_new_press(
            masks in proptest::collection::vec(any::<u32>(), 0..64),
            presses in 0u32..8,
        ) {
            let config = WeightJackerConfig { presses, ..enabled() };
            let reads = masks.iter().copied().map(Some).collect();
            let (events, _) = run(&config, reads);

            let new_presses = |mask: u32| {
                let mut previous = false;
                masks
                    .iter()
                    .filter(|m| {
                        let now = *m & mask != 0;
                        let rising = now && !previous;
                        previous = now;
                        rising
                    })
                    .count() as u32
            };
            let sent = taps(&events);
            let ups = sent.iter().filter(|d| **d == Direction::Up).count() as u32;
            let downs = sent.iter().filter(|d| **d == Direction::Down).count() as u32;

            prop_assert_eq!(ups, new_presses(UP_MASK) * presses);
            prop_assert_eq!(downs, new_presses(DOWN_MASK) * presses);
        }
    }
}
