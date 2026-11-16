//! Parity tests against Microsoft PowerToys Keyboard Manager.
//!
//! Oracle: src/modules/keyboardmanager/KeyboardManagerEngineTest/*.cpp in the
//! PowerToys repo. Each test names the upstream TEST_METHOD it ports. The
//! upstream tests assert the *virtual key state* after a sequence of inputs,
//! so these tests do the same through `Engine::virtual_down()`.

use std::collections::BTreeSet;

use evdev::Key;

use crate::engine::{Engine, Output, PRESS, RELEASE, REPEAT, Rules};
use crate::mapping::{KeyAction, KeyCombo, KeyMapping, KeyTrigger};

const A: u16 = Key::KEY_A.code();
const B: u16 = Key::KEY_B.code();
const C: u16 = Key::KEY_C.code();
const D: u16 = Key::KEY_D.code();
const V: u16 = Key::KEY_V.code();
const X: u16 = Key::KEY_X.code();
const TAB: u16 = Key::KEY_TAB.code();
const CAPS: u16 = Key::KEY_CAPSLOCK.code();
const LCTRL: u16 = Key::KEY_LEFTCTRL.code();
const LSHIFT: u16 = Key::KEY_LEFTSHIFT.code();
const LALT: u16 = Key::KEY_LEFTALT.code();
const LMETA: u16 = Key::KEY_LEFTMETA.code();
const RMETA: u16 = Key::KEY_RIGHTMETA.code();

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn engine(maps: Vec<KeyMapping>) -> Engine {
    let (rules, warnings) = Rules::compile(&maps);
    assert!(warnings.is_empty(), "{warnings:?}");
    Engine::new(rules)
}

/// Feed a sequence of (code, value) and return all outputs.
fn send(e: &mut Engine, seq: &[(u16, i32)]) -> Vec<Output> {
    seq.iter().flat_map(|&(c, v)| e.process(c, v)).collect()
}

fn down(e: &Engine) -> BTreeSet<u16> {
    e.virtual_down().collect()
}

fn set(v: &[u16]) -> BTreeSet<u16> {
    v.iter().copied().collect()
}

/// PowerToys `Shortcut` with VK_CONTROL etc. = either side. In the Rust model
/// that is the `CTRL`/`ALT`/`SHIFT`/`WIN` alias.
fn sc(mods: &[&str], key: &str) -> KeyCombo {
    KeyCombo::shortcut(s(mods), key)
}

// ---------------------------------------------------------------------------
// Single key remaps (SingleKeyRemappingTests.cpp)
// ---------------------------------------------------------------------------

#[test]
fn pt_remapped_key_should_set_target_key_state() {
    // RemappedKey_ShouldSetTargetKeyState_OnKeyEvent
    let mut e = engine(vec![KeyMapping::remap("KEY_A", "KEY_B")]);
    send(&mut e, &[(A, PRESS)]);
    assert_eq!(down(&e), set(&[B]));
    send(&mut e, &[(A, RELEASE)]);
    assert_eq!(down(&e), set(&[]));
}

#[test]
fn pt_remapped_key_disabled() {
    // RemappedKeyDisabled_ShouldNotChangeKeyState_OnKeyEvent
    let mut e = engine(vec![KeyMapping::disable("KEY_A")]);
    assert!(send(&mut e, &[(A, PRESS)]).is_empty());
    assert!(send(&mut e, &[(A, RELEASE)]).is_empty());
}

#[test]
fn pt_remapped_key_to_win_both_sets_left_win() {
    // RemappedKeyToWinBoth_ShouldSetWinLeftKeyState_OnKeyEvent
    let mut e = engine(vec![KeyMapping::remap("KEY_A", "WIN")]);
    send(&mut e, &[(A, PRESS)]);
    assert_eq!(down(&e), set(&[LMETA]));
    send(&mut e, &[(A, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_capslock_to_ctrl_sends_exactly_one_event() {
    // HandleSingleKeyRemapEvent_ShouldSendVirtualInputWithSuppressFlagExactlyOnce_WhenCapsLockIsMappedToCtrlAltShift
    let mut e = engine(vec![KeyMapping::remap("KEY_CAPSLOCK", "KEY_LEFTCTRL")]);
    assert_eq!(e.process(CAPS, PRESS).len(), 1);
}

#[test]
fn pt_ctrl_to_capslock_sends_exactly_one_event() {
    // ..._WhenCtrlAltShiftIsMappedToCapsLock
    let mut e = engine(vec![KeyMapping::remap("KEY_LEFTCTRL", "KEY_CAPSLOCK")]);
    assert_eq!(
        e.process(LCTRL, PRESS),
        vec![Output::Key {
            code: CAPS,
            value: PRESS
        }]
    );
}

#[test]
fn pt_capslock_to_ctrl_shift_v_sends_three_events() {
    // ..._ExactlyTwice_WhenCapsLockIsMappedToShortcutWithCtrlAltShift (2 modifiers + V)
    let mut e = engine(vec![KeyMapping::remap_to_combo(
        "KEY_CAPSLOCK",
        s(&["KEY_LEFTCTRL", "KEY_LEFTSHIFT"]),
        "KEY_V",
    )]);
    send(&mut e, &[(CAPS, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, LSHIFT, V]));
    send(&mut e, &[(CAPS, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_remapped_key_to_two_key_shortcut() {
    // RemappedKeyToTwoKeyShortcut_ShouldSetTargetKeyState_OnKeyEvent: A -> Ctrl+V
    let mut e = engine(vec![KeyMapping::remap_to_combo(
        "KEY_A",
        s(&["CTRL"]),
        "KEY_V",
    )]);
    send(&mut e, &[(A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, V]));
    send(&mut e, &[(A, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_remapped_key_to_shortcut_containing_source_key() {
    // RemappedKeyToShortcutContainingSourceKey_ShouldSetTargetKeyState_OnKeyEvent: LCtrl -> LCtrl+V
    let mut e = engine(vec![KeyMapping::remap_to_combo(
        "KEY_LEFTCTRL",
        s(&["KEY_LEFTCTRL"]),
        "KEY_V",
    )]);
    send(&mut e, &[(LCTRL, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, V]));
    send(&mut e, &[(LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_chained_single_key_remaps_do_not_loop() {
    // KBM ignores its own injected events, so A->B, B->C maps A to B (not C).
    let mut e = engine(vec![
        KeyMapping::remap("KEY_A", "KEY_B"),
        KeyMapping::remap("KEY_B", "KEY_C"),
    ]);
    send(&mut e, &[(A, PRESS)]);
    assert_eq!(down(&e), set(&[B]));
    send(&mut e, &[(A, RELEASE), (B, PRESS)]);
    assert_eq!(down(&e), set(&[C]));
}

#[test]
fn pt_shortcut_output_is_not_remapped_again() {
    // Alt+A -> Ctrl+C and Ctrl+C -> Ctrl+V: the injected Ctrl+C must not chain.
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["ALT"], "KEY_A"), sc(&["CTRL"], "KEY_C")),
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_C"), sc(&["CTRL"], "KEY_V")),
    ]);
    send(&mut e, &[(LALT, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, C]));
}

#[test]
fn pt_key_repeat_of_remapped_key_repeats_target() {
    let mut e = engine(vec![KeyMapping::remap("KEY_A", "KEY_B")]);
    send(&mut e, &[(A, PRESS)]);
    assert_eq!(
        e.process(A, REPEAT),
        vec![Output::Key {
            code: B,
            value: REPEAT
        }]
    );
}

// Single key -> text (SingleKeyToTextRemapTests)
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_key_to_text_releases_held_ctrl_and_never_represses_it() {
    // HandleSingleKeyToTextRemapEvent_ShouldReleaseCtrlAndNotRestore_WhenCtrlIsHeld
    // HandleSingleKeyToTextRemapEvent_ShouldNeverRePressModifier_WhenModifierIsHeld
    let mut e = engine(vec![KeyMapping::send_text("KEY_X", "hello")]);
    send(&mut e, &[(LCTRL, PRESS)]);
    let out = e.process(X, PRESS);
    let represses = out
        .iter()
        .filter(|o| {
            matches!(
                o,
                Output::Key {
                    code: LCTRL,
                    value: PRESS
                }
            )
        })
        .count();
    assert_eq!(represses, 0, "Ctrl re-pressed after text: {out:?}");
    assert!(!down(&e).contains(&LCTRL), "Ctrl must be left released");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_key_to_text_releases_held_win_and_never_represses_it() {
    // HandleSingleKeyToTextRemapEvent_ShouldReleaseWinKeyAndNotRestore_WhenWinKeyIsHeld
    let mut e = engine(vec![KeyMapping::send_text("KEY_X", "hello")]);
    send(&mut e, &[(LMETA, PRESS), (X, PRESS)]);
    assert!(!down(&e).contains(&LMETA), "Win must be left released");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_key_to_text_supports_unicode() {
    // PowerToys sends text with KEYEVENTF_UNICODE, so any character can be typed.
    let (_, warnings) = Rules::compile(&[KeyMapping::send_text("KEY_X", "héllo €")]);
    assert!(
        warnings.is_empty(),
        "non-ASCII text is dropped: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Shortcut remaps (OSLevelShortcutRemappingTests.cpp)
// ---------------------------------------------------------------------------

#[test]
fn pt_two_key_shortcut_diff_modifiers_keydown_and_keyup() {
    // RemappedTwoKeyShortcutWithDiffModifiers_ShouldSetTargetShortcutDown_OnKeyDown / _ShouldClearKeyboard_OnKeyUp
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["ALT"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LALT, V]));
    send(&mut e, &[(A, RELEASE), (LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_two_key_shortcut_same_modifiers() {
    // RemappedTwoKeyShortcutWithSameModifiers_ShouldSetTargetShortcutDown_OnKeyDown
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["CTRL"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, V]));
    send(&mut e, &[(A, RELEASE), (LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_shortcut_with_extra_modifier_is_not_remapped() {
    // RemappedTwoKeyShortcutInvokingAShortcutContainingThoseKeys_ShouldNotBeRemapped_OnKeyDown
    // Ctrl+A -> Alt+V; user presses Ctrl+Shift+A. PowerToys requires the
    // keyboard to be clear except the shortcut for shortcut->shortcut remaps
    // (IsKeyboardStateClearExceptShortcut), even without "exact match".
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["ALT"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (LSHIFT, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, LSHIFT, A]));
}

#[test]
fn pt_three_key_shortcut_subset_is_not_remapped() {
    // RemappedThreeKeyShortcutInvokingAShortcutSubsetOfThoseKeys_ShouldNotBeRemapped_OnKeyDown
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL", "SHIFT"], "KEY_A"),
        sc(&["ALT"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, A]));
}

#[test]
fn pt_three_key_to_two_key_partially_diff_modifiers() {
    // RemappedThreeKeyToTwoKeyShortcutWithPartiallyDiffModifiers: Ctrl+Shift+A -> Ctrl+V
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL", "SHIFT"], "KEY_A"),
        sc(&["CTRL"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (LSHIFT, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, V]));
    send(&mut e, &[(A, RELEASE), (LSHIFT, RELEASE), (LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_two_key_to_three_key_diff_modifiers() {
    // RemappedTwoKeyToThreeKeyShortcutWithDiffModifiers: Ctrl+A -> Alt+Shift+V
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["ALT", "SHIFT"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LALT, LSHIFT, V]));
    send(&mut e, &[(A, RELEASE), (LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_unremapped_shortcut_after_remapped_with_same_modifier() {
    // InvokingUnremappedShortcutAfterRemappedShortcutWithSameModifier_ShouldSetUnremappedShortcut_OnKeyDown
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["ALT", "SHIFT"], "KEY_V"),
    )]);
    send(
        &mut e,
        &[(LCTRL, PRESS), (A, PRESS), (A, RELEASE), (C, PRESS)],
    );
    assert_eq!(down(&e), set(&[LCTRL, C]));
}

#[test]
fn pt_win_both_modifier_in_source_and_target() {
    // RemappedShortcutWithWinBothModifier_ShouldSetRemappedShortcut_OnKeyEvent
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["WIN"], "KEY_A"), sc(&["ALT"], "KEY_V")),
        KeyMapping::shortcut_remap(sc(&["ALT"], "KEY_D"), sc(&["WIN"], "KEY_B")),
    ]);
    for win in [LMETA, RMETA] {
        send(&mut e, &[(win, PRESS), (A, PRESS)]);
        assert_eq!(down(&e), set(&[LALT, V]));
        send(&mut e, &[(A, RELEASE), (win, RELEASE)]);
        assert!(down(&e).is_empty());
    }
    send(&mut e, &[(LALT, PRESS), (D, PRESS)]);
    assert_eq!(down(&e), set(&[LMETA, B]));
    send(&mut e, &[(D, RELEASE), (LALT, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_unremapped_shortcut_with_rwin_after_win_both_remap() {
    // InvokingUnremappedShortcutWithRWinAfterRemappedShortcutWithWinBothModifier
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["WIN"], "KEY_A"),
        sc(&["ALT"], "KEY_V"),
    )]);
    send(
        &mut e,
        &[(RMETA, PRESS), (A, PRESS), (A, RELEASE), (C, PRESS)],
    );
    assert_eq!(down(&e), set(&[RMETA, C]));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_target_modifier_stays_held_after_action_key_release_alt_tab() {
    // RemappedShortcutModifiers_ShouldBeDetectedAsPressed_OnReleasingActionKeyButHoldingModifiers
    // Ctrl+A -> Alt+Tab; release A while holding Ctrl: Alt must stay down so
    // the window switcher stays open (Alt+Tab / Win+Space use case).
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["ALT"], "KEY_TAB"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS), (A, RELEASE)]);
    assert_eq!(down(&e), set(&[LALT]));
    let _ = TAB;
}

#[test]
fn pt_two_remapped_shortcuts_sharing_modifier_in_succession() {
    // TwoRemappedShortcutsWithDifferentModifiersThatShareModifiers_...
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["ALT"], "KEY_A"), sc(&["CTRL"], "KEY_C")),
        KeyMapping::shortcut_remap(sc(&["ALT"], "KEY_V"), sc(&["CTRL"], "KEY_X")),
    ]);
    send(
        &mut e,
        &[(LALT, PRESS), (A, PRESS), (A, RELEASE), (V, PRESS)],
    );
    assert_eq!(down(&e), set(&[LCTRL, X]));
}

#[test]
fn pt_two_remapped_shortcuts_same_modifiers_in_succession() {
    // TwoRemappedShortcutsWithSameModifiersThatShareModifiers_...
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), sc(&["CTRL"], "KEY_C")),
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_V"), sc(&["CTRL"], "KEY_X")),
    ]);
    send(
        &mut e,
        &[(LCTRL, PRESS), (A, PRESS), (A, RELEASE), (V, PRESS)],
    );
    assert_eq!(down(&e), set(&[LCTRL, X]));
}

// Shortcut -> single key

#[test]
fn pt_shortcut_to_key_not_containing_that_key() {
    // RemappedTwoKeyShortcutToSingleKeyNotContainingThatKey_ShouldSetCorrectKeyStates_OnKeyEvents
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        KeyCombo::key("KEY_LEFTALT"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LALT]));
    send(&mut e, &[(A, RELEASE), (LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_shortcut_to_key_release_action_key_does_not_restore_modifier() {
    // RemappedShortcutToSingleKeyWhereKeyIsNotInShortcut_ShouldNotSetOriginalModifier_OnReleasingActionKey
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        KeyCombo::key("KEY_LEFTALT"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS), (A, RELEASE)]);
    assert_eq!(down(&e), set(&[]), "Ctrl must not be re-pressed yet");
    // ..._IfActionKeyIsReleasedAndThenOtherKeysArePressedAfterIt: B restores Ctrl.
    send(&mut e, &[(B, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, B]));
}

#[test]
fn pt_shortcut_to_key_with_other_key_pressed_before() {
    // RemappedShortcutToSingleKey_ShouldBeInvoked_IfOtherKeysArePressedAlongWithIt
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        KeyCombo::key("KEY_LEFTALT"),
    )]);
    send(&mut e, &[(B, PRESS), (LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[B, LALT]));
    // ..._ShouldRevertBackToPhysicalKeys_IfOtherKeysArePressedAlongWithItAndThenActionKeyIsReleased
    send(&mut e, &[(A, RELEASE)]);
    assert_eq!(down(&e), set(&[B, LCTRL]));
}

#[test]
fn pt_shortcut_to_key_other_key_after_then_modifier_released() {
    // RemappedShortcutToSingleKey_ShouldBeInvokedAndThenRevertToPhysicalKeys_IfOtherKeysArePressedAfterItAndModifierKeyIsReleased
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        KeyCombo::key("KEY_LEFTALT"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS), (B, PRESS)]);
    assert_eq!(down(&e), set(&[LALT, B]));
    send(&mut e, &[(LCTRL, RELEASE)]);
    assert_eq!(down(&e), set(&[B]));
}

#[test]
fn pt_larger_shortcut_wins_over_smaller() {
    // RemappedShortcutToSingleKey_ShouldNotBeInvoked_IfALargerRemappedShortcutToShortcutContainingThoseShortcutKeysIsInvoked
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), KeyCombo::key("KEY_LEFTALT")),
        KeyMapping::shortcut_remap(sc(&["CTRL", "SHIFT"], "KEY_A"), sc(&["CTRL"], "KEY_V")),
    ]);
    send(&mut e, &[(LSHIFT, PRESS), (LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, V]));
}

#[test]
fn pt_larger_shortcut_to_key_wins() {
    // ..._IfALargerRemappedShortcutToKeyContainingThoseShortcutKeysIsInvoked
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), KeyCombo::key("KEY_LEFTALT")),
        KeyMapping::shortcut_remap(sc(&["CTRL", "SHIFT"], "KEY_A"), KeyCombo::key("KEY_B")),
    ]);
    send(&mut e, &[(LSHIFT, PRESS), (LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[B]));
}

#[test]
fn pt_shortcut_to_win_both_sets_left_win() {
    // RemappedShortcutToWinBoth_ShouldSetLWinKeyState_OnKeyEvent
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        KeyCombo::key("WIN"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LMETA]));
}

#[test]
fn pt_unrelated_key_up_does_not_reset_shortcut() {
    // ShortcutRemap_ShouldNotGetReset_OnSendingKeyUpForAKeyNotPresentInTheShortcutAfterInvokingTheShortcut
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["CTRL"], "KEY_A"),
        sc(&["CTRL"], "KEY_V"),
    )]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS), (B, RELEASE)]);
    assert_eq!(down(&e), set(&[LCTRL, V]));
}

// Shortcut -> Disable

fn disabled_ctrl_a() -> KeyMapping {
    KeyMapping {
        trigger: KeyTrigger::combo(s(&["CTRL"]), "KEY_A"),
        action: KeyAction::Disable,
        app_filter: None,
        enabled: true,
        exact_match: false,
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_shortcut_disable_on_exact_match_releases_modifier() {
    // ShortcutDisable_ShouldDisableShortcut_OnExactMatch: Ctrl and A both false.
    let mut e = engine(vec![disabled_ctrl_a()]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[]));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_shortcut_disable_not_on_superset() {
    // ShortcutDisable_ShouldNotDisableShortcut_OnSubsetMatch: Ctrl+Shift+A passes.
    let mut e = engine(vec![disabled_ctrl_a()]);
    send(&mut e, &[(LCTRL, PRESS), (LSHIFT, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, LSHIFT, A]));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_shortcut_disable_then_other_key_passes_through() {
    // ShortcutDisable_ShouldNotDisableShortcutSuperset_AfterShortcutWasDisabled:
    // Ctrl+A (disabled) then B -> Ctrl, A and B all down.
    let mut e = engine(vec![disabled_ctrl_a()]);
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS), (B, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, A, B]));
}

#[test]
fn pt_shortcut_disable_release_action_then_other_key() {
    // ShortcutDisable_ShouldNotDisableShortcutSuperset_AfterActionKeyWasReleasedAndAnotherKeyWasPressedAfterIt
    let mut e = engine(vec![disabled_ctrl_a()]);
    send(
        &mut e,
        &[(LCTRL, PRESS), (A, PRESS), (A, RELEASE), (B, PRESS)],
    );
    assert_eq!(down(&e), set(&[LCTRL, B]));
}

// Dummy key events (Win/Alt menu suppression)

/// True if the output contains a press of `m` immediately followed by its
/// release, i.e. a "lone tap" of the modifier that the desktop interprets as
/// an activation (GNOME/KDE open the overview/launcher on a lone Super tap;
/// many apps focus the menu bar on a lone Alt tap).
fn has_lone_tap(all: &[Output], m: u16) -> bool {
    let keys: Vec<(u16, i32)> = all
        .iter()
        .filter_map(|o| match o {
            Output::Key { code, value } if *value != REPEAT => Some((*code, *value)),
            _ => None,
        })
        .collect();
    keys.windows(2)
        .any(|w| w[0] == (m, PRESS) && w[1] == (m, RELEASE))
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_win_shortcut_remap_does_not_produce_lone_win_tap() {
    // HandleShortcutRemapEvent_ShouldSendOneSetOfDummyKeyEventsBeforeReleasingTheModifier_
    // WhenShortcutIsRemappedToAShortcutNotContainingOriginalShortcutModifiersOnInvoke
    // Win+A -> Ctrl+V: PowerToys injects a dummy key while Win is down before
    // releasing it, so the Start menu does not open.
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["WIN"], "KEY_A"),
        sc(&["CTRL"], "KEY_V"),
    )]);
    let out = send(&mut e, &[(LMETA, PRESS), (A, PRESS)]);
    assert!(
        !has_lone_tap(&out, LMETA),
        "lone Super tap emitted: {out:?}"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn pt_win_shortcut_to_key_does_not_produce_lone_win_tap() {
    // ..._ShouldSendOneSetOfDummyKeyEventsBeforeReleasingTheModifier_WhenShortcutIsRemappedToASingleKeyOnInvoke
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        sc(&["WIN"], "KEY_A"),
        KeyCombo::key("KEY_V"),
    )]);
    let out = send(
        &mut e,
        &[(LMETA, PRESS), (A, PRESS), (A, RELEASE), (LMETA, RELEASE)],
    );
    assert!(
        !has_lone_tap(&out, LMETA),
        "lone Super tap emitted: {out:?}"
    );
}

// ---------------------------------------------------------------------------
// App-specific shortcuts (AppSpecificShortcutRemappingTests.cpp)
// ---------------------------------------------------------------------------

#[test]
fn pt_app_specific_shortcut_in_foreground() {
    // AppSpecificShortcut_ShouldGetRemapped_WhenAppIsInForeground
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), sc(&["ALT"], "KEY_V"))
            .for_app("testprocess"),
    ]);
    e.set_focused_app(&s(&["TestProcess"]));
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LALT, V]));
}

#[test]
fn pt_app_specific_shortcut_not_in_foreground() {
    // AppSpecificShortcut_ShouldNotGetRemapped_WhenAppIsNotInForeground
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), sc(&["ALT"], "KEY_V"))
            .for_app("testprocess"),
    ]);
    e.set_focused_app(&s(&["other"]));
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[LCTRL, A]));
}

#[test]
fn pt_app_specific_clears_keys_when_foreground_changes_before_release() {
    // AppSpecificShortcut_ShouldClearKeyStates_WhenForegroundAppChangesAfterShortcutIsPressedOnRelease
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), sc(&["ALT"], "KEY_V"))
            .for_app("testprocess"),
    ]);
    e.set_focused_app(&s(&["testprocess"]));
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    e.set_focused_app(&s(&["other"]));
    send(&mut e, &[(A, RELEASE), (LCTRL, RELEASE)]);
    assert!(down(&e).is_empty());
}

#[test]
fn pt_app_specific_disable() {
    // AppSpecificShortcutToDisable_ShouldDisable_WhenAppIsOnForeground
    let mut m = disabled_ctrl_a().for_app("testprocess");
    m.exact_match = false;
    let mut e = engine(vec![m]);
    e.set_focused_app(&s(&["testprocess"]));
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert!(!down(&e).contains(&A));
}

#[test]
fn pt_app_specific_beats_global_shortcut() {
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), KeyCombo::key("KEY_B")),
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), KeyCombo::key("KEY_C")).for_app("code"),
    ]);
    e.set_focused_app(&s(&["code"]));
    send(&mut e, &[(LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[C]));
}

// ---------------------------------------------------------------------------
// Exact match / chords
// ---------------------------------------------------------------------------

#[test]
fn pt_exact_match_shortcut_to_key_rejects_extra_key() {
    // KeyboardEventHandlers.cpp: exactMatch && !IsKeyboardStateClearExceptShortcut -> skip
    let mut e = engine(vec![
        KeyMapping::shortcut_remap(sc(&["CTRL"], "KEY_A"), KeyCombo::key("KEY_LEFTALT")).exact(),
    ]);
    send(&mut e, &[(B, PRESS), (LCTRL, PRESS), (A, PRESS)]);
    assert_eq!(down(&e), set(&[B, LCTRL, A]));
}

#[test]
fn pt_chord_ctrl_k_then_d() {
    let mut e = engine(vec![KeyMapping::shortcut_remap(
        KeyCombo::chord(s(&["CTRL"]), "KEY_K", "KEY_D"),
        KeyCombo::key("KEY_B"),
    )]);
    send(
        &mut e,
        &[
            (LCTRL, PRESS),
            (Key::KEY_K.code(), PRESS),
            (Key::KEY_K.code(), RELEASE),
            (D, PRESS),
        ],
    );
    assert_eq!(down(&e), set(&[B]));
}

// ---------------------------------------------------------------------------
// Worker without /dev/uinput
// ---------------------------------------------------------------------------

#[test]
fn worker_start_without_uinput_fails_gracefully() {
    if std::path::Path::new("/dev/uinput").exists() {
        return; // only meaningful in a sandbox without uinput
    }
    let (rules, _) = Rules::compile(&[KeyMapping::remap("KEY_A", "KEY_B")]);
    let err = match crate::worker::start(rules) {
        Ok(_) => panic!("worker started without /dev/uinput"),
        Err(e) => format!("{e:#}"),
    };
    assert!(err.contains("uinput"), "unhelpful error: {err}");
}
