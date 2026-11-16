# A-settings report: Settings UI (mpt-settings), daemon (mpt-daemon), CLI (mpt-ctl), common

Scope: crates/ui, crates/daemon, crates/cli, crates/common. Reference: PowerToys src/settings-ui (ShellPage, DashboardPage, GeneralPage, module pages, Settings.UI.Library) and src/runner.
Already known, referenced but not re-investigated: the UI language is hard-coded to French (crates/ui/src/main.rs:138, now line 141 because I added a 3-line test registration); Key Manager "Add remap" buttons add fixed mappings; Quick Accent has no settings page; Shortcut Guide has no settings page.

Line numbers below refer to the original source. My only changes to existing files are test registrations appended or inserted: crates/ui/src/main.rs (+3 lines after line 12) and crates/daemon/src/hotkeys.rs (+4 lines at the end).

## Environment notes
- mpt-settings, mpt-daemon and mpt-ctl build cleanly. mpt-settings runs under Xvfb :121 with ICED_BACKEND=tiny-skia, with no panic in the log.
- The shared withx.sh never gets a D-Bus session: `dbus-launch` is not installed, so `DBUS_SESSION_BUS_ADDRESS` stays empty and every zbus call fails with "I/O error: No such file or directory". My copy, withx-A-settings.sh, falls back to `dbus-daemon --config-file=/usr/share/dbus-1/session.conf --fork --print-address=1`. Other groups using withx.sh have no session bus.
- No window manager is running. The 1000x700 window was resized to 1000x1000 with XMoveResizeWindow (shots/A-settings/xresize.py).
- iced redraws only on input, so a screenshot taken right after a click shows the previous page. The scripts move the pointer after each click to force a redraw.
- Scripts are in shots/A-settings/: run-ui.sh (page screenshots), run-daemon.sh and run-daemon2.sh (daemon/CLI/UI-to-daemon), run-prefs.sh (persistence). Outputs: daemon-run.txt, daemon-run2.txt, daemon*.log.
- Screenshots are shots/A-settings/<page>.png; <page>-1/-2/-3.png are the same page scrolled further down. Reduced side-by-side montages are in shots/A-settings/m/<page>.png.
- Awake cannot start in this sandbox because there is no org.freedesktop.ScreenSaver and no system bus. This is an environment limit, not a bug.
- The language selector shows only "Français / English / Español". The 日本語 and 中文 buttons render empty because no CJK font is installed here, so this is environment-dependent (shots/A-settings/m/prefs.jpg).

---

## 1. Settings UI shell / navigation (vs ShellPage.xaml)
| PowerToys ShellPage | Rust (crates/ui/src/views.rs:467-560) |
|---|---|
| Dashboard entry at top | Implemented ("Tableau de bord") |
| General entry at top | Partial: "Préférences" sits at the bottom of the sidebar after 27 modules (views.rs:525) and needs scrolling to reach |
| Utilities grouped by category (System Tools, Windowing & Layouts, Input/Output, File Management, Advanced) | Missing: one flat "MODULES" list in a fixed, non-alphabetical order (modules.rs) |
| Search box (Shell_SearchBox) | Missing |
| Quick Accent entry (Shell_QuickAccent) | Missing: no sidebar entry and no dashboard card (bug A1) |
| Mouse Without Borders, Registry Preview, File Explorer add-ons | N/A on Linux |
| (Rust-only) | "Tests" (shortcut tester), "Design System" (developer page), "À propos" |

## 2. Dashboard (vs DashboardPage.xaml)
Screenshots: dash.png / dash-1.png (no daemon), dash-daemon.png, dash-daemon-toggled.png, dash-daemon-killed.png, dash-daemon-relaunched.png, montage m/dash-daemon.jpg.
| PowerToys DashboardPage | Status |
|---|---|
| Utility list with enable toggles | Implemented; a toggle calls StartModule/StopModule over D-Bus and is persisted (verified) |
| Sort by name / by status (Dashboard_SortBy) | Missing |
| Shortcuts overview (ShortcutsOverview) showing the current shortcuts | Partial: keycaps come from a hard-coded table (modules.rs) and do not reflect daemon.toml or module configs (bug A6) |
| Quick access actions (QuickAccessTitle) | Missing |
| "What's new" link (LearnWhatsNew) | Missing |
| (Rust-only) Total / active / inactive counters | Present: 27/0/27 without daemon, 27/25/2 with daemon |
| (Rust-only) daemon status + "Lancer le daemon" button | Works: "Daemon non connecté" turns into "Daemon connecté" within one 3 s poll after the daemon starts. "Lancer le daemon" started mpt-daemon and the status went back to connected (dash-daemon-relaunched.png) |

## 3. General page (vs GeneralPage.xaml) — Rust "Préférences" + "À propos"
Screenshots: prefs*.png, m/prefs.jpg, about*.png, m/about.jpg, m/prefs-restart.jpg.
| PowerToys GeneralPage section/option (default) | Status |
|---|---|
| Version & updates: Check for updates, auto-download (on), toast on new update (on), What's new after update (on), update channel | Partial: "Mise à jour" card on the À propos page with version, state, "Vérifier" and "Mettre à jour". No auto-download, toast or channel options |
| Run at startup (Startup) | Missing in the UI. `general.autostart` exists in daemon.toml but nothing reads it (bug C3) |
| Run as administrator / always run elevated / warnings for elevated apps | N/A on Linux |
| Appearance: app theme Dark/Light/Windows default (default "system") | Implemented: Clair/Sombre/Auto, default Auto. Not persisted (bug A3) |
| Language selector (+ restart info) | Implemented (FR/EN/ES/JP/CN). Not persisted, and the default is French (known) |
| Show system tray icon / theme-adaptive tray icon | Missing |
| Quick Access enable + shortcut | Missing |
| Backup & restore (folder, Backup, Restore, status) | Missing |
| Diagnostics & feedback (data diagnostics, view diagnostic data, bug report package) | Missing (no telemetry, so mostly N/A). No "report bug" package |
| Documentation / repository / report a bug / request a feature / privacy links | Partial: the About page shows the repository as plain text, not a link |
| (Rust-only) Visual theme (colours, gradients, background images), Text size, Accessibility (high contrast, bold, compact, reduced motion) | Present. Only the visual theme is persisted (ui.toml) |

## 4. Module pages sampled (structure vs PowerToys *Page.xaml)
Every Rust module page has the same skeleton: banner, "Statut" card with an "Activé" toggle, a read-only "Raccourci" card (keycaps, not editable), "Paramètres", and a "Tests" card with "Lancer le test". PowerToys pages have an enable card, then sections with editable shortcut controls and inline descriptions. Below, each line gives the page, its screenshot, what it shows, and the notable differences:
- **Always on Top** (aot*.png): shows border toggle, thickness 1/2/3/5 (default 3), sound, excluded apps. Missing PT options: border colour/accent, opacity, rounded corners, "do not activate in game mode", transparency shortcuts. The shortcut shown is Super+T; PT uses Win+Ctrl+T.
- **Awake** (awake*.png): shows modes Désactivé/Indéfini/Temporisé/Jusqu'à and "Garder l'écran allumé". The defaults shown are Indéfini with the screen kept on; PT defaults are PASSIVE (Off) with KeepDisplayOn=false (AwakeProperties.cs:19-21). No timed/expiry inputs are visible while Indéfini is selected.
- **Advanced Paste** (paste*.png): shows shortcut text fields (open Advanced Paste Super+Shift+V, Markdown, JSON), clipboard preview, close on focus loss, paste as file, AI actions. The header keycap says Super+Ctrl+V (the plain-text paste) while the PT main shortcut is Win+Shift+V. The daemon names the module "Paste as Plain Text" (bug A7).
- **Color Picker** (color*.png): shows activation behaviour, default format, show colour name, per-format toggles. On screen, HSV and CMYK are enabled by default; PT VisibleColorFormats enables HEX/RGB/HSL only. Activation shows "Choisir & éditer"; PT ActivationAction = OpenColorPicker with PrimaryClick = PickColorThenEditor. Owned by the color-picker tester; noted as a UI-visible difference only.
- **Light Switch** (light*.png): shows schedule Désactivé / sunset-to-sunrise / fixed hours, and system/apps toggles. The shortcut shown is Super+Shift+D; PT uses Win+Ctrl+Shift+D.
- **Mouse Utilities** (mouse*.png): Find My Mouse with activation method, colours, radius, opacity, zoom, animation, game mode, excluded apps; Highlighter / Crosshairs / Mouse Jump / Cursor Wrap / Gliding cursor shown as toggles. Mouse Highlighter, Jump and Crosshairs have no sub-settings visible on the page, while PT has full sections.
- **FancyZones** (zones*.png): only "Espacement des zones" (0/4/8/16 px). Missing most PT options: editor shortcut, shift-drag, multi-monitor, zone colours, override Win+Arrow, layout flashing, excluded apps. The shortcut shown is Super+Shift+Z; the PT editor shortcut is Win+Shift+`.
- **Alt Window Cycle** (cycle*.png): next/previous shortcut fields (Alt+` / Alt+Shift+`, which match PT), include minimised, all desktops, cycle timeout 1500 ms.
- Also captured, and following the same skeleton: Hosts Editor, Bulk Rename, Image Resizer, Key Manager, Screen Ruler, Text Extractor, App Launcher, Peek, Workspaces, Command Palette, Crop and Lock, File Locksmith, Environment Variables, New+, ZoomIt, Power Display, Grab and Move, Command Not Found (the matching *.png files and m/*.png montages).
- Untranslated strings in the French UI: the whole visual theme card is hard-coded English, including "Visual Theme", "Background style for the settings window", "Colors", "Gradients", "Backgrounds", "Custom Image" and "Choose image..." (views.rs:2393-2526). Module descriptions in the banners and dashboard cards are English while the rest is French (modules.rs).

## 5. Module count: dashboard says 27, README lists 28
The missing module is **Quick Accent** (`quick-accent`).
- crates/ui/src/modules.rs `default_module_list()` has 27 entries with no quick-accent. crates/ui/Cargo.toml has no `mpt-quick-accent` (or `mpt-shortcut-guide`) dependency.
- The daemon registers all 28 (crates/daemon/src/modules.rs:20-104). `mpt-ctl list` prints "28 module(s) total", and quick-accent is `running` (daemon-run.txt).
- The UI never adds daemon modules it does not know: `Message::DaemonState` (main.rs:502-509) only updates entries already in `self.modules`. The dashboard total is `self.modules.len()` (views.rs:593). So Quick Accent runs, but it cannot be seen or toggled from the UI, and it is excluded from the "Actifs" count. With the daemon up, the dashboard shows 25 active / 2 inactive while the daemon had 26 of 28 running (dash-daemon.png).
- Test: crates/ui/src/parity_tests.rs `dashboard_lists_every_module_registered_by_the_daemon` FAILS with `modules missing from settings UI: ["quick-accent"] (UI has 27)`.

---

## 6. Daemon (mpt-daemon) vs PowerToys runner
| Runner feature | Status |
|---|---|
| Single instance | Implemented: a second instance exits 1 with "another mpt-daemon instance is already running" |
| Module registry, start/stop, enabled state persisted | Implemented (daemon.toml). Persistence across daemon restart verified (awake/color-picker stayed stopped) |
| Default enabled set (EnabledModules.cs) | Different: every module not in daemon.toml is started (main.rs:71) (bug C1) |
| Global hotkeys (centralized hotkey registration) | Partial: X11 only, grabbed once at startup; Wayland gets none (logged) |
| Unregister hotkeys of disabled modules | Missing (bug C2) |
| Hotkey changes applied live | Missing: a daemon restart is required (documented in README) |
| Tray icon | Implemented (tray.rs). Not inspected in depth |
| Corrupt settings fall back to defaults | Missing (bug C4) |
| IPC to settings | D-Bus: Ping, ListModules, StartModule, StopModule, RestartModule, TriggerHotkey, GetVersion, CheckForUpdates |
| Elevation, GPO, auto-update scheduling, bug report | N/A / missing |

## 7. CLI (mpt-ctl)
All of these were checked against the running daemon (daemon-run.txt):
- `ping` → "Daemon response: pong" (exit 0). With no daemon: "Error: Daemon is not running." (exit 1).
- `list` → 28 modules. Without the daemon it prints a friendly error plus the raw D-Bus error (exit 1).
- `start/stop` work. `stop` on an already stopped module returns "Stopped module" (ok) and persists `enabled=false`.
- Invalid ids: `start bogus-module` → "unknown module: bogus-module" (exit 1). The same happens for `stop`, `trigger`, `trigger ""`, and `start Awake` (ids are case-sensitive). None of these touch daemon.toml.
- `trigger color-picker` while it is stopped → "module 'color-picker' is not running" (exit 1). `trigger key-manager:reload` → ok.
- `start` with no argument → "missing argument: module id" (exit 1). An unknown command prints usage (exit 1).
- Bugs: C5 (unknown named action reports success), C6 (unstable order / misaligned columns).

---

## 8. Bugs

### Settings UI
- **A1 (medium)**: Quick Accent is missing from the settings UI. File: crates/ui/src/modules.rs:6 (`default_module_list`, 27 entries) and main.rs:502-509 (daemon modules not in the list are dropped). The daemon runs Quick Accent but the UI shows no card or sidebar entry and no way to disable it, and the counters are wrong (27 total; 25 active when the daemon reports 26 running). PowerToys has a Quick Accent entry in ShellPage and a card on the dashboard. Evidence: dash.png, dash-daemon.png; test `dashboard_lists_every_module_registered_by_the_daemon` FAILED.
- **A2 (medium)**: Shortcuts shown on the dashboard, module pages and Tests page differ from the PowerToys defaults. File: crates/ui/src/modules.rs (hotkey fields). The module crates register the same values, so this reflects the Rust defaults. Differences: Always on Top Super+T (PT Win+Ctrl+T); Advanced Paste Super+Ctrl+V (PT Win+Shift+V); Light Switch Super+Shift+D (PT Win+Ctrl+Shift+D); Screen Ruler Super+Shift+R (PT Win+Ctrl+Shift+M); Shortcut Guide "Super+?" (PT Win+Shift+/); FancyZones Super+Shift+Z (PT Win+Shift+`); Workspaces Super+Ctrl+W (PT Win+Ctrl+`); Command Palette Alt+Space (PT Win+Alt+Space); App Launcher none (PT Run Alt+Space). Evidence: test `displayed_hotkeys_match_powertoys_defaults` FAILED with exactly this list. Note: `displayed_hotkeys_match_what_the_modules_actually_register` PASSED, so the UI agrees with the module defaults.
- **A3 (medium)**: Theme mode, language, text size and accessibility options are not saved. File: crates/ui/src/main.rs:491-498 (`SetThemeMode`/`SetLanguage`/`SetFontSize`/toggles only set fields); persistence.rs stores only `visual_theme` and `custom_history`. After choosing Sombre + English and restarting, the app is back to Auto/light + French, and no file is written to ~/.config/my-power-toys. PowerToys persists Theme and Language in settings.json. Evidence: run-prefs.sh, m/prefs-restart.jpg (left: dark + English; right: after restart).
- **A4 (low)**: Dashboard shows stale "Actifs" counts after the daemon dies. File: crates/ui/src/daemon.rs:14,32 return `modules: vec![]` when disconnected, and main.rs:502-509 then leaves each `running` flag unchanged. After the daemon was killed, the status read "Daemon non connecté" but the counters still said 24 active and the toggles stayed on. Evidence: dash-daemon-killed.png.
- **A5 (low)**: Toggling a module while the daemon is disconnected changes only the in-memory switch. File: crates/ui/src/main.rs:273-283. Nothing is written to daemon.toml, and the change is lost on restart. PowerToys always writes settings.json when the module is toggled. Confirmed by code reading only (not run).
- **A6 (low)**: The shortcut keycaps never reflect the configured shortcut. File: crates/ui/src/modules.rs plus views.rs module_card and hotkey_card. A hotkey set in daemon.toml (e.g. `[modules.screen-ruler] hotkey=...`) or in the module's own config is not shown. The PT dashboard reads the shortcut from the current module settings. Confirmed by code reading: `ModuleInfo.hotkey` is never updated after `default_module_list()`.
- **A7 (low)**: The module name differs between the UI and the daemon/CLI/tray: "Advanced Paste" vs "Paste as Plain Text". File: crates/paste-plain/src/lib.rs:144-145 vs crates/ui/src/modules.rs. Evidence: test `display_names_match_daemon_names` FAILED: `paste-plain: UI Some("Advanced Paste") vs daemon "Paste as Plain Text"`.
- **A8 (low)**: Hard-coded English strings in the visual theme card. File: views.rs:2393-2526. Evidence: m/prefs.jpg ("Visual Theme", "Colors", "Gradients", "Backgrounds", "Custom Image", "Choose image..." amid French text).

### Daemon
- **C1 (medium)**: All modules are enabled on first run. File: crates/daemon/src/main.rs:71 (`is_none_or(|entry| entry.enabled)`). With an empty config all 28 modules start (`mpt-ctl list`, daemon-run.txt). PowerToys EnabledModules.cs turns these off by default: Shortcut Guide, Keyboard Manager, PowerToys Run, Crop and Lock, Alt Window Cycle, Quick Accent, Text Extractor, Advanced Paste, Hosts, Environment Variables, New+, Workspaces, ZoomIt, Light Switch, Power Display, Grab and Move, Mouse Jump, Crosshairs. As a result, default global grabs such as Ctrl+1..6 (ZoomIt), Alt+` and Super+Ctrl+W are taken without the user opting in.
- **C2 (medium)**: A disabled module's global hotkey is still grabbed, so the key combination is swallowed. File: crates/daemon/src/modules.rs:168-196 (`hotkey_bindings` iterates every module and ignores `enabled`); main.rs:79-84 grabs once at startup. With color-picker disabled, pressing Super+Shift+C was intercepted and logged `Hotkey 'Super+Shift+C' for module 'color-picker' failed: module 'color-picker' is not running`, and the key never reached other apps. Starting or stopping a module at runtime never changes the grabs. In PowerToys, disabling a module unregisters its hotkeys. Evidence: daemon-run2.txt; test `disabled_module_hotkey_is_not_registered` FAILED: `disabled color-picker still grabs its hotkey: [("color-picker", "Super+Shift+C")]`.
- **C3 (low)**: `general.autostart` and `general.theme` in daemon.toml are written but never read. File: crates/common/src/config.rs:17-22. A grep for `autostart`/`.general` outside config.rs finds no reader; the UI also has no run-at-startup toggle. PowerToys GeneralPage "Run at startup" controls the startup task. Evidence: code search.
- **C4 (high)**: A corrupt daemon.toml prevents the daemon from starting. File: crates/daemon/src/main.rs:54 (`load_daemon_config()?`) and crates/common/src/config.rs:59-71. With `[modules.awake] enabled = "yes"`, mpt-daemon exits with status 1 and "invalid type: string \"yes\", expected a boolean", so there are no tray, hotkeys or modules. PowerToys SettingsUtils.GetSettingsOrDefault falls back to defaults. Evidence: daemon-run.txt "[daemon exit=1]"; tests `config_corrupt_daemon_toml_falls_back_to_defaults` and `config_garbage_daemon_toml_falls_back_to_defaults` FAILED.
- **C5 (low)**: An unknown named action on a running module reports success. File: crates/common/src/module.rs:41 (default `on_named_action` returns Ok) and daemon/src/modules.rs:206. `mpt-ctl trigger peek:nonsense` and `trigger always-on-top:bogus` print "Triggered hotkey for: …" with exit 0. The expected result is "unknown action". Evidence: daemon-run2.txt.
- **C6 (low)**: `ListModules` / `mpt-ctl list` order is arbitrary HashMap order and changes between daemon runs. File: daemon/src/modules.rs:161-166. The CLI ID column is 20 wide, so `environment-variables` (21 characters) misaligns its row (cli/src/main.rs:104,108). Evidence: daemon-run.txt; test `list_modules_is_in_a_stable_order` FAILED.
- **C7 (medium)**: The daemon's X11 grabber cannot bind many keys PowerToys allows: PageUp, PageDown, Home, End, Insert, Print, F13-F24. File: crates/daemon/src/hotkeys.rs:285-327 (`hotkey_key_to_keysyms`). A configured `Super+Shift+PageUp` logs "Skipping hotkey … unsupported key 'PageUp'" and the module ends up with no hotkey at all, not even its default. Evidence: daemon-run2.txt; test `navigation_and_punctuation_keys_are_supported` FAILED: `["PageUp", "PageDown", "Home", "End", "Insert", "Print", "F13", "F24"]`.
- **C8 (low)**: `"Hold "` (trailing space) is accepted as a hotkey with key "Hold". File: hotkeys.rs:153-160; the input is trimmed before the `"hold "` prefix check. The result is a key named "Hold", which is later skipped as unsupported. Evidence: test `hotkey_notation_parsing` FAILED at `parse_notation("Hold ").is_err()`.
- **C9 (low, environment-limited)**: When `StartModule` fails, the enable intent is not persisted. File: daemon/src/dbus.rs:33-44. `mpt-ctl start awake` failed in the sandbox (no inhibitor), and daemon.toml kept `enabled=false`. PowerToys stores the enabled flag regardless. Evidence: daemon-run.txt.

### Common (hotkey / config)
- **H1 (medium)**: Modifier order is significant. File: crates/common/src/hotkey.rs:35-46 (`Vec<Modifier>` kept in input order) and 55-75 (Display). `Shift+Ctrl+A` displays as "Shift+Ctrl+A" and is `!=` `Ctrl+Shift+A`. PowerToys HotkeySettings stores booleans and prints Win, Ctrl, Alt, Shift. This matters for comparing configured hotkeys and detecting conflicts. Tests `hotkey_format_uses_canonical_modifier_order` and `hotkey_equality_ignores_modifier_order` FAILED (`left: "Shift+Ctrl+A" right: "Ctrl+Shift+A"`).
- **H2 (low)**: Duplicate modifiers are accepted. `Ctrl+Ctrl+A` gives modifiers [Ctrl, Ctrl]. Test `hotkey_duplicate_modifier_is_rejected_or_collapsed` FAILED.
- **H3 (low)**: `Ctrl+` parses as Ok with an empty key (`Hotkey { modifiers: [Ctrl], key: "" }`); PowerToys treats a shortcut without a key as invalid. Test `hotkey_trailing_plus_without_key_is_rejected` FAILED.
- **H4 (low)**: The '+' key cannot be expressed. `Ctrl++` → Err("unknown modifier: "). PowerToys allows VK_OEM_PLUS, e.g. the Always on Top opacity shortcut Win+Ctrl+'+'. Test `hotkey_plus_key_is_representable` FAILED.
- **H5 (low)**: A modifier-only string `Ctrl` parses as the key "Ctrl" with no modifiers. The daemon uses this deliberately for double-tap/"Ctrl+Ctrl" notation, but it is not a valid PowerToys shortcut. Test `hotkey_modifier_only_is_not_a_valid_shortcut` FAILED.
- **H6 (low)**: Key case is not normalised: `super+t` displays "Super+t" (PowerToys shows "T"). The daemon normalises single letters itself, so this affects display only. Test `hotkey_single_letter_key_is_uppercased_in_display` FAILED.
- **H7 (medium)**: `set_module_enabled` silently wipes daemon.toml when it fails to parse. File: crates/common/src/config.rs:85 (`load_daemon_config().unwrap_or_default()` followed by save). One bad value anywhere means the next toggle from the UI or CLI rewrites the file with only that module, losing every other module's enabled flag and custom hotkey, with no backup and no warning. Unknown keys are also dropped on every save (`foo`, `extra` disappeared; daemon-run.txt). Test `set_module_enabled_on_corrupt_file_does_not_silently_discard_other_settings` FAILED; the file afterwards contained only `[general]` and `[modules.zoom-it]`.
- **H8 (low)**: A corrupt module config (`load_module_config`) returns Err rather than the default (PT GetSettingsOrDefault<T>). File: config.rs:100-112. Test `module_config_corrupt_falls_back_to_default` FAILED.

## 9. Tests added
| File | Result |
|---|---|
| crates/common/tests/parity.rs (21 tests: hotkey parse/format edge cases, config load/save with missing/empty/corrupt/garbage files, set_module_enabled) | 10 passed, 11 failed (H1-H8, C4) |
| crates/daemon/src/parity_tests.rs, registered in crates/daemon/src/hotkeys.rs via `#[cfg(test)] #[path = "parity_tests.rs"] mod parity_tests;` so it can reach the private parsers (9 tests) | 5 passed, 4 failed (C2, C6, C7, C8) |
| crates/ui/src/parity_tests.rs, registered in crates/ui/src/main.rs (6 tests) | 3 passed, 3 failed (A1, A2, A7) |
| Existing tests | mpt-common lib: 10/10 pass; mpt-ui: 2/2 pass; mpt-cli: 0 tests; mpt-daemon: no tests before mine |

Commands: `cargo test -p mpt-common --test parity`, `cargo test -p mpt-daemon`, `cargo test --no-fail-fast -p mpt-ui`. Use `--no-fail-fast` when running several packages, because the failing common parity tests otherwise stop cargo before the UI tests run.
