// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed single-chord Wayland output. No dynamic library paths, IPC or HID I/O.
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};

unsafe extern "C" {
    fn scrollmaster_keyboard_open() -> *mut c_void;
    fn scrollmaster_keyboard_state(handle: *mut c_void, mask: libc::c_uint) -> libc::c_int;
    fn scrollmaster_keyboard_action(
        handle: *mut c_void,
        key: libc::c_uint,
        mask: libc::c_uint,
    ) -> libc::c_int;
    fn scrollmaster_keyboard_action_valid(key: libc::c_uint, mask: libc::c_uint) -> libc::c_int;
    fn scrollmaster_keyboard_close_checked(handle: *mut c_void) -> libc::c_int;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeldModifiers {
    #[default]
    None,
    Ctrl,
    Alt,
    CtrlAlt,
}
impl HeldModifiers {
    fn mask(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Ctrl => 4,
            Self::Alt => 8,
            Self::CtrlAlt => 12,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardAction {
    key: u16,
    modifiers: u8,
}
fn named_key(name: &str) -> Option<u16> {
    if let Some(letter) = name
        .strip_prefix("Key")
        .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_uppercase())
    {
        return Some(
            [
                30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22,
                47, 17, 45, 21, 44,
            ][(letter.as_bytes()[0] - b'A') as usize],
        );
    }
    if let Some(digit) = name
        .strip_prefix("Digit")
        .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit())
    {
        let n = digit.as_bytes()[0] - b'0';
        return Some(if n == 0 { 11 } else { u16::from(n) + 1 });
    }
    if let Some(number) = name
        .strip_prefix('F')
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| (1..=12).contains(n) && name == format!("F{n}"))
    {
        return Some([59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88][number - 1]);
    }
    Some(match name {
        "Escape" => 1,
        "Tab" => 15,
        "Enter" => 28,
        "Space" => 57,
        "Backspace" => 14,
        "Delete" => 111,
        "Insert" => 110,
        "Home" => 102,
        "End" => 107,
        "PageUp" => 104,
        "PageDown" => 109,
        "ArrowUp" => 103,
        "ArrowDown" => 108,
        "ArrowLeft" => 105,
        "ArrowRight" => 106,
        _ => return None,
    })
}
impl KeyboardAction {
    /// Same canonical keyboard identifiers as profile schemas 4/7/9. Desktop
    /// dispatchers are deliberately a different output layer and are rejected.
    pub fn parse(action: &str) -> Result<Self, String> {
        if action.len() > 80 {
            return Err("Keyboard action is too long".into());
        }
        let fixed = match action {
            "shortcut:copy" => Some((46, 4)),
            "shortcut:paste" => Some((47, 4)),
            "shortcut:cut" => Some((45, 4)),
            "shortcut:undo" => Some((44, 4)),
            "media:play-pause" => Some((164, 0)),
            "media:previous" => Some((165, 0)),
            "media:next" => Some((163, 0)),
            "media:volume-up" => Some((115, 0)),
            "media:volume-down" => Some((114, 0)),
            "media:mute" => Some((113, 0)),
            _ => None,
        };
        let (key, modifiers) = if let Some(fixed) = fixed {
            fixed
        } else {
            let parts: Vec<_> = action.split(':').collect();
            if action.len() > 80 || parts.len() != 3 || parts[0] != "custom" {
                return Err("Unknown keyboard action".into());
            }
            let key = named_key(parts[2]).ok_or("Unknown shortcut key")?;
            let modifiers: Vec<_> = if parts[1].is_empty() {
                vec![]
            } else {
                parts[1].split('+').collect()
            };
            let catalogue = [("ctrl", 4u8), ("alt", 8), ("shift", 1), ("super", 64)];
            let expected: Vec<_> = catalogue
                .iter()
                .filter(|(name, _)| modifiers.contains(name))
                .map(|(name, _)| *name)
                .collect();
            if modifiers != expected {
                return Err(
                    "Expected canonical Ctrl/Alt/Shift/Super modifiers without duplicates".into(),
                );
            }
            let mask = catalogue
                .iter()
                .filter(|(name, _)| modifiers.contains(name))
                .map(|(_, mask)| mask)
                .copied()
                .sum();
            (key, mask)
        };
        // SAFETY: this pure C validator does not allocate or connect to Wayland.
        if unsafe { scrollmaster_keyboard_action_valid(u32::from(key), u32::from(modifiers)) } == 0
        {
            return Err("Keyboard action rejected by native boundary".into());
        }
        Ok(Self { key, modifiers })
    }
    pub fn code(&self) -> (u16, u8) {
        (self.key, self.modifiers)
    }
}

trait Driver {
    fn state(&mut self, mask: u8) -> Result<(), String>;
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String>;
    fn close(&mut self) -> Result<(), String>;
}
struct FfiDriver(Option<NonNull<c_void>>);
impl Driver for FfiDriver {
    fn state(&mut self, mask: u8) -> Result<(), String> {
        let handle = self.0.ok_or("Keyboard is closed")?;
        // SAFETY: a unique live C allocation, used only on this owning thread.
        if unsafe { scrollmaster_keyboard_state(handle.as_ptr(), u32::from(mask)) } != 0 {
            return Err("Wayland modifier state was not confirmed".into());
        }
        Ok(())
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        let handle = self.0.ok_or("Keyboard is closed")?;
        // SAFETY: action was validated in Rust and is revalidated by C.
        if unsafe {
            scrollmaster_keyboard_action(
                handle.as_ptr(),
                u32::from(action.key),
                u32::from(action.modifiers),
            )
        } != 0
        {
            return Err("Wayland key press/release was not confirmed".into());
        }
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        if let Some(handle) = self.0.take() {
            // SAFETY: take makes this the only call freeing the allocation, even
            // when the compositor does not acknowledge key/modifier release.
            if unsafe { scrollmaster_keyboard_close_checked(handle.as_ptr()) } != 0 {
                return Err("Wayland final key/modifier release was not confirmed".into());
            }
        }
        Ok(())
    }
}
impl Drop for FfiDriver {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

struct Keyboard<D: Driver> {
    driver: Option<D>,
    held: HeldModifiers,
    error: Option<String>,
    closed: Option<Result<(), String>>,
}
impl<D: Driver> Keyboard<D> {
    fn new(driver: D) -> Self {
        Self {
            driver: Some(driver),
            held: HeldModifiers::None,
            error: None,
            closed: None,
        }
    }
    fn state(&mut self, held: HeldModifiers) -> Result<(), String> {
        let driver = self.driver.as_mut().ok_or("Keyboard is closed")?;
        if self.held == held {
            return Ok(());
        }
        let result = driver.state(held.mask());
        self.complete(result)?;
        self.held = held;
        Ok(())
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        let result = self
            .driver
            .as_mut()
            .ok_or("Keyboard is closed")?
            .tap(action);
        self.complete(result)
    }
    fn complete(&mut self, result: Result<(), String>) -> Result<(), String> {
        if let Err(error) = &result {
            self.error = Some(error.clone());
            let _ = self.close();
        }
        result
    }
    fn close(&mut self) -> Result<(), String> {
        if let Some(result) = &self.closed {
            return result.clone();
        }
        let result = self
            .driver
            .take()
            .map_or(Ok(()), |mut driver| driver.close());
        self.held = HeldModifiers::None;
        self.closed = Some(result.clone());
        result
    }
}
impl<D: Driver> Drop for Keyboard<D> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Owns a virtual keyboard on one thread. All failures close it; no implicit
/// reopening/retry. Drop releases pending keys and zeroes our own modifiers.
/// Confirmation is by compositor roundtrip, not acceptance by the focused app.
pub struct WaylandKeyboard {
    keyboard: Keyboard<FfiDriver>,
    _same_thread: PhantomData<Rc<()>>,
}
impl WaylandKeyboard {
    pub fn open() -> Result<Self, String> {
        if unsafe { libc::getuid() } == 0 {
            return Err("Do not open the virtual keyboard as root".into());
        }
        // SAFETY: C returns null or an exclusive initialized allocation; open
        // confirms its zero modifier state before handing ownership to Rust.
        let handle = NonNull::new(unsafe { scrollmaster_keyboard_open() })
            .ok_or("No authorized Wayland virtual keyboard or compositor confirmation")?;
        Ok(Self {
            keyboard: Keyboard::new(FfiDriver(Some(handle))),
            _same_thread: PhantomData,
        })
    }
    pub fn set_held(&mut self, held: HeldModifiers) -> Result<(), String> {
        self.keyboard.state(held)
    }
    pub fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        self.keyboard.tap(action)
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.keyboard.close()
    }
    pub fn is_open(&self) -> bool {
        self.keyboard.driver.is_some()
    }
    pub fn last_error(&self) -> Option<&str> {
        self.keyboard.error.as_deref().or_else(|| {
            self.keyboard
                .closed
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .map(String::as_str)
        })
    }
}

pub(super) use super::events::EventSink as GuardianKeyboard;
impl GuardianKeyboard for WaylandKeyboard {
    fn close(&mut self) -> Result<(), String> {
        self.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Mock {
        events: Rc<RefCell<Vec<String>>>,
        state_error: bool,
        tap_error: bool,
        close_error: bool,
    }
    impl Driver for Mock {
        fn state(&mut self, mask: u8) -> Result<(), String> {
            self.events.borrow_mut().push(format!("state:{mask}"));
            if self.state_error {
                Err("state failed".into())
            } else {
                Ok(())
            }
        }
        fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
            self.events
                .borrow_mut()
                .push(format!("tap:{}:{}", action.key, action.modifiers));
            if self.tap_error {
                Err("tap failed".into())
            } else {
                Ok(())
            }
        }
        fn close(&mut self) -> Result<(), String> {
            self.events.borrow_mut().push("release-and-close".into());
            if self.close_error {
                Err("release failed".into())
            } else {
                Ok(())
            }
        }
    }
    fn fixture(
        state_error: bool,
        tap_error: bool,
        close_error: bool,
    ) -> (Keyboard<Mock>, Rc<RefCell<Vec<String>>>) {
        let events = Rc::new(RefCell::new(vec![]));
        (
            Keyboard::new(Mock {
                events: events.clone(),
                state_error,
                tap_error,
                close_error,
            }),
            events,
        )
    }
    #[test]
    fn fixed_editing_and_media_catalogue_matches_native_key_codes() {
        for (id, key, modifiers) in [
            ("shortcut:copy", 46, 4),
            ("shortcut:paste", 47, 4),
            ("shortcut:cut", 45, 4),
            ("shortcut:undo", 44, 4),
            ("media:play-pause", 164, 0),
            ("media:previous", 165, 0),
            ("media:next", 163, 0),
            ("media:volume-up", 115, 0),
            ("media:volume-down", 114, 0),
            ("media:mute", 113, 0),
        ] {
            assert_eq!(KeyboardAction::parse(id).unwrap().code(), (key, modifiers));
        }
    }
    #[test]
    fn all_named_keys_and_modifier_combinations_are_accepted_by_real_c_boundary() {
        let mut keys: Vec<_> = ('A'..='Z').map(|c| format!("Key{c}")).collect();
        keys.extend((0..=9).map(|d| format!("Digit{d}")));
        keys.extend((1..=12).map(|n| format!("F{n}")));
        keys.extend(
            [
                "Escape",
                "Tab",
                "Enter",
                "Space",
                "Backspace",
                "Delete",
                "Insert",
                "Home",
                "End",
                "PageUp",
                "PageDown",
                "ArrowUp",
                "ArrowDown",
                "ArrowLeft",
                "ArrowRight",
            ]
            .map(str::to_string),
        );
        let modifiers = [("ctrl", 4), ("alt", 8), ("shift", 1), ("super", 64)];
        let mut codes = std::collections::BTreeSet::new();
        for name in keys {
            for selection in 0..16 {
                let chosen: Vec<_> = modifiers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| selection & (1 << i) != 0)
                    .map(|(_, m)| *m)
                    .collect();
                let text = chosen.iter().map(|(m, _)| *m).collect::<Vec<_>>().join("+");
                let action = KeyboardAction::parse(&format!("custom:{text}:{name}")).unwrap();
                assert_eq!(
                    action.modifiers,
                    chosen.iter().map(|(_, bit)| bit).sum::<u8>()
                );
                codes.insert(action.key);
            }
        }
        assert_eq!(codes.len(), 63);
        assert_eq!(
            KeyboardAction::parse("custom:ctrl+shift:KeyS")
                .unwrap()
                .code(),
            (31, 5)
        );
        assert_eq!(
            KeyboardAction::parse("custom::Digit0").unwrap().code(),
            (11, 0)
        );
        assert_eq!(
            KeyboardAction::parse("custom::F12").unwrap().code(),
            (88, 0)
        );
    }
    #[test]
    fn malformed_shortcuts_desktop_commands_and_unsafe_keys_are_rejected() {
        for id in [
            "",
            "copy",
            "media:power",
            "desktop:fullscreen",
            "shell:echo hi",
            "custom:alt+ctrl:KeyA",
            "custom:ctrl+ctrl:KeyA",
            "custom:ctrl+altgr:KeyA",
            "custom:Ctrl:KeyA",
            "custom:ctrl:Keya",
            "custom::F01",
            "custom::F13",
            "custom::Power",
            "custom::CapsLock",
            "custom::AltLeft",
            "custom::46",
            "custom::KeyA:extra",
            "custom:ctrl+:KeyA",
            "custom:ctrl:KeyA\0",
        ] {
            assert!(KeyboardAction::parse(id).is_err(), "{id:?}");
        }
        assert!(KeyboardAction::parse(&"x".repeat(1000)).is_err());
    }
    #[test]
    fn held_modifiers_are_confirmed_once_and_taps_preserve_held_state() {
        let (mut keyboard, events) = fixture(false, false, false);
        keyboard.state(HeldModifiers::CtrlAlt).unwrap();
        keyboard.state(HeldModifiers::CtrlAlt).unwrap();
        keyboard
            .tap(&KeyboardAction::parse("shortcut:copy").unwrap())
            .unwrap();
        assert_eq!(keyboard.held, HeldModifiers::CtrlAlt);
        keyboard.state(HeldModifiers::None).unwrap();
        keyboard.close().unwrap();
        drop(keyboard);
        assert_eq!(
            *events.borrow(),
            ["state:12", "tap:46:4", "state:0", "release-and-close"]
        );
    }
    #[test]
    fn modifier_failure_closes_before_returning_and_prevents_further_output() {
        let (mut keyboard, events) = fixture(true, false, false);
        assert!(keyboard.state(HeldModifiers::Alt).is_err());
        assert!(keyboard.driver.is_none());
        assert!(keyboard.state(HeldModifiers::None).is_err());
        assert!(keyboard
            .tap(&KeyboardAction::parse("media:mute").unwrap())
            .is_err());
        keyboard.close().unwrap();
        drop(keyboard);
        assert_eq!(*events.borrow(), ["state:8", "release-and-close"]);
    }
    #[test]
    fn tap_failure_preserves_original_error_and_reports_release_failure_separately() {
        let (mut keyboard, events) = fixture(false, true, true);
        keyboard.state(HeldModifiers::Ctrl).unwrap();
        assert_eq!(
            keyboard
                .tap(&KeyboardAction::parse("shortcut:paste").unwrap())
                .unwrap_err(),
            "tap failed"
        );
        assert_eq!(keyboard.error.as_deref(), Some("tap failed"));
        assert_eq!(keyboard.close().unwrap_err(), "release failed");
        assert_eq!(keyboard.held, HeldModifiers::None);
        drop(keyboard);
        assert_eq!(
            *events.borrow(),
            ["state:4", "tap:47:4", "release-and-close"]
        );
    }
    #[test]
    fn explicit_close_is_idempotent_even_if_release_was_not_confirmed() {
        let (mut keyboard, events) = fixture(false, false, true);
        assert_eq!(keyboard.close().unwrap_err(), "release failed");
        assert_eq!(keyboard.close().unwrap_err(), "release failed");
        drop(keyboard);
        assert_eq!(*events.borrow(), ["release-and-close"]);
    }
    #[test]
    fn dropping_an_open_keyboard_releases_its_held_modifiers() {
        let (mut keyboard, events) = fixture(false, false, false);
        keyboard.state(HeldModifiers::Alt).unwrap();
        drop(keyboard);
        assert_eq!(*events.borrow(), ["state:8", "release-and-close"]);
    }
}
