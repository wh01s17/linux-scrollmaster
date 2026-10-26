// SPDX-License-Identifier: GPL-3.0-or-later
//! N4 action state machine and typed output routing. No HID setters or profile I/O.
use super::desktop::{DesktopAction, Dispatcher, Hyprland};
use super::events::{Event, EventSink};
use super::{HeldModifiers, KeyboardAction, WaylandKeyboard};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

const PENDING_GESTURE_ACTION_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureDirection {
    Up,
    Down,
    Left,
    Right,
    Click,
}
impl GestureDirection {
    fn index(self) -> usize {
        self as usize
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GestureFeedback {
    pub sequence: u64,
    /// None means an ambiguous diagonal, with no action emitted.
    pub direction: Option<GestureDirection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TapAction {
    Keyboard(KeyboardAction),
    Desktop(DesktopAction),
}
impl TapAction {
    fn parse(value: &str) -> Result<Self, String> {
        if value.starts_with("desktop:") {
            DesktopAction::parse(value).map(Self::Desktop)
        } else {
            KeyboardAction::parse(value).map(Self::Keyboard)
        }
    }
    /// Volume follows the movement while the gesture is held, like a slider.
    fn incremental(&self) -> bool {
        matches!(self, Self::Keyboard(action) if matches!(action.code(), (114, 0) | (115, 0)))
    }
    fn emit(&self, output: &mut impl Output) -> Result<(), String> {
        match self {
            Self::Keyboard(action) => output.tap(action),
            Self::Desktop(action) => output.desktop(*action),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum SideAction {
    Ctrl,
    Alt,
    Tap(TapAction),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct GestureConfig {
    threshold: u16,
    physical: bool,
    actions: [Option<TapAction>; 5],
}
impl GestureConfig {
    fn parse(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("Expected a gesture object")?;
        if ![3, 4].contains(&object.len())
            || object
                .keys()
                .any(|k| !["type", "threshold", "units", "actions"].contains(&k.as_str()))
            || value["type"] != "gestures"
        {
            return Err("Invalid gesture configuration fields".into());
        }
        let physical = match object.get("units").and_then(Value::as_str) {
            None if !object.contains_key("units") => false,
            Some("counts") => false,
            Some("tenths-mm") => true,
            _ => return Err("Invalid gesture distance units".into()),
        };
        let threshold = value["threshold"]
            .as_u64()
            .filter(|n| {
                if physical {
                    (10..=200).contains(n)
                } else {
                    (20..=2000).contains(n)
                }
            })
            .ok_or("Gesture distance outside permitted range")? as u16;
        let object = value["actions"]
            .as_object()
            .ok_or("Missing gesture actions")?;
        let names = ["up", "down", "left", "right", "click"];
        if object.len() != names.len() || names.iter().any(|k| !object.contains_key(*k)) {
            return Err("Expected all five gesture directions".into());
        }
        let mut actions = std::array::from_fn(|_| None);
        for (i, name) in names.iter().enumerate() {
            let action = &object[*name];
            if !action.is_null() {
                actions[i] = Some(TapAction::parse(
                    action.as_str().ok_or("Invalid gesture action")?,
                )?);
            }
        }
        Ok(Self {
            threshold,
            physical,
            actions,
        })
    }
    fn counts(&self, dpi: Option<u16>) -> Result<i32, String> {
        if self.physical {
            let dpi = dpi
                .filter(|v| *v != 0)
                .ok_or("Physical gestures require a current DPI reading")?;
            Ok((u32::from(self.threshold) * u32::from(dpi)).div_ceil(254) as i32)
        } else {
            Ok(i32::from(self.threshold))
        }
    }
}

/// Closed CID-to-action map. Omitted CIDs emit no actions. Parsing performs no
/// I/O and never accepts shell commands or arbitrary compositor dispatchers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionBindings {
    sides: [Option<SideAction>; 2],
    gesture: Option<GestureConfig>,
}
impl ActionBindings {
    pub fn uses_desktop(&self) -> bool {
        self.sides
            .iter()
            .any(|s| matches!(s, Some(SideAction::Tap(TapAction::Desktop(_)))))
            || self.gesture.as_ref().is_some_and(|g| {
                g.actions
                    .iter()
                    .any(|a| matches!(a, Some(TapAction::Desktop(_))))
            })
    }
    /// Secondary controls explicitly assigned by this closed action map.
    pub fn controls(&self) -> Vec<u16> {
        [
            (83, self.sides[0].is_some()),
            (86, self.sides[1].is_some()),
            (195, self.gesture.is_some()),
        ]
        .into_iter()
        .filter_map(|(cid, bound)| bound.then_some(cid))
        .collect()
    }
    pub fn parse(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("Expected button bindings")?;
        if object.len() > 3
            || object
                .keys()
                .any(|cid| !["83", "86", "195"].contains(&cid.as_str()))
        {
            return Err("Only secondary CIDs 83, 86 and 195 may have actions".into());
        }
        let mut result = Self::default();
        for (i, cid) in ["83", "86"].iter().enumerate() {
            if let Some(action) = object.get(*cid) {
                result.sides[i] =
                    Some(match action.as_str().ok_or("Invalid side button action")? {
                        "modifier:ctrl" => SideAction::Ctrl,
                        "modifier:alt" => SideAction::Alt,
                        action => SideAction::Tap(TapAction::parse(action)?),
                    });
            }
        }
        if let Some(config) = object.get("195") {
            result.gesture = Some(GestureConfig::parse(config)?);
        }
        Ok(result)
    }
    fn validate_dpi(&self, dpi: Option<u16>) -> Result<(), String> {
        if dpi == Some(0) {
            return Err("Invalid zero DPI reading".into());
        }
        if let Some(config) = &self.gesture {
            config.counts(dpi)?;
        }
        Ok(())
    }
    fn validate_controls(&self, snapshot: &Value) -> Result<(), String> {
        let controls = snapshot["controls"]
            .as_array()
            .ok_or("Missing action control catalogue")?;
        for (cid, required) in [
            ("83", self.sides[0].is_some()),
            ("86", self.sides[1].is_some()),
            ("195", self.gesture.is_some()),
        ] {
            if !required {
                continue;
            }
            let matches: Vec<_> = controls
                .iter()
                .filter(|control| control["id"] == cid)
                .collect();
            if matches.len() != 1 {
                return Err("Action control missing or duplicated in device catalogue".into());
            }
            let flags = matches[0]["capabilities"]["flags"]
                .as_u64()
                .ok_or("Missing action capabilities")?;
            if flags & 0x31 != 0x31 || flags & 0x80 != 0 || (cid == "195" && flags & 0x100 == 0) {
                return Err("Device does not support the requested action capture".into());
            }
        }
        Ok(())
    }
    pub(super) fn validate_snapshot(&self, snapshot: &Value) -> Result<Option<u16>, String> {
        self.validate_controls(snapshot)?;
        let dpi = snapshot["snapshot"]["device"]["settings"]["dpi"]["value"]
            .as_u64()
            .and_then(|dpi| u16::try_from(dpi).ok())
            .filter(|dpi| *dpi != 0);
        self.validate_dpi(dpi)?;
        Ok(dpi)
    }
}

pub(super) trait Output {
    fn held(&mut self, value: HeldModifiers) -> Result<(), String>;
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String>;
    fn require_desktop(&mut self) -> Result<(), String> {
        Err("Desktop output is unavailable".into())
    }
    fn desktop(&mut self, _action: DesktopAction) -> Result<(), String> {
        Err("Desktop output is unavailable".into())
    }
    fn close(&mut self) -> Result<(), String>;
}

struct NativeOutput<K: Output, D: Dispatcher> {
    keyboard: K,
    desktop: D,
}
impl<K: Output, D: Dispatcher> Output for NativeOutput<K, D> {
    fn held(&mut self, value: HeldModifiers) -> Result<(), String> {
        self.keyboard.held(value)
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        self.keyboard.tap(action)
    }
    fn require_desktop(&mut self) -> Result<(), String> {
        self.desktop.require_actions()
    }
    fn desktop(&mut self, action: DesktopAction) -> Result<(), String> {
        self.desktop.dispatch(action)
    }
    fn close(&mut self) -> Result<(), String> {
        self.keyboard.close()
    }
}
impl Output for WaylandKeyboard {
    fn held(&mut self, value: HeldModifiers) -> Result<(), String> {
        self.set_held(value)
    }
    fn tap(&mut self, action: &KeyboardAction) -> Result<(), String> {
        self.tap(action)
    }
    fn close(&mut self) -> Result<(), String> {
        self.close()
    }
}

struct Stroke {
    started: Duration,
    /// Speed-weighted totals since the press; the peak separates a click.
    x: f64,
    y: f64,
    peak: f64,
    /// Speed-weighted movement since the last lock, axis change or step.
    ax: f64,
    ay: f64,
    /// Locked direction; its action already ran live (nothing on release).
    direction: Option<GestureDirection>,
    /// The next raw XY after this press is the 1B04 v5+ anomalous report.
    initial_raw: bool,
}
/// Speed-weighted travel: every report adds `sign(d)·|d|^1.4`, so fast movement
/// counts more and slow movement less. Normalized to 1000 DPI so the configured
/// distance stays stable when the DPI changes.
fn weighted(delta: i16, dpi: Option<u16>) -> f64 {
    let d = f64::from(delta);
    let scale = dpi.map_or(1.0, |dpi| 1000.0 / f64::from(dpi));
    d * (d.abs() * scale).powf(0.4)
}
/// Dominant axis (1.2x the other) once it reaches the distance.
fn dominant(x: f64, y: f64, threshold: f64) -> Option<GestureDirection> {
    let (ax, ay) = (x.abs(), y.abs());
    if ax >= threshold && ax * 5.0 >= ay * 6.0 {
        Some(if x > 0.0 {
            GestureDirection::Right
        } else {
            GestureDirection::Left
        })
    } else if ay >= threshold && ay * 5.0 >= ax * 6.0 {
        Some(if y > 0.0 {
            GestureDirection::Down
        } else {
            GestureDirection::Up
        })
    } else {
        None
    }
}
fn vertical(direction: GestureDirection) -> bool {
    matches!(direction, GestureDirection::Up | GestureDirection::Down)
}
pub(super) struct Runtime<O: Output> {
    output: Option<O>,
    bindings: ActionBindings,
    dpi: Option<u16>,
    pressed: BTreeSet<u16>,
    suppressed: BTreeSet<u16>,
    synchronized: bool,
    held: HeldModifiers,
    stroke: Option<Stroke>,
    feedback: Option<GestureFeedback>,
    last_time: Duration,
    error: Option<String>,
    closed: Option<Result<(), String>>,
    start: Instant,
    query_pending: bool,
    // Provisional physical-gesture output, consumed only after a fresh snapshot
    // confirms the same DPI. Side buttons continue to run synchronously.
    pending_gesture_actions: Vec<TapAction>,
    /// From the last device snapshot; unknown until one is validated.
    skip_initial_raw: Option<bool>,
}
/// 1B04 version >= 5 (MX Master 3S) sends motion accumulated before the
/// press as the first raw XY of each gesture; the legacy adapter skipped it.
/// Measured on the reference mouse: 6 of 16 recorded strokes changed direction with it.
fn initial_raw_quirk(snapshot: &Value) -> Option<bool> {
    snapshot["feature"]["version"]
        .as_u64()
        .filter(|v| (2..=5).contains(v))
        .map(|v| v >= 5)
}
impl<O: Output> Runtime<O> {
    pub(super) fn new(
        mut output: O,
        bindings: ActionBindings,
        dpi: Option<u16>,
    ) -> Result<Self, String> {
        let prepared = bindings.validate_dpi(dpi).and_then(|()| {
            if bindings.uses_desktop() {
                output.require_desktop()
            } else {
                Ok(())
            }
        });
        if let Err(error) = prepared {
            return Err(match output.close() {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; keyboard cleanup: {cleanup}"),
            });
        }
        Ok(Self {
            output: Some(output),
            bindings,
            dpi,
            pressed: BTreeSet::new(),
            suppressed: BTreeSet::new(),
            synchronized: false,
            held: HeldModifiers::None,
            stroke: None,
            feedback: None,
            last_time: Duration::ZERO,
            error: None,
            closed: None,
            start: Instant::now(),
            query_pending: false,
            pending_gesture_actions: Vec::new(),
            skip_initial_raw: None,
        })
    }
    fn check_time(&mut self, now: Duration) -> Result<(), String> {
        if self.output.is_none() {
            return Err("Action output is closed".into());
        }
        if now < self.last_time {
            return Err("Action clock moved backwards".into());
        }
        self.last_time = now;
        Ok(())
    }
    fn finish(&mut self, result: Result<(), String>) -> Result<(), String> {
        if let Err(error) = &result {
            if self.error.is_none() {
                self.error = Some(error.clone());
            }
            let _ = self.close();
        }
        result
    }
    fn buttons(&mut self, held: &[u16], now: Duration) -> Result<(), String> {
        let result = self.update_buttons(held, now);
        self.finish(result)
    }
    fn update_buttons(&mut self, held: &[u16], now: Duration) -> Result<(), String> {
        self.check_time(now)?;
        if held.len() > 3 || held.iter().any(|cid| ![83, 86, 195].contains(cid)) {
            return Err("Invalid action notification controls".into());
        }
        let next: BTreeSet<_> = held.iter().copied().collect();
        if next.len() != held.len() {
            return Err("Duplicate action notification control".into());
        }
        if !self.synchronized {
            self.pressed = next;
            self.suppressed = self.pressed.clone();
            self.synchronized = true;
            return Ok(());
        }
        self.suppressed.retain(|cid| next.contains(cid));
        let newly_pressed: BTreeSet<_> = next
            .difference(&self.pressed)
            .filter(|cid| !self.suppressed.contains(cid))
            .copied()
            .collect();
        let released_gesture = self.pressed.contains(&195) && !next.contains(&195);
        self.pressed = next;
        self.update_modifiers()?;
        // Consume edges before output; a failed/uncertain tap is never retried.
        for (i, cid) in [83, 86].iter().enumerate() {
            if newly_pressed.contains(cid) {
                if let Some(SideAction::Tap(action)) = &self.bindings.sides[i] {
                    action.emit(self.output.as_mut().ok_or("Action output is closed")?)?;
                }
            }
        }
        if newly_pressed.contains(&195) {
            if let Some(config) = &self.bindings.gesture {
                config.counts(self.dpi)?;
                self.stroke = Some(Stroke {
                    started: now,
                    x: 0.0,
                    y: 0.0,
                    peak: 0.0,
                    ax: 0.0,
                    ay: 0.0,
                    direction: None,
                    initial_raw: self.skip_initial_raw == Some(true),
                });
            }
        }
        if released_gesture {
            self.release_gesture(now)?;
        }
        Ok(())
    }
    fn update_modifiers(&mut self) -> Result<(), String> {
        let (mut ctrl, mut alt) = (false, false);
        for (i, cid) in [83, 86].iter().enumerate() {
            if self.pressed.contains(cid) && !self.suppressed.contains(cid) {
                ctrl |= self.bindings.sides[i] == Some(SideAction::Ctrl);
                alt |= self.bindings.sides[i] == Some(SideAction::Alt);
            }
        }
        let held = match (ctrl, alt) {
            (false, false) => HeldModifiers::None,
            (true, false) => HeldModifiers::Ctrl,
            (false, true) => HeldModifiers::Alt,
            (true, true) => HeldModifiers::CtrlAlt,
        };
        if held != self.held {
            self.output
                .as_mut()
                .ok_or("Action output is closed")?
                .held(held)?;
            self.held = held;
        }
        Ok(())
    }
    fn motion(&mut self, dx: i16, dy: i16, now: Duration) -> Result<(), String> {
        let result = self.update_motion(dx, dy, now);
        self.finish(result)
    }
    fn update_motion(&mut self, dx: i16, dy: i16, now: Duration) -> Result<(), String> {
        self.check_time(now)?;
        let Some(stroke) = &mut self.stroke else {
            return Ok(());
        };
        if now - stroke.started > Duration::from_secs(10) {
            self.stroke = None;
            return Ok(());
        }
        let config = self
            .bindings
            .gesture
            .as_ref()
            .ok_or("Gesture configuration lost")?;
        let threshold = f64::from(config.counts(self.dpi)?);
        let (wx, wy) = (weighted(dx, self.dpi), weighted(dy, self.dpi));
        stroke.x = (stroke.x + wx).clamp(-1e9, 1e9);
        stroke.y = (stroke.y + wy).clamp(-1e9, 1e9);
        stroke.ax = (stroke.ax + wx).clamp(-1e9, 1e9);
        stroke.ay = (stroke.ay + wy).clamp(-1e9, 1e9);
        stroke.peak = stroke.peak.max(stroke.x.abs()).max(stroke.y.abs());
        // The action runs when the gesture starts, not on release.
        // The other axis may take over mid-gesture when it reaches the distance
        // and dominates the movement since the last action.
        let emit = match (stroke.direction, dominant(stroke.ax, stroke.ay, threshold)) {
            (None, Some(next)) => Some(next),
            (Some(current), Some(next)) if vertical(current) != vertical(next) => Some(next),
            (Some(current), _) => {
                // Continuous actions step every distance and restart the count;
                // volume steps every half distance.
                // Other actions run once per axis, but the window still restarts
                // so an axis change compares recent movement.
                let along = if vertical(current) {
                    stroke.ay
                } else {
                    stroke.ax
                };
                let incremental = config.actions[current.index()]
                    .as_ref()
                    .is_some_and(TapAction::incremental);
                let step = if incremental {
                    threshold / 2.0
                } else {
                    threshold
                };
                if along.abs() < step {
                    return Ok(());
                }
                stroke.ax = 0.0;
                stroke.ay = 0.0;
                let next = match (vertical(current), along > 0.0) {
                    (true, true) => GestureDirection::Down,
                    (true, false) => GestureDirection::Up,
                    (false, true) => GestureDirection::Right,
                    (false, false) => GestureDirection::Left,
                };
                let repeats = config.actions[next.index()]
                    .as_ref()
                    .is_some_and(TapAction::incremental);
                (incremental && repeats).then_some(next)
            }
            (None, None) => None,
        };
        let Some(next) = emit else {
            return Ok(());
        };
        stroke.direction = Some(next);
        stroke.ax = 0.0;
        stroke.ay = 0.0;
        if let Some(action) = config.actions[next.index()].clone() {
            self.emit_gesture(action)?;
        }
        Ok(())
    }
    fn release_gesture(&mut self, now: Duration) -> Result<(), String> {
        let Some(stroke) = self.stroke.take() else {
            return Ok(());
        };
        if now - stroke.started > Duration::from_secs(10) {
            return Ok(());
        }
        let config = self
            .bindings
            .gesture
            .as_ref()
            .ok_or("Gesture configuration lost")?;
        // A started gesture already ran its actions; a short press is a click.
        let click = stroke.direction.is_none() && stroke.peak < f64::from(config.counts(self.dpi)?);
        let direction = if click {
            Some(GestureDirection::Click)
        } else {
            stroke.direction
        };
        self.feedback = Some(GestureFeedback {
            sequence: self.feedback.map_or(1, |f| f.sequence.saturating_add(1)),
            direction,
        });
        if let Some(action) = config.actions[GestureDirection::Click.index()]
            .clone()
            .filter(|_| click)
        {
            self.emit_gesture(action)?;
        }
        Ok(())
    }
    fn emit_gesture(&mut self, action: TapAction) -> Result<(), String> {
        if self.query_pending && self.bindings.gesture.as_ref().is_some_and(|g| g.physical) {
            // A busy query cannot grow delayed output without bound.
            // Uncertain/failed snapshots never flush this queue.
            if self.pending_gesture_actions.len() >= PENDING_GESTURE_ACTION_LIMIT {
                return Err("Pending physical gesture action budget exhausted".into());
            }
            self.pending_gesture_actions.push(action);
            Ok(())
        } else {
            action.emit(self.output.as_mut().ok_or("Action output is closed")?)
        }
    }
    fn set_dpi(&mut self, dpi: Option<u16>) -> Result<(), String> {
        let result = (|| {
            if self.output.is_none() {
                return Err("Action output is closed".into());
            }
            if dpi == Some(0) {
                return Err("Invalid zero DPI reading".into());
            }
            if dpi != self.dpi && self.bindings.gesture.as_ref().is_some_and(|g| g.physical) {
                self.stroke = None;
                self.pending_gesture_actions.clear();
                if self.query_pending {
                    self.feedback = None;
                }
                if self.pressed.contains(&195) {
                    self.suppressed.insert(195);
                }
            }
            self.dpi = dpi;
            Ok(())
        })();
        self.finish(result)
    }
    fn replace_bindings(&mut self, bindings: ActionBindings) -> Result<(), String> {
        let result = (|| {
            if self.output.is_none() {
                return Err("Action output is closed".into());
            }
            bindings.validate_dpi(self.dpi)?;
            if bindings.uses_desktop() {
                self.output
                    .as_mut()
                    .ok_or("Action output is closed")?
                    .require_desktop()?;
            }
            for (i, cid) in [83, 86].iter().enumerate() {
                if bindings.sides[i] != self.bindings.sides[i] && self.pressed.contains(cid) {
                    self.suppressed.insert(*cid);
                }
            }
            if bindings.gesture != self.bindings.gesture {
                self.stroke = None;
                self.pending_gesture_actions.clear();
                if self.pressed.contains(&195) {
                    self.suppressed.insert(195);
                }
            }
            self.bindings = bindings;
            self.update_modifiers()
        })();
        self.finish(result)
    }
    fn cancel(&mut self) -> Result<(), String> {
        self.stroke = None;
        self.pending_gesture_actions.clear();
        self.suppressed = self.pressed.clone();
        self.synchronized = false;
        let result = self.update_modifiers();
        // Check even when zero state needs no output.
        let result = result.and_then(|()| {
            self.output
                .as_ref()
                .ok_or_else(|| "Action output is closed".into())
                .map(|_| ())
        });
        self.finish(result)
    }
    fn close(&mut self) -> Result<(), String> {
        if let Some(result) = &self.closed {
            return result.clone();
        }
        self.stroke = None;
        self.pending_gesture_actions.clear();
        self.held = HeldModifiers::None;
        let result = self
            .output
            .take()
            .map_or(Ok(()), |mut output| output.close());
        self.closed = Some(result.clone());
        result
    }
}
impl<O: Output> Drop for Runtime<O> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
impl<O: Output> EventSink for Runtime<O> {
    fn event(&mut self, event: &Event) -> Result<(), String> {
        let now = self.start.elapsed();
        match event {
            Event::Buttons { initial, held, .. } => {
                if *initial {
                    self.cancel()?;
                }
                self.buttons(held, now)
            }
            Event::RawXY {
                dx, dy, sources, ..
            } => {
                if sources.as_slice() != [195] {
                    // Raw XY has no CID on the wire. Ambiguity must cancel,
                    // otherwise release could invent a click or wrong direction.
                    self.stroke = None;
                    if self.pressed.contains(&195) {
                        self.suppressed.insert(195);
                    }
                    Ok(())
                } else if let Some(stroke) = self.stroke.as_mut().filter(|s| s.initial_raw) {
                    stroke.initial_raw = false;
                    Ok(())
                } else {
                    self.motion(*dx, *dy, now)
                }
            }
        }
    }
    fn before_query(&mut self) -> Result<(), String> {
        if self.output.is_none() {
            return Err("Action output is closed".into());
        }
        // Keep physical travel provisional while querying: unchanged periodic
        // reads must not cancel a stroke or swallow new presses. No gesture
        // output escapes before the entire snapshot validates the same scale.
        self.query_pending = true;
        if self.bindings.gesture.as_ref().is_some_and(|g| g.physical) {
            Ok(())
        } else {
            self.set_dpi(None)
        }
    }
    fn snapshot(&mut self, snapshot: &Value) -> Result<(), String> {
        let result = (|| {
            let dpi = self.bindings.validate_snapshot(snapshot)?;
            self.skip_initial_raw = initial_raw_quirk(snapshot);
            if self.bindings.gesture.is_some() && self.skip_initial_raw.is_none() {
                return Err("Cannot verify the 1B04 version for gesture motion".into());
            }
            if self.bindings.uses_desktop() {
                self.output
                    .as_mut()
                    .ok_or("Action output is closed")?
                    .require_desktop()?;
            }
            self.set_dpi(dpi)?;
            self.query_pending = false;
            // Consume before output: a failed/uncertain action is never retried,
            // and finish() closes output and discards any remaining actions.
            for action in std::mem::take(&mut self.pending_gesture_actions) {
                action.emit(self.output.as_mut().ok_or("Action output is closed")?)?;
            }
            Ok(())
        })();
        self.finish(result)
    }
    fn close(&mut self) -> Result<(), String> {
        self.close()
    }
}

/// Owns an already authorized keyboard. Feed only current, capability-validated
/// notifications from the single HID owner, never a recorded observation log.
/// The first button list synchronizes without output. This API does not enable
/// reporting; the guardian owns capture activation and recovery separately.
pub struct NativeActions {
    runtime: Runtime<NativeOutput<WaylandKeyboard, Hyprland>>,
}
impl NativeActions {
    pub fn new(
        mut keyboard: WaylandKeyboard,
        bindings: ActionBindings,
        dpi: Option<u16>,
    ) -> Result<Self, String> {
        if !keyboard.is_open() {
            return Err("Action output is closed".into());
        }
        // Ownership may come from an existing keyboard with held modifiers.
        // Confirm zero before the engine assumes its initial empty state.
        keyboard.set_held(HeldModifiers::None)?;
        Ok(Self {
            runtime: Runtime::new(
                NativeOutput {
                    keyboard,
                    desktop: Hyprland::environment(),
                },
                bindings,
                dpi,
            )?,
        })
    }
    pub fn buttons(&mut self, held: &[u16]) -> Result<(), String> {
        self.runtime.buttons(held, self.runtime.start.elapsed())
    }
    pub fn motion(&mut self, dx: i16, dy: i16) -> Result<(), String> {
        self.runtime.motion(dx, dy, self.runtime.start.elapsed())
    }
    pub fn set_dpi(&mut self, dpi: Option<u16>) -> Result<(), String> {
        self.runtime.set_dpi(dpi)
    }
    pub fn replace_bindings(&mut self, bindings: ActionBindings) -> Result<(), String> {
        self.runtime.replace_bindings(bindings)
    }
    /// Link loss/cancellation releases modifiers and requires fresh synchronization.
    pub fn cancel(&mut self) -> Result<(), String> {
        self.runtime.cancel()
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.runtime.close()
    }
    pub fn feedback(&self) -> Option<GestureFeedback> {
        self.runtime.feedback
    }
    pub fn is_open(&self) -> bool {
        self.runtime.output.is_some()
    }
    pub fn last_error(&self) -> Option<&str> {
        self.runtime.error.as_deref().or_else(|| {
            self.runtime
                .closed
                .as_ref()
                .and_then(|result| result.as_ref().err())
                .map(String::as_str)
        })
    }
}

impl EventSink for NativeActions {
    fn event(&mut self, event: &Event) -> Result<(), String> {
        self.runtime.event(event)
    }
    fn before_query(&mut self) -> Result<(), String> {
        self.runtime.before_query()
    }
    fn snapshot(&mut self, snapshot: &Value) -> Result<(), String> {
        self.runtime.snapshot(snapshot)
    }
    fn close(&mut self) -> Result<(), String> {
        self.close()
    }
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
