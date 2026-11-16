# Group F-input report: Key Manager, Quick Accent, Shortcut Guide, Mouse Utilities (+ Mouse Without Borders)

Rust repo: this repository at commit `3a4491e`. Reference: microsoft/PowerToys (main).
Screenshots: shots/F-input/. No existing non-test source was modified. The only change to existing files is a `#[cfg(test)] mod parity_tests;` line added to each crate's lib.rs.

Environment notes:
- /dev/uinput and /dev/input do not exist in the sandbox.
- xdotool and xrandr are missing. Monitor detection (`mpt_common::monitor::detect_monitors`) shells out to `xrandr`, so for the Mouse Jump tests I used a fake `xrandr` at .deps/fakebin-F-input/xrandr that prints a chosen layout.
- egui apps rendered under Xvfb with `LIBGL_ALWAYS_SOFTWARE=1`. The iced settings app rendered with `ICED_BACKEND=tiny-skia`.
- Input was injected with XTest, either from the Rust tests (x11rb xtest) or from .deps/click-F-input.py (ctypes against libXtst).

---------------------------------------------------------------------------------------------------

## 1. Key Manager (crates/key-manager vs src/modules/keyboardmanager)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Key → key remap | Implemented (passes PT tests) |
| Key → shortcut | Implemented |
| Key → text | Partial: US-ASCII only, other characters are dropped (bug 6). Modifiers are re-pressed afterwards (bug 5) |
| Shortcut → key / shortcut | Implemented, with semantic differences (bugs 1–4) |
| Shortcut → text | Implemented (ASCII only) |
| Disable key / disable shortcut | Key: implemented. Shortcut: differs (bug 3) |
| Run program (args, start-in, elevation) | Partial: no env-var expansion, no "if running" (show window / start another / close / end task), no window visibility option |
| Open URI | Implemented (xdg-open) |
| App-specific shortcuts | Implemented on X11 (WM_CLASS / process name, exact match only). Skipped on Wayland |
| Chords (Ctrl+K, D) | Implemented |
| Exact match flag | Implemented, but PT applies exact matching implicitly to shortcut→shortcut and shortcut→disable (bug 1) |
| Left/Right vs Either modifiers, Win "Both" | Implemented (`CTRL`/`ALT`/`SHIFT`/`WIN` aliases; Either outputs the left key, as in PT) |
| Remapped modifier used in shortcuts (CapsLock→Ctrl, then Ctrl+H) | Implemented |
| No infinite loop when the remap target is itself remapped | Implemented (A→B, B→C: A gives B; shortcut output is not re-matched) |
| Key repeat | Implemented (the target repeats) |
| "Alone" tap remap (dual-role key, newer PT) | Missing |
| Dummy-key suppression of Start menu / lone Win tap | Missing (bug 2) |
| AltGr handling | N/A (evdev level) |
| Editor UI (Remap keys / Remap shortcuts dialogs, key capture) | Missing: the settings page can only add a hardcoded mapping, toggle and delete (bug 7) |

### Bugs
1. **Medium.** Non-exact shortcuts match even when extra modifiers are held. engine.rs:567-590 (`match_mods` only rejects extra keys when `rule.exact` is set). PT always requires `IsKeyboardStateClearExceptShortcut` for shortcut→shortcut and shortcut→disable (KeyboardEventHandlers.cpp ~line 710). With Ctrl+A→Alt+V, pressing Ctrl+Shift+A fires the remap in Rust and gives Shift+Alt+V. PT leaves it as Ctrl+Shift+A.
   - Evidence: `pt_shortcut_with_extra_modifier_is_not_remapped` FAILED (`left: {42,47,56}` = Shift,V,Alt; `right: {29,30,42}`). `pt_shortcut_disable_not_on_superset` FAILED (`{29,42}` vs `{29,30,42}`).
2. **Medium.** A lone Super tap leaks to the desktop when a Win shortcut is remapped. engine.rs fire() / end_active() (lines ~650-700). Win+A→Ctrl+V emits LMETA press, then LMETA release, with no key in between. GNOME/KDE treat that as a Super tap and open the overview. PT sends a dummy key (0xFF) before releasing or after re-pressing Win.
   - Evidence: `pt_win_shortcut_remap_does_not_produce_lone_win_tap` and `pt_win_shortcut_to_key_does_not_produce_lone_win_tap` FAILED.
   - The same pattern applies to Alt (menu-bar activation). That case is suspected, not tested in an app.
3. **Medium.** Disabled-shortcut semantics differ. engine.rs:674 (`Effect::Disable => Swallow{restore:false}`).
   - PT (ShortcutDisable_ShouldDisableShortcut_OnExactMatch) releases Ctrl as well; Rust keeps Ctrl down. Evidence: `pt_shortcut_disable_on_exact_match_releases_modifier` FAILED (`{29}` vs `{}`).
   - PT (ShortcutDisable_ShouldNotDisableShortcutSuperset_AfterShortcutWasDisabled): Ctrl+A (disabled) followed by B gives Ctrl+A+B. Rust gives Ctrl+B and A is lost. Evidence: `pt_shortcut_disable_then_other_key_passes_through` FAILED.
4. **Medium.** Target modifiers are released when the action key is released, which breaks Alt+Tab-style remaps. engine.rs:692-697 (`end_active` → `release_combo` + `restore_modifiers`). PT keeps the target modifiers held while the trigger modifier is held (RemappedShortcutModifiers_ShouldBeDetectedAsPressed_OnReleasingActionKeyButHoldingModifiers). With Ctrl+A→Alt+Tab, releasing A closes the switcher in Rust. The existing Rust test `shortcut_remap_alt_c_to_ctrl_c` asserts the non-PT behaviour.
   - Evidence: `pt_target_modifier_stays_held_after_action_key_release_alt_tab` FAILED (`{29}` vs `{56}`).
   - Related, low severity: for shortcut→key, PT does not re-press Ctrl until another key is pressed; Rust re-presses it immediately. Evidence: `pt_shortcut_to_key_release_action_key_does_not_restore_modifier` FAILED.
5. **Low/Medium.** Key→text re-presses held modifiers. engine.rs:527 (`restore_modifiers()` after `type_text`). PT releases Ctrl/Win/Alt and never re-presses them, so they cannot get stuck.
   - Evidence: `pt_key_to_text_releases_held_ctrl_and_never_represses_it` FAILED (output ends with `Key{29,1}`). `pt_key_to_text_releases_held_win_and_never_represses_it` FAILED.
6. **Medium.** Send-text only supports US-ASCII through a US keymap. keymap.rs:1-120. Characters like é and € are skipped with a warning. PT sends any Unicode character (KEYEVENTF_UNICODE). Text is also wrong on non-US layouts (documented in the code).
   - Evidence: `pt_key_to_text_supports_unicode` FAILED.
7. **High (UI).** The Key Manager settings page cannot create or edit real mappings. ui/src/views.rs:4234 and 4283.
   - "Add key remap" always appends CapsLock→Esc. "Add shortcut remap" always appends **Ctrl+C → Ctrl+V** and immediately reloads the engine, so after one click Ctrl+C pastes instead of copying.
   - There is no key-capture or edit UI.
   - Evidence: clicking "Ajouter un remap de touche" wrote `trigger="KEY_CAPSLOCK" → KEY_ESC` to key-manager.toml. Screenshot: settings-keymanager-added.png.
8. **Low.** Run-program differences. actions.rs:20-50. There is no `$VAR` expansion (PT EnvironmentVariable_ShouldExpandBeforeResolvingPath). An empty working directory inherits the daemon's cwd, where PT uses the executable's directory. There are no "if running" actions. Found by code reading, no test.

/dev/uinput missing is handled gracefully. `worker::start` returns an error containing "/dev/uinput not found: load the module with `sudo modprobe uinput` …" plus the permission help text. `KeyManager::start` propagates it and the daemon's `start_all` only logs a warning. Test: `worker_start_without_uinput_fails_gracefully` passed.

### UI differences vs PowerToys
- PT has a dedicated editor window: a mapping list with "Remap a key" and "Remap a shortcut" dialogs, key capture, "Select" buttons, a target-app field, and action types (Key/Shortcut, Text, Run program, Open URI).
- Rust shows the same sections (Keys / Shortcuts). Each row has keycaps, an "All apps" badge, a toggle and a delete button. There is no editor.
- The UI starts in French on a fresh HOME ("Tableau de bord", "Touches", "Raccourcis"; settings-home.png).
- Rows whose trigger is a `Combo` without modifiers are listed under "Shortcuts".

### Tests added
- crates/key-manager/src/parity_tests.rs: 48 tests, **37 pass / 11 fail**. Each test names the PT TEST_METHOD it ports.
- Existing crate tests: 55/55 pass.

---------------------------------------------------------------------------------------------------

## 2. Quick Accent (crates/quick-accent vs src/modules/poweraccent)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Activation key Space / Left-Right / Both / Press-and-hold | Implemented. Default is wrong (bug 2). Arrow activation is broken (bug 4) |
| Input delay (InputTime, default 300 ms) | Implemented, default 200 (bug 2) |
| Hold duration 500 ms | Implemented |
| Toolbar position (9 PT positions) | Implemented, plus AboveCursor/BelowCursor. Default differs. Edge offset is 20, PT uses 24 |
| Languages | Partial: 14 of PT's 47 sets, incomplete tables (bug 1) |
| Non-letter keys (comma quotes, digits, `/`, SPECIAL, currency, IPA …) | Missing |
| Shift / CapsLock uppercase | Broken (bug 3) |
| Sort by usage frequency | Implemented (count only; PT also breaks ties by last-use timestamp and clears usage when the option is off) |
| Exclusion list | Partial: exact WM_CLASS only. PT matches a process-name prefix or a window-title substring |
| Show Unicode description | Implemented. Format is `U+00E9 NAME`, PT uses `(U+00E9) NAME` |
| Start selection from the left | Implemented |
| Do not activate in game mode | Implemented (fullscreen / Feral gamemoded) |
| Release the letter to insert | Missing: Enter or click is required (bug 5) |
| Settings page / module toggle in the settings app | Missing (bug 6) |

### Bugs
1. **Medium.** Character tables are incomplete or in a different order compared with CharacterMappings.cs.
   - 13 of 14 compared languages differ. Only RO matches.
   - Examples:
     - FR 'a': upstream [à â á ä ã æ], Rust [à â æ]
     - '€' is missing from every language
     - ES ḥ ḷ missing
     - PT ª º $ missing
     - NL is missing ç ñ è ê î ô û
     - TK is missing â ë ô û ₺
     - Order differs for CZ ě/é and ů/ú, PL ż/ź, HU ő/ö and ű/ü
   - When several languages are selected, Rust merges them in selection order. PT merges in alphabetical DisplayOrder (`["es","de"]` 'u' gives [ú ü], PT gives [ü ú]).
   - Evidence: `table_*_matches_upstream` (13 FAILED, mismatch lists in the test output), `language_count_matches_upstream` (14 vs 47), `non_letter_keys_have_accents`, `multi_language_merge_uses_display_order_not_selection_order`.
2. **Low.** Defaults differ from PowerAccentProperties.cs (config.rs:177, 189, 7, 23):
   - Activation: PT Both, Rust Space
   - Toolbar: PT "Top center", Rust AboveCursor
   - Input delay: PT 300 ms, Rust 200 ms
   - Languages: PT ALL, Rust fr/es/de/pt/it
   - Edge offset: PT 24, Rust 20 (config.rs:95)
   - Evidence: the `default_*` tests and `toolbar_edge_offset_is_24` FAILED.
3. **Medium.** Uppercase is wrong or never used.
   - characters.rs:15 uses `to_uppercase().next()`. 'ß' becomes 'S' (PT: ẞ) and Turkish 'ı' becomes plain 'I' (PT: İ). Evidence: `uppercase_sharp_s_is_capital_sharp_s` (got ['S']) and `uppercase_dotless_i_is_dotted_capital_i` (got 'I') FAILED.
   - The monitor never passes an uppercase letter anyway. monitor.rs:396 `keysym_to_char` always lower-cases and Shift/CapsLock state is never read, so Shift+E offers é instead of É. Confirmed by code reading.
4. **High.** Left/Right arrow activation deletes the wrong character. monitor.rs:168 adds 1 extra backspace only for Space. The monitor listens to XI2 raw events passively and does not swallow keys, so the arrow key really moves the caret in the target app. The backspaces sent later (main.rs:81) then delete the character next to the letter instead of the letter. PT swallows the activation keys in its low-level hook. Confirmed by code reading; it could not be run because xdotool is unavailable.
5. **High (env/UX).** The overlay depends entirely on `xdotool` with silent failure (overlay.rs:290, 300-330; main.rs:81-92). It uses xdotool for the cursor position, for activating the window, and for typing backspaces and the character. Without xdotool the toolbar opens at a default position, and the chosen character is never typed; the `.status()` result is discarded.
   - Monitor detection also needs `xrandr`. Without it the toolbar is placed for an assumed 1920x1080 screen: on the 1600x1000 Xvfb, "top-center" landed at x≈790 instead of 630 (quick-accent.png).
   - Selection requires Enter or a click. PT inserts the selected character when the letter key is released.
6. **Medium (UI).** Quick Accent is missing from the settings app. ui/src/modules.rs has no "quick-accent" entry (27 modules, the list ends at command-not-found), and there is no settings page. It cannot be enabled or configured from the GUI. Screenshot: settings-mouseutils2.png shows the full sidebar.
7. **Low.** Ctrl/Alt/Win held with the letter still triggers the toolbar. PT `IsBlockingModifierDown` blocks it. Found by code reading (monitor.rs Idle state).

### UI differences
- The toolbar (quick-accent.png) has a dark rounded bar, a selected cell, a description row and an extra English hint line ("Space/Arrows: navigate Enter: confirm Esc: cancel"). PT has no hint line and uses the system theme/accent.
- It rendered correctly, with no crash.

### Tests added
- crates/quick-accent/src/parity_tests.rs: 31 tests, **6 pass / 25 fail**.
- Existing crate tests: 17/17 pass.

---------------------------------------------------------------------------------------------------

## 3. Shortcut Guide (crates/shortcut-guide vs src/modules/ShortcutGuide)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Activation shortcut Win+Shift+/ | Implemented ("Super+?") |
| Windows key action Off / Taskbar indicators / Open guide | Implemented. Taskbar indicators is N/A on Linux and behaves like Off |
| Press time 900 ms (100–5000) | Implemented, clamped |
| Close on Win release | Implemented |
| Theme light/dark/system | Implemented (light screenshot OK) |
| Window position Left/Right | Implemented, plus Center. Default is wrong (bug 1) |
| Disabled apps | Partial: exact WM_CLASS only. PT matches a process-name prefix or title substring (bug 2) |
| Overlay contents | Partial: 14 static GNOME shortcuts plus custom entries. PT shows per-app shortcut manifests, search box, navigation pane, pinned shortcuts and settings button |
| Opacity | N/A: the current PT has no opacity option (the old v1 overlay_opacity was removed) |

### Bugs
1. **Low.** The default window position is Center; PT defaults to Left. config.rs:43. Evidence: `default_window_position_is_left` FAILED.
2. **Low.** Disabled-apps matching is exact only. config.rs:152. PT's "steam" disables steamwebhelper.exe. Evidence: `disabled_apps_match_like_upstream_prefix` FAILED.
3. **Low (rendering).** The header background fills only the title's width, not the full panel. A long description is truncated ("Move windo…"). Screenshots: shortcut-guide.png and shortcut-guide-light-right.png.
4. **Suspected, low.** On a secondary monitor the overlay is placed relative to (0,0). overlay.rs:322-326 uses only the monitor size, not its origin. Not run (single-screen Xvfb).
5. Shortcut Guide has a sidebar entry in the settings app but no settings page ("no settings" fallback, views.rs ~2589), so none of the options above can be changed from the GUI.

### Tests added
- crates/shortcut-guide/src/parity_tests.rs: 8 tests, **6 pass / 2 fail**.
- Existing crate tests: 22/22 pass.
- The overlay launched fine (dark/centre and light/right) and closed with the X server. The only panic was winit's X connection error during Xvfb teardown, which is not a module bug.

---------------------------------------------------------------------------------------------------

## 4. Mouse Utilities (crates/mouse-utils vs src/modules/MouseUtils)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Find My Mouse: double LCtrl / double RCtrl / shake / custom shortcut | Implemented |
| FMM spotlight radius / animation / zoom / shake params | Implemented. Initial zoom default is 4, PT uses 9 |
| FMM background colour + opacity | Implemented (needs a compositor for `_NET_WM_WINDOW_OPACITY`) |
| FMM spotlight colour | Not wired: the key is saved and shown in the UI but never read (bug 3) |
| FMM excluded apps, game mode | Implemented (exact WM_CLASS) |
| Highlighter: left/right colours, always colour, radius, fade, spotlight mode, opacity | Implemented, but behaviour while the button is held is wrong (bug 4) and defaults differ (bug 5) |
| Highlighter ripple mode (new PT default) | Missing |
| Crosshairs: colour, opacity, thickness, border, orientation, auto-hide, fixed length | Implemented |
| Crosshairs centre radius (gap around pointer) | Not wired (bug 2) |
| Gliding cursor | Implemented (speed units differ: PT 25/5, Rust 1000/500) |
| Mouse Jump | Partial: coloured rectangles instead of screenshots, no monitor clamp (bug 1) |
| Cursor Wrap (mode, activation, drag, single monitor) | Implemented. Auto-activate default is true, PT false |
| Auto Hide Cursor, Mouse Button Lock | Missing |
| **Mouse Without Borders** | **Missing**: no crate, no code and no docs mention it (grep for "without borders", "barrier", "input-leap" finds nothing) |

### Bugs
1. **Medium.** The Mouse Jump preview is not limited to the screen. mouse_jump.rs:145-171.
   - PT `LayoutHelper.GetFormLayout` clamps the form to the activated screen. Rust scales to `max_width x max_height` only and allows a scale above 1.
   - With PT's default thumbnail size 1600x1200 on a 1600x1000 screen, the popup was (-24,-24) with size 1624x1024, i.e. off-screen.
   - The Rust default of 400x300 differs from PT's 1600x1200.
   - Evidence: tests/parity_mouse_jump.rs `mouse_jump_preview_fits_on_screen_with_upstream_default_size` FAILED (`popup (-24, -24, 1624, 1024)`).
   - Screen↔thumbnail mapping itself matches PT `MouseHelper.GetJumpLocation`:
     - single 1600x1000 screen: preview (100,50) → (400,200)
     - two monitors 800x1000+0+0 and 800x500+800+0: preview (300,50) → (1200,200)
     - Both PASS under Xvfb with real XTest clicks.
   - Clicks on the 12 px padding map outside the desktop (no clamp). Suspected, low.
   - Monitor labels are drawn with `image_text8` on a default-background GC and show as a black box (mouse-jump.png).
2. **Medium.** Crosshairs ignore `crosshair_center_radius`. The field is never read in crosshairs.rs (`grep radius` finds nothing), so the lines run straight through the pointer. PT leaves a 20 px gap. Evidence: xh-center.png (4× crop of mouse-crosshairs.png around the pointer at 800,500 shows no gap).
3. **Low.** `find_my_mouse_spotlight_color` is saved by the settings UI (ui/src/main.rs:721) but never used by find_my_mouse.rs. A grep finds no reader.
4. **Medium.** Highlighter circles fade on a fixed timer from the press and do not follow the pointer while the button is held. highlighter.rs:115, 236-250 (lifetime = fade_delay + fade_duration from `created_at`). PT (MouseHighlighter.cpp `UpdateDrawingPointPosition` / `StartDrawingPointFading` on button-up) keeps the circle while the button is held, moves it with the pointer, and starts fading on release. Evidence: mouse-highlighter.png, taken ~2 s after a held left-button press, shows nothing at the pointer.
5. **Low.** Defaults differ from the *Properties.cs files (config.rs:202, 228, 273, 321, 331, 336, 348):
   - FMM initial zoom: Rust 4, PT 9
   - Spotlight colour: Rust #FFFFFF, PT #80FFFFFF
   - Highlighter radius: Rust 20, PT 30
   - Fade: Rust 500/250, PT 400/400
   - Click colours: Rust #FFFF00 / #0000FF, PT #a6BFFF00 / #a600BFFF
   - Highlighter auto-activate: Rust true, PT false
   - Crosshair border: Rust 0, PT 1
   - Fixed length: Rust 400, PT 1
   - Crosshairs auto-activate: Rust true, PT false
   - Cursor Wrap auto-activate: Rust true, PT false
   - Mouse Jump size: Rust 400x300, PT 1600x1200
   - Gliding speeds: Rust 1000/500, PT 25/5
   - Evidence: 13 FAILED tests in src/parity_tests.rs.
6. **Low.** Default hotkey conflict: Mouse Jump (Super+Shift+D) uses the same shortcut as Light Switch, whose card on the settings dashboard shows ⌘+Shift+D (settings-home.png).
7. **Env.** All multi-monitor logic depends on the `xrandr` binary (common/src/monitor.rs:47) rather than RandR over X11. Without xrandr, Mouse Jump fails with "cannot detect monitors". `parse_geometry` cannot parse negative offsets.

### UI differences
- The settings page (settings-mouseutils2.png) is one long page with Find My Mouse fields as free-text boxes (colour hex, opacity 0.5, zoom 4). PT uses separate expanders per utility with colour pickers, sliders and activation dropdowns.
- Labels are in French by default.
- Find My Mouse could not be verified visually: the overlay is black on a black Xvfb root and there is no compositor for opacity.

### Tests added
- crates/mouse-utils/src/parity_tests.rs: 19 tests, **6 pass / 13 fail**.
- crates/mouse-utils/tests/parity_mouse_jump.rs: 3 Xvfb tests, **2 pass / 1 fail**. They return early unless `MPT_XVFB_TEST=1` and a fake xrandr layout are set. Run each test in its own process; repeated popups in one process did not appear reliably (cause not identified).
- crates/mouse-utils/tests/parity_visual.rs: screenshot harness gated on `MPT_XVFB_SHOT`.
- Existing crate tests: 20/20 pass.

---------------------------------------------------------------------------------------------------

## Totals
| Crate | Parity tests | Pass | Fail |
|---|---|---|---|
| key-manager | 48 | 37 | 11 |
| quick-accent | 31 | 6 | 25 |
| shortcut-guide | 8 | 6 | 2 |
| mouse-utils (unit) | 19 | 6 | 13 |
| mouse-utils (Xvfb Mouse Jump) | 3 | 2 | 1 |

Screenshots: quick-accent.png, shortcut-guide.png, shortcut-guide-light-right.png, mouse-crosshairs.png, xh-center.png, mouse-highlighter.png, mouse-findmymouse.png, mouse-jump.png, mouse-montage.png, settings-home.png, settings-keymanager.png, settings-keymanager-added.png, settings-mouseutils*.png.
