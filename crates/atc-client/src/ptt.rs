//! Push-to-talk inputs: a joystick button (gilrs, true hold-to-talk) and a terminal key
//! (crossterm raw mode, toggle: a terminal on Linux reports key presses but no releases, so
//! the first press starts the transmission and the next one ends it).

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PttEvent {
    Press,
    Release,
}

/// Raw input from the PTT sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PttInput {
    /// The configured joystick button went down (`true`) or up.
    Joystick(bool),
    /// The PTT key was tapped.
    KeyToggle,
    /// Ctrl-C, Esc or `q` in the terminal (raw mode swallows the interrupt signal).
    Quit,
}

/// Transmitting while the joystick button is held or the key toggle is on; reports the
/// edges of that.
#[derive(Debug, Default)]
pub struct PttMerger {
    joystick: bool,
    key: bool,
}

impl PttMerger {
    pub fn active(&self) -> bool {
        self.joystick || self.key
    }

    /// `None` for [`PttInput::Quit`] or when the transmit state does not change.
    pub fn on_input(&mut self, input: PttInput) -> Option<PttEvent> {
        let before = self.active();
        match input {
            PttInput::Joystick(down) => self.joystick = down,
            PttInput::KeyToggle => self.key = !self.key,
            PttInput::Quit => return None,
        }
        match (before, self.active()) {
            (false, true) => Some(PttEvent::Press),
            (true, false) => Some(PttEvent::Release),
            _ => None,
        }
    }

    /// Forgets the transmit state (the transmission was cut off, e.g. by a disconnect).
    pub fn reset(&mut self) {
        *self = PttMerger::default();
    }
}

/// `"space"`, `"enter"`, `"tab"` or a single character.
pub fn parse_key(name: &str) -> anyhow::Result<KeyCode> {
    let lower = name.trim().to_ascii_lowercase();
    Ok(match lower.as_str() {
        "space" => KeyCode::Char(' '),
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        _ => {
            let mut chars = lower.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => anyhow::bail!(
                    "unsupported PTT key {name:?} (use space, enter, tab or one character)"
                ),
            }
        }
    })
}

/// A held key auto-repeats as more presses, after an initial delay (Xorg 660 ms, KDE ~600 ms)
/// that is longer than any sensible tap gap. A press this soon after the *last* press event
/// of the key (repeats included, so a continuous burst stays one hold) is a repeat and does
/// not toggle; a new toggle needs a quiet gap of at least this long.
const REPEAT_GAP: Duration = Duration::from_millis(1000);

/// Turns terminal key events into [`PttInput`]s.
pub struct KeyMapper {
    key: KeyCode,
    last_press: Option<Instant>,
}

impl KeyMapper {
    pub fn new(key: KeyCode) -> Self {
        KeyMapper {
            key,
            last_press: None,
        }
    }

    pub fn on_key(&mut self, ev: &KeyEvent, now: Instant) -> Option<PttInput> {
        if ev.kind != KeyEventKind::Press {
            return None;
        }
        let ctrl_c = ev.modifiers.contains(KeyModifiers::CONTROL) && ev.code == KeyCode::Char('c');
        if ctrl_c
            || ev.code == KeyCode::Esc
            || (ev.code == KeyCode::Char('q') && self.key != ev.code)
        {
            return Some(PttInput::Quit);
        }
        if ev.code != self.key {
            return None;
        }
        let repeat = self
            .last_press
            .is_some_and(|t| now.duration_since(t) < REPEAT_GAP);
        self.last_press = Some(now);
        (!repeat).then_some(PttInput::KeyToggle)
    }
}

/// gilrs button code → button number as configured (`joystick_button`). On Linux the code is
/// `(EV_KEY << 16) | evdev code` and joystick buttons start at `BTN_TRIGGER` (0x120), so
/// button 0 is the trigger; elsewhere the platform's own code is used.
pub fn button_number(raw_code: u32) -> u32 {
    if cfg!(target_os = "linux") {
        let code = raw_code & 0xFFFF;
        code.checked_sub(0x120).unwrap_or(code)
    } else {
        raw_code
    }
}

/// Restores the terminal when dropped (and on panic, via the hook installed by
/// [`spawn_keyboard`]).
pub struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// Enables raw mode and reads terminal keys on a thread.
pub fn spawn_keyboard(key: KeyCode, tx: mpsc::Sender<PttInput>) -> anyhow::Result<RawModeGuard> {
    crossterm::terminal::enable_raw_mode()?;
    let guard = RawModeGuard;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        hook(info);
    }));
    std::thread::spawn(move || {
        let mut mapper = KeyMapper::new(key);
        loop {
            match crossterm::event::read() {
                Ok(crossterm::event::Event::Key(ev)) => {
                    tracing::debug!(code = ?ev.code, kind = ?ev.kind, "terminal key");
                    if let Some(input) = mapper.on_key(&ev, Instant::now())
                        && tx.blocking_send(input).is_err()
                    {
                        return;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("terminal input failed: {e}");
                    return;
                }
            }
        }
    });
    Ok(guard)
}

/// Watches all joysticks on a thread for `button`. No joystick support (gilrs init error) is
/// logged and leaves only the keyboard.
pub fn spawn_joystick(button: u32, tx: mpsc::Sender<PttInput>) {
    std::thread::spawn(move || {
        let mut gilrs = match gilrs::Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("joystick input unavailable: {e}");
                return;
            }
        };
        for (_, pad) in gilrs.gamepads() {
            tracing::info!("joystick: {}", pad.name());
        }
        loop {
            let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(200))) else {
                if tx.is_closed() {
                    return;
                }
                continue;
            };
            let (down, code) = match ev.event {
                gilrs::EventType::ButtonPressed(_, c) => (true, c),
                gilrs::EventType::ButtonReleased(_, c) => (false, c),
                gilrs::EventType::Connected => {
                    tracing::info!("joystick connected: {}", gilrs.gamepad(ev.id).name());
                    continue;
                }
                _ => continue,
            };
            let n = button_number(code.into_u32());
            let state = if down { "pressed" } else { "released" };
            tracing::debug!(raw = code.into_u32(), "joystick button {n} {state}");
            if n == button && tx.blocking_send(PttInput::Joystick(down)).is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn merger_edges() {
        use PttEvent::*;
        use PttInput::*;
        let mut m = PttMerger::default();
        assert_eq!(m.on_input(KeyToggle), Some(Press));
        assert_eq!(m.on_input(KeyToggle), Some(Release));
        assert_eq!(m.on_input(Joystick(true)), Some(Press));
        // the key toggling while the button is held changes nothing
        assert_eq!(m.on_input(KeyToggle), None);
        assert_eq!(m.on_input(KeyToggle), None);
        assert_eq!(m.on_input(Joystick(false)), Some(Release));
        assert_eq!(m.on_input(Joystick(false)), None);
        assert_eq!(m.on_input(Quit), None);
        m.on_input(KeyToggle);
        m.reset();
        assert!(!m.active());
    }

    #[test]
    fn key_toggle_ignores_auto_repeat() {
        let mut k = KeyMapper::new(parse_key("space").unwrap());
        let t0 = Instant::now();
        let space = press(KeyCode::Char(' '));
        assert_eq!(k.on_key(&space, t0), Some(PttInput::KeyToggle));
        // held: repeats every ~33 ms
        for i in 1..30 {
            assert_eq!(
                k.on_key(&space, t0 + Duration::from_millis(500 + 33 * i)),
                None
            );
        }
        let later = t0 + Duration::from_millis(500 + 33 * 29) + REPEAT_GAP;
        assert_eq!(k.on_key(&space, later), Some(PttInput::KeyToggle));
        assert_eq!(k.on_key(&press(KeyCode::Char('x')), later), None);
    }

    #[test]
    fn held_key_with_initial_repeat_delay_toggles_once() {
        let mut k = KeyMapper::new(KeyCode::Char(' '));
        let t0 = Instant::now();
        let space = press(KeyCode::Char(' '));
        let ms = Duration::from_millis;
        let mut toggles = vec![];
        if k.on_key(&space, t0).is_some() {
            toggles.push(0);
        }
        // Xorg: 660 ms initial delay, then a repeat every 33 ms for 3 s
        let mut t = 660;
        while t <= 660 + 3000 {
            if k.on_key(&space, t0 + ms(t)).is_some() {
                toggles.push(t);
            }
            t += 33;
        }
        let last = t - 33;
        // released; a deliberate second tap 1.5 s later ends the transmission
        let second = last + 1500;
        if k.on_key(&space, t0 + ms(second)).is_some() {
            toggles.push(second);
        }
        assert_eq!(
            toggles,
            [0, second],
            "start on the first press, end on the later tap"
        );
    }

    #[test]
    fn quit_keys() {
        let mut k = KeyMapper::new(KeyCode::Char(' '));
        let now = Instant::now();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(k.on_key(&ctrl_c, now), Some(PttInput::Quit));
        assert_eq!(k.on_key(&press(KeyCode::Esc), now), Some(PttInput::Quit));
        assert_eq!(
            k.on_key(&press(KeyCode::Char('q')), now),
            Some(PttInput::Quit)
        );
        let mut release = press(KeyCode::Char(' '));
        release.kind = KeyEventKind::Release;
        assert_eq!(k.on_key(&release, now), None);
    }

    #[test]
    fn key_names() {
        assert_eq!(parse_key("space").unwrap(), KeyCode::Char(' '));
        assert_eq!(parse_key("T").unwrap(), KeyCode::Char('t'));
        assert_eq!(parse_key("enter").unwrap(), KeyCode::Enter);
        assert!(parse_key("ctrl+space").is_err());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn linux_button_numbers() {
        assert_eq!(button_number((1 << 16) | 0x120), 0);
        assert_eq!(button_number((1 << 16) | 0x123), 3);
    }
}
