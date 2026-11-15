//! Pure key-remapping decision logic.
//!
//! The [`Engine`] receives physical key events (`code`, `value` with
//! 0 = release, 1 = press, 2 = autorepeat) and returns the events to write to
//! the virtual keyboard plus any side-effect actions (launch app, open URI,
//! run command). It has no I/O, so it can be tested exhaustively.
//!
//! Processing happens in two stages, like upstream Keyboard Manager:
//! 1. **Single-key rules** (`KeyTrigger::Key`) translate the physical key.
//!    Key→key remaps produce a *logical* key that continues to stage 2, so a
//!    remapped CapsLock→Ctrl takes part in Ctrl shortcuts.
//! 2. **Shortcut rules** (`KeyTrigger::Combo`) match the logical key state.
//!    The best match wins: app-specific rules first, then rules with more
//!    modifiers, then config order.
//!
//! Stuck-key prevention: the engine tracks the exact set of keys that are
//! down on the virtual device (`virt`). Presses of keys already down and
//! releases of keys that are not down are dropped, every press records what
//! it was translated to so its release is translated the same way (even if
//! the focused app or rules changed in between), and [`Engine::release_all`]
//! releases everything still down.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;

use evdev::Key;

use crate::keymap;
use crate::mapping::{KeyAction, KeyCombo, KeyMapping, KeyTrigger};

pub const RELEASE: i32 = 0;
pub const PRESS: i32 = 1;
pub const REPEAT: i32 = 2;

const CTRL: (u16, u16) = (Key::KEY_LEFTCTRL.code(), Key::KEY_RIGHTCTRL.code());
const SHIFT: (u16, u16) = (Key::KEY_LEFTSHIFT.code(), Key::KEY_RIGHTSHIFT.code());
const ALT: (u16, u16) = (Key::KEY_LEFTALT.code(), Key::KEY_RIGHTALT.code());
const META: (u16, u16) = (Key::KEY_LEFTMETA.code(), Key::KEY_RIGHTMETA.code());

/// Whether `code` is one of the eight modifier keys.
pub fn is_modifier(code: u16) -> bool {
    [CTRL, SHIFT, ALT, META]
        .iter()
        .any(|&(l, r)| code == l || code == r)
}

/// A modifier requirement in a shortcut trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModSpec {
    /// A specific key, e.g. `KEY_LEFTCTRL`.
    Exact(u16),
    /// Either side, e.g. `CTRL` matches left or right control.
    Either(u16, u16),
}

impl ModSpec {
    fn matches(self, code: u16) -> bool {
        match self {
            Self::Exact(c) => c == code,
            Self::Either(l, r) => code == l || code == r,
        }
    }

    /// Key to press when this spec is used on the output side.
    fn output_code(self) -> u16 {
        match self {
            Self::Exact(c) | Self::Either(c, _) => c,
        }
    }
}

/// Parse a key name: evdev names (`KEY_A`), names without prefix (`a`, `esc`)
/// and side-agnostic modifier aliases (`CTRL`, `SHIFT`, `ALT`, `META`/`SUPER`/`WIN`).
pub fn parse_key(name: &str) -> Option<ModSpec> {
    let upper = name.trim().to_ascii_uppercase();
    let bare = upper.strip_prefix("KEY_").unwrap_or(&upper);
    let either = match bare {
        "CTRL" | "CONTROL" => Some(CTRL),
        "SHIFT" => Some(SHIFT),
        "ALT" => Some(ALT),
        "META" | "SUPER" | "WIN" => Some(META),
        _ => None,
    };
    if let Some((l, r)) = either {
        return Some(ModSpec::Either(l, r));
    }
    Key::from_str(&upper)
        .or_else(|_| Key::from_str(&format!("KEY_{bare}")))
        .ok()
        .map(|k| ModSpec::Exact(k.code()))
}

fn parse_code(name: &str) -> Option<u16> {
    parse_key(name).map(ModSpec::output_code)
}

/// Output side of a remap: modifiers held, `taps` pressed and released in
/// order, then `key` held for as long as the trigger is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutCombo {
    pub mods: Vec<u16>,
    pub taps: Vec<u16>,
    pub key: u16,
}

impl OutCombo {
    fn is_plain_key(&self) -> bool {
        self.mods.is_empty() && self.taps.is_empty()
    }
}

/// Side-effect actions the worker performs outside the key stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Shell command line (the model's `RunCommand` is documented as a shell command).
    RunCommand(String),
    LaunchApp {
        path: String,
        args: Vec<String>,
        working_dir: Option<String>,
        elevated: bool,
    },
    OpenUri(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Key { code: u16, value: i32 },
    Action(Action),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Effect {
    Remap(OutCombo),
    Disable,
    Text(Vec<(u16, bool)>),
    Action(Action),
}

#[derive(Debug, Clone)]
struct KeyRule {
    from: u16,
    effect: Effect,
    app: Option<String>,
}

#[derive(Debug, Clone)]
struct ShortcutRule {
    mods: Vec<ModSpec>,
    key: u16,
    chord: Option<u16>,
    exact: bool,
    effect: Effect,
    app: Option<String>,
}

/// Compiled, validated mapping rules.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    keys: Vec<KeyRule>,
    shortcuts: Vec<ShortcutRule>,
}

impl Rules {
    /// Compile the config model. Invalid mappings are skipped and described
    /// in the returned warnings; disabled mappings are ignored.
    pub fn compile(mappings: &[KeyMapping]) -> (Self, Vec<String>) {
        let mut rules = Self::default();
        let mut warnings = Vec::new();
        for (i, m) in mappings.iter().enumerate() {
            if !m.enabled {
                continue;
            }
            if let Err(e) = rules.add(m, &mut warnings) {
                warnings.push(format!("mapping #{}: {e}; skipped", i + 1));
            }
        }
        (rules, warnings)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.shortcuts.is_empty()
    }

    /// Whether any rule is restricted to a specific application.
    pub fn has_app_filters(&self) -> bool {
        self.keys.iter().any(|r| r.app.is_some()) || self.shortcuts.iter().any(|r| r.app.is_some())
    }

    fn add(&mut self, m: &KeyMapping, warnings: &mut Vec<String>) -> Result<(), String> {
        let effect = compile_effect(&m.action, warnings)?;
        let app = m
            .app_filter
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase);
        let single = match &m.trigger {
            KeyTrigger::Key(k) => Some(k.as_str()),
            KeyTrigger::Combo(c) if c.is_single_key() => Some(c.key.as_str()),
            KeyTrigger::Combo(_) => None,
        };
        if let Some(name) = single {
            let from = parse_code(name).ok_or_else(|| format!("unknown key {name:?}"))?;
            self.keys.push(KeyRule { from, effect, app });
            return Ok(());
        }
        let KeyTrigger::Combo(c) = &m.trigger else {
            unreachable!()
        };
        let mods = c
            .modifiers
            .iter()
            .map(|n| parse_key(n).ok_or_else(|| format!("unknown key {n:?}")))
            .collect::<Result<Vec<_>, _>>()?;
        let key = parse_code(&c.key).ok_or_else(|| format!("unknown key {:?}", c.key))?;
        let chord = match &c.chord {
            Some(n) => Some(parse_code(n).ok_or_else(|| format!("unknown key {n:?}"))?),
            None => None,
        };
        self.shortcuts.push(ShortcutRule {
            mods,
            key,
            chord,
            exact: m.exact_match,
            effect,
            app,
        });
        Ok(())
    }
}

fn compile_out_combo(c: &KeyCombo) -> Result<OutCombo, String> {
    let mods = c
        .modifiers
        .iter()
        .map(|n| parse_code(n).ok_or_else(|| format!("unknown target key {n:?}")))
        .collect::<Result<Vec<_>, _>>()?;
    let first = parse_code(&c.key).ok_or_else(|| format!("unknown target key {:?}", c.key))?;
    Ok(match &c.chord {
        Some(n) => OutCombo {
            mods,
            taps: vec![first],
            key: parse_code(n).ok_or_else(|| format!("unknown target key {n:?}"))?,
        },
        None => OutCombo {
            mods,
            taps: Vec::new(),
            key: first,
        },
    })
}

fn compile_effect(a: &KeyAction, warnings: &mut Vec<String>) -> Result<Effect, String> {
    Ok(match a {
        KeyAction::RemapKey { to } => Effect::Remap(compile_out_combo(to)?),
        KeyAction::Disable => Effect::Disable,
        KeyAction::SendText { text } => {
            let (keys, skipped) = keymap::text_to_keys(text);
            if !skipped.is_empty() {
                warnings.push(format!(
                    "send_text {text:?}: characters {skipped:?} cannot be typed with the US keymap and are skipped"
                ));
            }
            Effect::Text(keys)
        }
        KeyAction::RunCommand { command } => {
            if command.trim().is_empty() {
                return Err("empty command".into());
            }
            Effect::Action(Action::RunCommand(command.clone()))
        }
        KeyAction::LaunchApp {
            path,
            args,
            working_dir,
            elevated,
        } => {
            if path.trim().is_empty() {
                return Err("empty launch_app path".into());
            }
            Effect::Action(Action::LaunchApp {
                path: path.clone(),
                args: args.clone(),
                working_dir: working_dir.clone(),
                elevated: *elevated,
            })
        }
        KeyAction::OpenUri { uri } => {
            if uri.trim().is_empty() {
                return Err("empty uri".into());
            }
            Effect::Action(Action::OpenUri(uri.clone()))
        }
    })
}

/// What a physical key press was translated to (stage 1).
#[derive(Debug, Clone)]
enum Held {
    Logical(u16),
    Combo { key: u16, pressed_mods: Vec<u16> },
    Swallow,
}

#[derive(Debug, Clone)]
enum ActiveKind {
    Remap { key: u16, pressed_mods: Vec<u16> },
    Swallow { restore: bool },
}

/// A fired shortcut whose action key is still held.
#[derive(Debug, Clone)]
struct Active {
    end_key: u16,
    trigger_mods: Vec<u16>,
    kind: ActiveKind,
}

pub struct Engine {
    rules: Rules,
    /// Lower-cased identifiers of the focused app (WM_CLASS, process name).
    /// Empty = unknown, so app-specific rules never apply.
    app: Vec<String>,
    /// Logical keys down (after stage 1), with a count for keys held via
    /// several physical keys.
    logical: BTreeMap<u16, u32>,
    /// Keys currently down on the virtual device.
    virt: BTreeSet<u16>,
    held: HashMap<u16, Held>,
    active: Option<Active>,
    /// Logical keys whose repeat/release must be swallowed.
    suppressed: BTreeSet<u16>,
    /// Shortcut rule index whose first chord key was pressed.
    chord: Option<usize>,
    out: Vec<Output>,
}

impl Engine {
    pub fn new(rules: Rules) -> Self {
        Self {
            rules,
            app: Vec::new(),
            logical: BTreeMap::new(),
            virt: BTreeSet::new(),
            held: HashMap::new(),
            active: None,
            suppressed: BTreeSet::new(),
            chord: None,
            out: Vec::new(),
        }
    }

    /// Set identifiers of the focused application (e.g. WM_CLASS instance and
    /// class, process name). Matching is case-insensitive.
    pub fn set_focused_app(&mut self, ids: &[String]) {
        self.app = ids.iter().map(|s| s.to_lowercase()).collect();
    }

    /// Keys currently down on the virtual device.
    pub fn virtual_down(&self) -> impl Iterator<Item = u16> + '_ {
        self.virt.iter().copied()
    }

    /// Process one physical key event; returns what to emit.
    pub fn process(&mut self, code: u16, value: i32) -> Vec<Output> {
        match value {
            PRESS => self.phys_press(code),
            RELEASE => self.phys_release(code),
            REPEAT => self.phys_repeat(code),
            _ => {}
        }
        std::mem::take(&mut self.out)
    }

    /// Release every key still down on the virtual device and reset state.
    pub fn release_all(&mut self) -> Vec<Output> {
        let down: Vec<u16> = self.virt.iter().copied().collect();
        for c in down {
            self.emit(c, RELEASE);
        }
        self.logical.clear();
        self.held.clear();
        self.active = None;
        self.suppressed.clear();
        self.chord = None;
        std::mem::take(&mut self.out)
    }

    // -- helpers -------------------------------------------------------------

    fn emit(&mut self, code: u16, value: i32) {
        let ok = match value {
            PRESS => self.virt.insert(code),
            RELEASE => self.virt.remove(&code),
            _ => self.virt.contains(&code),
        };
        if ok {
            self.out.push(Output::Key { code, value });
        }
    }

    fn app_ok(&self, app: &Option<String>) -> bool {
        match app {
            None => true,
            Some(a) => self.app.iter().any(|id| id == a),
        }
    }

    fn is_logical(&self, code: u16) -> bool {
        self.logical.contains_key(&code)
    }

    /// Press logical modifiers that are not down on the virtual device.
    fn restore_modifiers(&mut self) {
        let mods: Vec<u16> = self
            .logical
            .keys()
            .copied()
            .filter(|&c| is_modifier(c) && !self.virt.contains(&c) && !self.suppressed.contains(&c))
            .collect();
        for c in mods {
            self.emit(c, PRESS);
        }
    }

    fn release_virtual_modifiers(&mut self) {
        let mods: Vec<u16> = self
            .virt
            .iter()
            .copied()
            .filter(|&c| is_modifier(c))
            .collect();
        for c in mods {
            self.emit(c, RELEASE);
        }
    }

    fn type_text(&mut self, keys: &[(u16, bool)]) {
        self.release_virtual_modifiers();
        let shift = SHIFT.0;
        for &(code, needs_shift) in keys {
            if self.virt.contains(&code) {
                self.emit(code, RELEASE);
            }
            if needs_shift {
                self.emit(shift, PRESS);
            }
            self.emit(code, PRESS);
            self.emit(code, RELEASE);
            if needs_shift {
                self.emit(shift, RELEASE);
            }
        }
    }

    /// Press an output combo; returns the modifiers this call pressed.
    fn press_combo(&mut self, combo: &OutCombo) -> Vec<u16> {
        let mut pressed = Vec::new();
        for &m in &combo.mods {
            if !self.virt.contains(&m) {
                self.emit(m, PRESS);
                pressed.push(m);
            }
        }
        for &t in &combo.taps {
            self.emit(t, PRESS);
            self.emit(t, RELEASE);
        }
        if self.virt.contains(&combo.key) {
            self.emit(combo.key, RELEASE);
        }
        self.emit(combo.key, PRESS);
        pressed
    }

    fn release_combo(&mut self, key: u16, pressed_mods: &[u16]) {
        self.emit(key, RELEASE);
        for &m in pressed_mods.iter().rev() {
            if !self.is_logical(m) {
                self.emit(m, RELEASE);
            }
        }
    }

    // -- stage 1: physical keys ---------------------------------------------

    fn find_key_rule(&self, code: u16) -> Option<Effect> {
        let mut best: Option<&KeyRule> = None;
        for r in self.rules.keys.iter().filter(|r| r.from == code) {
            if !self.app_ok(&r.app) {
                continue;
            }
            match best {
                Some(b) if b.app.is_some() || r.app.is_none() => {}
                _ => best = Some(r),
            }
        }
        best.map(|r| r.effect.clone())
    }

    fn phys_press(&mut self, code: u16) {
        if self.held.contains_key(&code) {
            // Same key pressed again without release (e.g. two keyboards).
            self.phys_repeat(code);
            return;
        }
        let held = match self.find_key_rule(code) {
            None => {
                self.logical_press(code);
                Held::Logical(code)
            }
            Some(Effect::Remap(combo)) if combo.is_plain_key() => {
                self.logical_press(combo.key);
                Held::Logical(combo.key)
            }
            Some(Effect::Remap(combo)) => {
                let pressed_mods = self.press_combo(&combo);
                Held::Combo {
                    key: combo.key,
                    pressed_mods,
                }
            }
            Some(Effect::Disable) => Held::Swallow,
            Some(Effect::Text(keys)) => {
                self.type_text(&keys);
                self.restore_modifiers();
                Held::Swallow
            }
            Some(Effect::Action(a)) => {
                self.out.push(Output::Action(a));
                Held::Swallow
            }
        };
        self.held.insert(code, held);
    }

    fn phys_release(&mut self, code: u16) {
        match self.held.remove(&code) {
            Some(Held::Logical(c)) => self.logical_release(c),
            Some(Held::Combo { key, pressed_mods }) => self.release_combo(key, &pressed_mods),
            Some(Held::Swallow) => {}
            // Pressed before the engine started: pass the release through
            // (dropped by `emit` if the virtual device never saw the press).
            None => self.emit(code, RELEASE),
        }
    }

    fn phys_repeat(&mut self, code: u16) {
        match self.held.get(&code) {
            Some(Held::Logical(c)) => {
                let c = *c;
                self.logical_repeat(c);
            }
            Some(Held::Combo { key, .. }) => {
                let k = *key;
                self.emit(k, REPEAT);
            }
            Some(Held::Swallow) | None => {}
        }
    }

    // -- stage 2: shortcuts on logical keys ---------------------------------

    /// Modifier codes held for `rule`, or `None` if the rule is not satisfied.
    /// `code` is the key being pressed now (excluded from the checks).
    fn match_mods(&self, rule: &ShortcutRule, code: u16) -> Option<Vec<u16>> {
        let held: Vec<u16> = self
            .logical
            .keys()
            .copied()
            .filter(|&c| c != code)
            .collect();
        let mut matched = Vec::new();
        for spec in &rule.mods {
            let hits: Vec<u16> = held.iter().copied().filter(|&c| spec.matches(c)).collect();
            if hits.is_empty() {
                return None;
            }
            matched.extend(hits);
        }
        if rule.exact {
            let extra = held
                .iter()
                .any(|&c| c != rule.key && !rule.mods.iter().any(|s| s.matches(c)));
            if extra {
                return None;
            }
        }
        matched.sort_unstable();
        matched.dedup();
        Some(matched)
    }

    fn find_shortcut(&self, code: u16) -> Option<(usize, Vec<u16>)> {
        let mut best: Option<(usize, Vec<u16>)> = None;
        for (i, r) in self.rules.shortcuts.iter().enumerate() {
            if r.key != code || !self.app_ok(&r.app) {
                continue;
            }
            let Some(m) = self.match_mods(r, code) else {
                continue;
            };
            let better = match &best {
                None => true,
                Some((b, _)) => {
                    let b = &self.rules.shortcuts[*b];
                    (r.app.is_some(), r.mods.len()) > (b.app.is_some(), b.mods.len())
                }
            };
            if better {
                best = Some((i, m));
            }
        }
        best
    }

    fn logical_press(&mut self, code: u16) {
        if let Some(n) = self.logical.get_mut(&code) {
            // Already held through another physical key: just count it.
            *n += 1;
            return;
        }

        if let Some(idx) = self.chord.take()
            && !is_modifier(code)
        {
            let rule = self.rules.shortcuts[idx].clone();
            if rule.chord == Some(code)
                && let Some(mods) = self.match_mods(&rule, code)
            {
                self.logical.insert(code, 1);
                self.fire(&rule.effect, code, mods);
                return;
            }
            // Wrong second key: chord cancelled, key handled normally.
        }

        let found = self.find_shortcut(code);
        self.logical.insert(code, 1);
        match found {
            Some((idx, _)) if self.rules.shortcuts[idx].chord.is_some() => {
                // First half of a chord: swallow and wait for the second key.
                self.chord = Some(idx);
                self.suppressed.insert(code);
            }
            Some((idx, mods)) => {
                let effect = self.rules.shortcuts[idx].effect.clone();
                self.fire(&effect, code, mods);
            }
            None => self.emit(code, PRESS),
        }
    }

    fn fire(&mut self, effect: &Effect, code: u16, trigger_mods: Vec<u16>) {
        if let Some(prev) = self.active.take() {
            // Another shortcut was still held; finish it first.
            self.suppressed.insert(prev.end_key);
            self.end_active(prev);
        }
        let kind = match effect {
            Effect::Remap(combo) => {
                for &m in &trigger_mods {
                    if !combo.mods.contains(&m) {
                        self.emit(m, RELEASE);
                    }
                }
                let pressed_mods = self.press_combo(combo);
                ActiveKind::Remap {
                    key: combo.key,
                    pressed_mods,
                }
            }
            Effect::Disable => ActiveKind::Swallow { restore: false },
            Effect::Text(keys) => {
                self.type_text(keys);
                ActiveKind::Swallow { restore: true }
            }
            Effect::Action(a) => {
                self.release_virtual_modifiers();
                self.out.push(Output::Action(a.clone()));
                ActiveKind::Swallow { restore: true }
            }
        };
        self.active = Some(Active {
            end_key: code,
            trigger_mods,
            kind,
        });
    }

    fn end_active(&mut self, active: Active) {
        match active.kind {
            ActiveKind::Remap { key, pressed_mods } => {
                self.release_combo(key, &pressed_mods);
                self.restore_modifiers();
            }
            ActiveKind::Swallow { restore: true } => self.restore_modifiers(),
            ActiveKind::Swallow { restore: false } => {}
        }
    }

    fn logical_release(&mut self, code: u16) {
        match self.logical.get_mut(&code) {
            Some(n) if *n > 1 => {
                *n -= 1;
                return;
            }
            Some(_) => {
                self.logical.remove(&code);
            }
            None => {}
        }

        if let Some(active) = self.active.take() {
            if active.end_key == code {
                self.suppressed.remove(&code);
                self.end_active(active);
                return;
            }
            if active.trigger_mods.contains(&code) {
                // A trigger modifier was released before the action key: end
                // the shortcut now and swallow the action key's release.
                self.suppressed.insert(active.end_key);
                self.end_active(active);
                self.emit(code, RELEASE);
                return;
            }
            self.active = Some(active);
        }

        if self.suppressed.remove(&code) {
            return;
        }
        if is_modifier(code) {
            self.chord = None;
        }
        self.emit(code, RELEASE);
    }

    fn logical_repeat(&mut self, code: u16) {
        if let Some(active) = &self.active
            && active.end_key == code
        {
            if let ActiveKind::Remap { key, .. } = active.kind {
                self.emit(key, REPEAT);
            }
            return;
        }
        if self.suppressed.contains(&code) {
            return;
        }
        self.emit(code, REPEAT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u16 = Key::KEY_A.code();
    const B: u16 = Key::KEY_B.code();
    const C: u16 = Key::KEY_C.code();
    const D: u16 = Key::KEY_D.code();
    const K: u16 = Key::KEY_K.code();
    const V: u16 = Key::KEY_V.code();
    const T: u16 = Key::KEY_T.code();
    const H: u16 = Key::KEY_H.code();
    const I: u16 = Key::KEY_I.code();
    const ESC: u16 = Key::KEY_ESC.code();
    const CAPS: u16 = Key::KEY_CAPSLOCK.code();
    const INS: u16 = Key::KEY_INSERT.code();
    const HOME: u16 = Key::KEY_HOME.code();
    const LCTRL: u16 = Key::KEY_LEFTCTRL.code();
    const RCTRL: u16 = Key::KEY_RIGHTCTRL.code();
    const LSHIFT: u16 = Key::KEY_LEFTSHIFT.code();
    const LALT: u16 = Key::KEY_LEFTALT.code();
    const LMETA: u16 = Key::KEY_LEFTMETA.code();

    fn engine(maps: Vec<KeyMapping>) -> Engine {
        let (rules, warnings) = Rules::compile(&maps);
        assert!(warnings.is_empty(), "{warnings:?}");
        Engine::new(rules)
    }

    fn k(code: u16, value: i32) -> Output {
        Output::Key { code, value }
    }

    fn press(e: &mut Engine, code: u16) -> Vec<Output> {
        e.process(code, PRESS)
    }

    fn release(e: &mut Engine, code: u16) -> Vec<Output> {
        e.process(code, RELEASE)
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn assert_clean(e: &Engine) {
        assert_eq!(e.virtual_down().count(), 0, "keys still down: {:?}", e.virt);
    }

    #[test]
    fn passthrough_without_rules() {
        let mut e = engine(vec![]);
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
        assert_eq!(e.process(A, REPEAT), vec![k(A, REPEAT)]);
        assert_eq!(release(&mut e, A), vec![k(A, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn release_of_key_pressed_before_start_is_dropped() {
        let mut e = engine(vec![]);
        assert!(release(&mut e, A).is_empty());
        assert!(e.process(A, REPEAT).is_empty());
    }

    #[test]
    fn key_to_key_remap() {
        let mut e = engine(vec![KeyMapping::remap("KEY_CAPSLOCK", "KEY_ESC")]);
        assert_eq!(press(&mut e, CAPS), vec![k(ESC, PRESS)]);
        assert_eq!(e.process(CAPS, REPEAT), vec![k(ESC, REPEAT)]);
        assert_eq!(release(&mut e, CAPS), vec![k(ESC, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn swap_keys() {
        let mut e = engine(vec![
            KeyMapping::remap("KEY_A", "KEY_B"),
            KeyMapping::remap("KEY_B", "KEY_A"),
        ]);
        assert_eq!(press(&mut e, A), vec![k(B, PRESS)]);
        assert_eq!(press(&mut e, B), vec![k(A, PRESS)]);
        assert_eq!(release(&mut e, A), vec![k(B, RELEASE)]);
        assert_eq!(release(&mut e, B), vec![k(A, RELEASE)]);
    }

    #[test]
    fn remapped_key_takes_part_in_shortcuts() {
        // CapsLock -> Ctrl, and Ctrl+H -> Backspace.
        let mut e = engine(vec![
            KeyMapping::remap("KEY_CAPSLOCK", "KEY_LEFTCTRL"),
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_H"),
                KeyCombo::key("KEY_BACKSPACE"),
            ),
        ]);
        let bs = Key::KEY_BACKSPACE.code();
        assert_eq!(press(&mut e, CAPS), vec![k(LCTRL, PRESS)]);
        assert_eq!(press(&mut e, H), vec![k(LCTRL, RELEASE), k(bs, PRESS)]);
        assert_eq!(release(&mut e, H), vec![k(bs, RELEASE), k(LCTRL, PRESS)]);
        assert_eq!(release(&mut e, CAPS), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn key_to_combo() {
        let mut e = engine(vec![KeyMapping::remap_to_combo(
            "KEY_INSERT",
            s(&["KEY_LEFTCTRL", "KEY_LEFTSHIFT"]),
            "KEY_V",
        )]);
        assert_eq!(
            press(&mut e, INS),
            vec![k(LCTRL, PRESS), k(LSHIFT, PRESS), k(V, PRESS)]
        );
        assert_eq!(e.process(INS, REPEAT), vec![k(V, REPEAT)]);
        assert_eq!(
            release(&mut e, INS),
            vec![k(V, RELEASE), k(LSHIFT, RELEASE), k(LCTRL, RELEASE)]
        );
        assert_clean(&e);
    }

    #[test]
    fn key_to_combo_keeps_physically_held_modifier() {
        let mut e = engine(vec![KeyMapping::remap_to_combo(
            "KEY_INSERT",
            s(&["KEY_LEFTCTRL"]),
            "KEY_V",
        )]);
        assert_eq!(press(&mut e, LCTRL), vec![k(LCTRL, PRESS)]);
        assert_eq!(press(&mut e, INS), vec![k(V, PRESS)]);
        assert_eq!(release(&mut e, INS), vec![k(V, RELEASE)]);
        assert_eq!(release(&mut e, LCTRL), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn disable_key() {
        let mut e = engine(vec![KeyMapping::disable("KEY_INSERT")]);
        assert!(press(&mut e, INS).is_empty());
        assert!(e.process(INS, REPEAT).is_empty());
        assert!(release(&mut e, INS).is_empty());
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
    }

    #[test]
    fn shortcut_remap_alt_c_to_ctrl_c() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_C"),
            KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_C"),
        )]);
        assert_eq!(press(&mut e, LALT), vec![k(LALT, PRESS)]);
        assert_eq!(
            press(&mut e, C),
            vec![k(LALT, RELEASE), k(LCTRL, PRESS), k(C, PRESS)]
        );
        assert_eq!(e.process(C, REPEAT), vec![k(C, REPEAT)]);
        // Releasing the action key restores the original modifier.
        assert_eq!(
            release(&mut e, C),
            vec![k(C, RELEASE), k(LCTRL, RELEASE), k(LALT, PRESS)]
        );
        // Shortcut can be repeated while Alt is still held.
        assert_eq!(
            press(&mut e, C),
            vec![k(LALT, RELEASE), k(LCTRL, PRESS), k(C, PRESS)]
        );
        assert_eq!(
            release(&mut e, C),
            vec![k(C, RELEASE), k(LCTRL, RELEASE), k(LALT, PRESS)]
        );
        assert_eq!(release(&mut e, LALT), vec![k(LALT, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn shortcut_modifier_released_first_no_stuck_keys() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_C"),
            KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_C"),
        )]);
        press(&mut e, LALT);
        press(&mut e, C);
        // Alt released while C still held: target combo released completely.
        assert_eq!(
            release(&mut e, LALT),
            vec![k(C, RELEASE), k(LCTRL, RELEASE)]
        );
        // C's repeat and release are swallowed (its press was never sent as C).
        assert!(e.process(C, REPEAT).is_empty());
        assert!(release(&mut e, C).is_empty());
        assert_clean(&e);
        // Normal typing continues.
        assert_eq!(press(&mut e, C), vec![k(C, PRESS)]);
        assert_eq!(release(&mut e, C), vec![k(C, RELEASE)]);
    }

    #[test]
    fn shortcut_same_modifier_in_target() {
        // Ctrl+A -> Ctrl+Home: Ctrl stays down, nothing toggles.
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_A"),
            KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_HOME"),
        )]);
        press(&mut e, LCTRL);
        assert_eq!(press(&mut e, A), vec![k(HOME, PRESS)]);
        assert_eq!(release(&mut e, A), vec![k(HOME, RELEASE)]);
        assert_eq!(release(&mut e, LCTRL), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn shortcut_requires_modifiers() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_C"),
            KeyCombo::key("KEY_D"),
        )]);
        assert_eq!(press(&mut e, C), vec![k(C, PRESS)]);
        assert_eq!(release(&mut e, C), vec![k(C, RELEASE)]);
        // Wrong side does not match an exact-side modifier.
        press(&mut e, Key::KEY_RIGHTALT.code());
        assert_eq!(press(&mut e, C), vec![k(C, PRESS)]);
    }

    #[test]
    fn either_side_modifier_alias() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["CTRL"]), "KEY_A"),
            KeyCombo::key("KEY_B"),
        )]);
        press(&mut e, RCTRL);
        assert_eq!(press(&mut e, A), vec![k(RCTRL, RELEASE), k(B, PRESS)]);
        assert_eq!(release(&mut e, A), vec![k(B, RELEASE), k(RCTRL, PRESS)]);
    }

    #[test]
    fn non_exact_allows_extra_modifiers() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_A"),
            KeyCombo::key("KEY_B"),
        )]);
        press(&mut e, LCTRL);
        press(&mut e, LSHIFT);
        // Shift is not part of the trigger and stays down: Shift+B.
        assert_eq!(press(&mut e, A), vec![k(LCTRL, RELEASE), k(B, PRESS)]);
    }

    #[test]
    fn exact_match_rejects_extra_keys() {
        let mut e = engine(vec![
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_A"),
                KeyCombo::key("KEY_B"),
            )
            .exact(),
        ]);
        press(&mut e, LCTRL);
        press(&mut e, LSHIFT);
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
        release(&mut e, A);
        release(&mut e, LSHIFT);
        // Without Shift it matches.
        assert_eq!(press(&mut e, A), vec![k(LCTRL, RELEASE), k(B, PRESS)]);
    }

    #[test]
    fn exact_match_rejects_extra_non_modifier() {
        let mut e = engine(vec![
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_A"),
                KeyCombo::key("KEY_B"),
            )
            .exact(),
        ]);
        press(&mut e, LCTRL);
        press(&mut e, D);
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
    }

    #[test]
    fn more_specific_shortcut_wins() {
        let mut e = engine(vec![
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_A"),
                KeyCombo::key("KEY_B"),
            ),
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTCTRL", "KEY_LEFTSHIFT"]), "KEY_A"),
                KeyCombo::key("KEY_D"),
            ),
        ]);
        press(&mut e, LCTRL);
        press(&mut e, LSHIFT);
        assert_eq!(
            press(&mut e, A),
            vec![k(LCTRL, RELEASE), k(LSHIFT, RELEASE), k(D, PRESS)]
        );
        assert_eq!(
            release(&mut e, A),
            vec![k(D, RELEASE), k(LCTRL, PRESS), k(LSHIFT, PRESS)]
        );
    }

    #[test]
    fn disabled_shortcut() {
        let mut m = KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_Q"),
            KeyCombo::key("KEY_A"),
        );
        m.action = KeyAction::Disable;
        let q = Key::KEY_Q.code();
        let mut e = engine(vec![m]);
        assert_eq!(press(&mut e, LCTRL), vec![k(LCTRL, PRESS)]);
        assert!(press(&mut e, q).is_empty());
        assert!(e.process(q, REPEAT).is_empty());
        assert!(release(&mut e, q).is_empty());
        assert_eq!(release(&mut e, LCTRL), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn disabled_mapping_is_ignored() {
        let mut m = KeyMapping::remap("KEY_A", "KEY_B");
        m.enabled = false;
        let mut e = engine(vec![m]);
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
    }

    #[test]
    fn chord_fires_on_second_key() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::chord(s(&["KEY_LEFTCTRL"]), "KEY_K", "KEY_D"),
            KeyCombo::key("KEY_HOME"),
        )]);
        press(&mut e, LCTRL);
        assert!(press(&mut e, K).is_empty());
        assert!(release(&mut e, K).is_empty());
        assert_eq!(press(&mut e, D), vec![k(LCTRL, RELEASE), k(HOME, PRESS)]);
        assert_eq!(release(&mut e, D), vec![k(HOME, RELEASE), k(LCTRL, PRESS)]);
        assert_eq!(release(&mut e, LCTRL), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn chord_with_first_key_still_held() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::chord(s(&["KEY_LEFTCTRL"]), "KEY_K", "KEY_D"),
            KeyCombo::key("KEY_HOME"),
        )]);
        press(&mut e, LCTRL);
        press(&mut e, K);
        assert_eq!(press(&mut e, D), vec![k(LCTRL, RELEASE), k(HOME, PRESS)]);
        assert!(release(&mut e, K).is_empty());
        assert_eq!(release(&mut e, D), vec![k(HOME, RELEASE), k(LCTRL, PRESS)]);
    }

    #[test]
    fn chord_wrong_second_key_cancels() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::chord(s(&["KEY_LEFTCTRL"]), "KEY_K", "KEY_D"),
            KeyCombo::key("KEY_HOME"),
        )]);
        press(&mut e, LCTRL);
        press(&mut e, K);
        release(&mut e, K);
        // Wrong key passes through as Ctrl+A.
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
        release(&mut e, A);
        // Chord was reset: D alone is plain Ctrl+D now.
        assert_eq!(press(&mut e, D), vec![k(D, PRESS)]);
    }

    #[test]
    fn chord_reset_by_modifier_release() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::chord(s(&["KEY_LEFTCTRL"]), "KEY_K", "KEY_D"),
            KeyCombo::key("KEY_HOME"),
        )]);
        press(&mut e, LCTRL);
        press(&mut e, K);
        release(&mut e, K);
        release(&mut e, LCTRL);
        press(&mut e, LCTRL);
        assert_eq!(press(&mut e, D), vec![k(D, PRESS)]);
    }

    #[test]
    fn send_text_from_shortcut_releases_modifiers() {
        let maps = vec![KeyMapping {
            trigger: KeyTrigger::combo(s(&["KEY_LEFTCTRL"]), "KEY_T"),
            action: KeyAction::SendText { text: "Hi".into() },
            app_filter: None,
            enabled: true,
            exact_match: false,
        }];
        let mut e = engine(maps);
        press(&mut e, LCTRL);
        assert_eq!(
            press(&mut e, T),
            vec![
                k(LCTRL, RELEASE),
                k(LSHIFT, PRESS),
                k(H, PRESS),
                k(H, RELEASE),
                k(LSHIFT, RELEASE),
                k(I, PRESS),
                k(I, RELEASE),
            ]
        );
        assert!(e.process(T, REPEAT).is_empty());
        assert_eq!(release(&mut e, T), vec![k(LCTRL, PRESS)]);
        assert_eq!(release(&mut e, LCTRL), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn send_text_from_single_key() {
        let mut e = engine(vec![KeyMapping::send_text("KEY_F1", "a")]);
        let f1 = Key::KEY_F1.code();
        assert_eq!(press(&mut e, f1), vec![k(A, PRESS), k(A, RELEASE)]);
        assert!(release(&mut e, f1).is_empty());
    }

    #[test]
    fn send_text_non_ascii_warns() {
        let (_, warnings) = Rules::compile(&[KeyMapping::send_text("KEY_F1", "日本")]);
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn launch_actions() {
        let mut e = engine(vec![
            KeyMapping::launch_app(
                KeyTrigger::combo(s(&["KEY_LEFTMETA"]), "KEY_T"),
                "alacritty",
            ),
            KeyMapping::open_uri(
                KeyTrigger::combo(s(&["KEY_LEFTMETA"]), "KEY_G"),
                "https://github.com",
            ),
            KeyMapping::command("KEY_F12", "notify-send hi"),
        ]);
        press(&mut e, LMETA);
        assert_eq!(
            press(&mut e, T),
            vec![
                k(LMETA, RELEASE),
                Output::Action(Action::LaunchApp {
                    path: "alacritty".into(),
                    args: vec![],
                    working_dir: None,
                    elevated: false
                })
            ]
        );
        assert_eq!(release(&mut e, T), vec![k(LMETA, PRESS)]);
        let g = Key::KEY_G.code();
        assert_eq!(
            press(&mut e, g),
            vec![
                k(LMETA, RELEASE),
                Output::Action(Action::OpenUri("https://github.com".into()))
            ]
        );
        release(&mut e, g);
        release(&mut e, LMETA);
        let f12 = Key::KEY_F12.code();
        assert_eq!(
            press(&mut e, f12),
            vec![Output::Action(Action::RunCommand("notify-send hi".into()))]
        );
        assert!(release(&mut e, f12).is_empty());
        assert_clean(&e);
    }

    #[test]
    fn app_filter() {
        let mut e = engine(vec![KeyMapping::remap("KEY_A", "KEY_B").for_app("Firefox")]);
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
        release(&mut e, A);
        e.set_focused_app(&s(&["Navigator", "firefox"]));
        assert_eq!(press(&mut e, A), vec![k(B, PRESS)]);
        // Focus changes while held: release still matches the press.
        e.set_focused_app(&s(&["code"]));
        assert_eq!(release(&mut e, A), vec![k(B, RELEASE)]);
        assert_eq!(press(&mut e, A), vec![k(A, PRESS)]);
    }

    #[test]
    fn app_specific_rule_beats_global() {
        let mut e = engine(vec![
            KeyMapping::remap("KEY_A", "KEY_B"),
            KeyMapping::remap("KEY_A", "KEY_D").for_app("code"),
        ]);
        assert_eq!(press(&mut e, A), vec![k(B, PRESS)]);
        release(&mut e, A);
        e.set_focused_app(&s(&["code"]));
        assert_eq!(press(&mut e, A), vec![k(D, PRESS)]);
    }

    #[test]
    fn duplicate_logical_key_from_two_sources() {
        // CapsLock -> Ctrl while physical Ctrl is also held.
        let mut e = engine(vec![KeyMapping::remap("KEY_CAPSLOCK", "KEY_LEFTCTRL")]);
        assert_eq!(press(&mut e, LCTRL), vec![k(LCTRL, PRESS)]);
        assert!(press(&mut e, CAPS).is_empty());
        // Ctrl stays down until both are released.
        assert!(release(&mut e, LCTRL).is_empty());
        assert_eq!(release(&mut e, CAPS), vec![k(LCTRL, RELEASE)]);
        assert_clean(&e);
    }

    #[test]
    fn release_all_releases_everything() {
        let mut e = engine(vec![KeyMapping::remap_to_combo(
            "KEY_INSERT",
            s(&["KEY_LEFTCTRL"]),
            "KEY_V",
        )]);
        press(&mut e, A);
        press(&mut e, INS);
        let out = e.release_all();
        assert_eq!(out.len(), 3);
        assert!(
            out.iter()
                .all(|o| matches!(o, Output::Key { value: RELEASE, .. }))
        );
        assert_clean(&e);
        // Late releases after reset produce nothing.
        assert!(release(&mut e, INS).is_empty());
        assert!(release(&mut e, A).is_empty());
    }

    #[test]
    fn second_shortcut_while_first_held() {
        let mut e = engine(vec![
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_A"),
                KeyCombo::key("KEY_B"),
            ),
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_C"),
                KeyCombo::key("KEY_D"),
            ),
        ]);
        press(&mut e, LALT);
        press(&mut e, A);
        let out = press(&mut e, C);
        assert_eq!(out.last(), Some(&k(D, PRESS)));
        assert!(out.contains(&k(B, RELEASE)));
        assert!(release(&mut e, A).is_empty());
        release(&mut e, C);
        release(&mut e, LALT);
        assert_clean(&e);
    }

    #[test]
    fn output_chord() {
        let mut e = engine(vec![KeyMapping::shortcut_remap(
            KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_A"),
            KeyCombo::chord(s(&["KEY_LEFTCTRL"]), "KEY_K", "KEY_D"),
        )]);
        press(&mut e, LALT);
        assert_eq!(
            press(&mut e, A),
            vec![
                k(LALT, RELEASE),
                k(LCTRL, PRESS),
                k(K, PRESS),
                k(K, RELEASE),
                k(D, PRESS)
            ]
        );
    }

    #[test]
    fn parse_key_names() {
        assert_eq!(parse_key("KEY_A"), Some(ModSpec::Exact(A)));
        assert_eq!(parse_key("esc"), Some(ModSpec::Exact(ESC)));
        assert_eq!(parse_key("Ctrl"), Some(ModSpec::Either(LCTRL, RCTRL)));
        assert_eq!(parse_key("KEY_NOPE"), None);
        let (_, w) = Rules::compile(&[KeyMapping::remap("KEY_NOPE", "KEY_A")]);
        assert_eq!(w.len(), 1);
    }

    #[test]
    fn random_sequences_never_leave_keys_stuck() {
        // Deterministic pseudo-random press/release sequences across all rule
        // kinds; after releasing every physical key nothing may stay down.
        let maps = vec![
            KeyMapping::remap("KEY_CAPSLOCK", "KEY_LEFTCTRL"),
            KeyMapping::remap_to_combo("KEY_INSERT", s(&["KEY_LEFTCTRL"]), "KEY_V"),
            KeyMapping::disable("KEY_D"),
            KeyMapping::shortcut_remap(
                KeyCombo::shortcut(s(&["KEY_LEFTALT"]), "KEY_C"),
                KeyCombo::shortcut(s(&["KEY_LEFTCTRL"]), "KEY_C"),
            ),
            KeyMapping::shortcut_remap(
                KeyCombo::chord(s(&["KEY_LEFTCTRL"]), "KEY_K", "KEY_A"),
                KeyCombo::key("KEY_HOME"),
            ),
            KeyMapping {
                trigger: KeyTrigger::combo(s(&["KEY_LEFTSHIFT"]), "KEY_T"),
                action: KeyAction::SendText { text: "Ab".into() },
                app_filter: None,
                enabled: true,
                exact_match: false,
            },
        ];
        let keys = [CAPS, INS, D, C, K, A, T, LALT, LCTRL, LSHIFT];
        let mut seed: u32 = 12345;
        for _ in 0..300 {
            let mut e = engine(maps.clone());
            let mut down: Vec<u16> = Vec::new();
            for _ in 0..30 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let key = keys[(seed >> 16) as usize % keys.len()];
                if let Some(pos) = down.iter().position(|&x| x == key) {
                    down.remove(pos);
                    e.process(key, RELEASE);
                } else {
                    down.push(key);
                    e.process(key, PRESS);
                }
            }
            for key in down {
                e.process(key, RELEASE);
            }
            assert_clean(&e);
        }
    }
}
