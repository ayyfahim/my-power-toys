# H-misc parity report (Awake, Peek, Light Switch, Power Display, unported modules)

Tester: H-misc. Display :126. Screenshots: `shots/H-misc/`. Samples: `.deps/samples-H/`. Fake tools: `.deps/fakebin-H/`. Only test files were added (plus `#[cfg(test)] mod parity_tests;` lines in awake, peek and light-switch lib.rs). Summary: Awake 8 failing parity tests, Peek 10, Light Switch 2, Power Display 8.

## 1. Awake (crates/awake vs src/modules/awake)

Oracle: `Awake/Program.cs`, `Awake/Core/Manager.cs`, `Awake/Core/TrayHelper.cs`, `Awake/Core/AwakeStateCalculator.cs`, `Settings.UI.Library/AwakeProperties.cs`, `SettingsXAML/Views/AwakePage.xaml`.

### Feature parity
| PowerToys feature | Rust status |
|---|---|
| Passive / Indefinite / Timed / Expirable modes | Implemented (`config.rs` AwakeMode, `lib.rs` Deadline) |
| Keep display on | Implemented (`keep_screen_on`; ScreenSaver.Inhibit + logind `idle:sleep`) |
| Allow display off while locked | Logic implemented (LockWatcher via logind LockedHint / ScreenSaver.GetActive; truth table equals AwakeStateCalculator). **No UI and no tray item** |
| Timer expiry → back to Passive (persisted) | Partial: inhibitor released, but mode is not written back to config |
| Expirable in the past at load → now+5 min | Missing (treated as already expired) |
| Tray: Off / Indefinite / Interval submenu (30 min, 1 h, 2 h + customTrayTimes) / Until expiration / Keep screen on / Allow display off while locked / Exit | Partial: radio Off/Indefinite/Timed/Expirable + "Keep screen on" only (crates/daemon/src/tray.rs:172-280). No interval presets, no custom tray times, no lock option, tray tooltip has no remaining time |
| Settings page: mode combo, interval h/m, expiration date + time pickers, keep display on, allow display off while locked | Partial: mode segmented buttons, h/m text inputs, free-text date field, keep screen toggle. Missing allow-display-off-while-locked |
| Settings applied live (PT FileSystemWatcher re-runs ProcessSettings) | Missing: UI only saves TOML (crates/ui/src/main.rs:1039-1067), never restarts the running module |
| CLI `--use-pt-config/-c`, `--display-on/-d`, `--time-limit/-t`, `--pid/-p`, `--expire-at/-e`, `--use-parent-pid/-u` | Missing (no Awake binary or CLI subcommand anywhere in the workspace) |
| Process-bound keep awake (`--pid`) | Missing |
| Linux inhibitor (org.freedesktop.ScreenSaver + logind) | Implemented; Linux replacement for SetThreadExecutionState |

### Bugs
1. **medium**: Timed mode with 0 h 0 min keeps the machine awake forever. `crates/awake/src/lib.rs:70-75` maps 0 s to `Deadline::Never`. PT `Manager.SetTimedKeepAwake` (Manager.cs:412-428) uses `TakeWhile(remaining > 0)`, so it completes on the first tick and returns to Passive. Evidence: `parity_tests::timed_zero_duration_expires_immediately_like_pt` FAILED (`left: Never`).
2. **medium**: An expiration time in the past at load turns Awake off. `lib.rs:200-209` sets `expired` and does not inhibit. PT `Program.ProcessSettings` (Program.cs:588-596) moves ExpirationDateTime to now+5 min, saves it, and keeps awake. Evidence: `parity_tests::expirable_in_past_at_load_is_bumped_by_5_minutes_like_pt` FAILED.
3. **medium**: After the timer or expiration ends, the persisted mode stays Timed/Expirable. Nothing in crates/awake calls `save_module_config`, and `check_expired` (lib.rs:155-166) only joins the thread. On the next daemon start or UI toggle, the timer re-arms for the full duration. PT `HandleTimerCompletion` → `SetPassiveKeepAwake()` writes Mode=PASSIVE (Manager.cs:435-447, 546-548). Evidence: `parity_tests::expiry_reverts_persisted_mode_to_passive` FAILED (`left: Expirable right: Off`).
4. **medium**: Wrong defaults. `config.rs:7-16,59-69`: mode=Indefinite, keep_screen_on=true, timed_minutes=30. PT AwakeProperties.cs:17-22: PASSIVE, KeepDisplayOn=false, IntervalMinutes=1. A fresh install therefore inhibits sleep and screen blanking as soon as the module is enabled. Evidence: `default_mode_is_passive`, `default_keep_display_on_is_false`, `default_interval_is_0h_1m` FAILED. The A-settings screenshot `shots/A-settings/awake.png` shows Indéfini with the screen toggle on.
5. **low**: An invalid `expire_at` string keeps the machine awake forever. `lib.rs:79-88` falls back to `Deadline::Never` with only a warn log. The Rust settings page uses a free-text field (views.rs:2700-2712), so a typo is easy. PT uses Date/Time pickers and the CLI validator rejects bad dates. Evidence: `expirable_invalid_date_does_not_keep_awake_forever` FAILED.
6. **low**: `expire_at` rejects ISO-8601 strings with a UTC offset. PT stores a DateTimeOffset, e.g. `2026-03-01T18:00:00+01:00`. `config.rs:77-82` accepts only 3 naive formats. Evidence: `expirable_accepts_pt_settings_iso8601_with_offset` FAILED.
7. **medium**: Settings changed in the settings UI do not reach the running module. For mode, keep-screen, hours/minutes and expire_at, `crates/ui/src/main.rs:1039-1067` calls `save("awake")` with no `daemon_restart_module`. The tray does restart the module. PT applies changes immediately through its settings file watcher. Confirmed by code reading: there are no other restart calls for awake in crates/ui.
8. **low**: The minutes input clamps to 59 (`main.rs:1060`). The PT NumberBox Maximum is 60 (AwakePage.xaml:81-89). Confirmed by code reading.
9. **low (suspected)**: The Expirable deadline compares naive local times (`lib.rs:27-28,36`). Across a DST change the deadline can be off by up to 1 h, or an ambiguous local time may be misread. PT compares DateTimeOffset. Not tested.
10. **low**: In this sandbox, `start()` returns Err when neither the ScreenSaver nor the logind inhibitor is available (inhibitor.rs:51-53). PT never fails to start. Group A saw this and reported it as an environment limit.

### UI differences
- PT has a separate "Behavior" group: Mode ComboBox, then an expander with Date and Time pickers (24 h), then a NumberBox pair for the interval, then "Keep screen on", then "Allow display off while locked". Rust uses segmented buttons, plain text inputs for hours/minutes, a single free-text date-time field, and no lock option. Labels on the page are French ("Désactivé/Indéfini/Temporisé/Jusqu'à"); see group A.
- Tray: no interval submenu, no custom times, no lock toggle, no tooltip with remaining time or expiry.

### Tests added
- `crates/awake/src/parity_tests.rs` (registered in lib.rs). Run: `XDG_CONFIG_HOME=<scratch> MPT_PARITY_SCRATCH=1 cargo test -p mpt-awake`. Result: 21 tests in the crate, **13 passed, 8 failed**. The parity file has 16 tests: 8 pass, 8 fail. Failures are bugs 1-6.

## 2. Peek (crates/peek vs src/modules/peek)

Oracle: `Peek.FilePreviewer/Previewers/*` (supported extension sets), `Peek.Common/Helpers/ReadableStringHelper.cs`, `Peek.UI/PeekXAML/Views/TitleBar.xaml`, `Settings.UI.Library/PeekProperties.cs`, `SettingsXAML/Views/PeekPage.xaml`.
Samples: `.deps/samples-H/`. Runner scripts: `.deps/peek-H.sh`, `peek-nav-H.sh`. eframe runs with `LIBGL_ALWAYS_SOFTWARE=1` and no crash on any file.

### Feature parity
| PowerToys feature | Rust status |
|---|---|
| Image previewer (all WIC formats + .qoi, zoom) | Partial. PNG/JPG/GIF/BMP/WebP render (`peek-sample_png.png`). TIFF and ICO are detected as Image but fail to decode ("The image format Tiff is not supported"); the image crate is built without those features. jpe/jfif/dib/qoi/heic/avif are not recognised. No zoom |
| SVG (WebView2) | Missing. Only "200 × 200 pixels" with a placeholder icon (`peek-sample_svg.png`) |
| Text/code (Monaco, syntax highlight) | Implemented with syntect (`peek-sample_rs.png`). Only the first 50 lines are shown (`max_preview_lines`); Monaco shows the whole file |
| Monaco options: wrap text, try format, font size, sticky scroll, minimap, max file size | Missing |
| Markdown (WebView2) | Implemented with an egui renderer: headings, lists, tables, code, quote (`peek-sample_md.png`) |
| HTML preview (WebView2) | Missing (shown as highlighted source) |
| PDF (WebView2) | Partial. Needs `pdftoppm` for a thumbnail or `pdfinfo` for metadata. Neither is installed here, so only an icon is shown (`peek-sample_pdf.png`) |
| Audio / video playback | Missing. Metadata only (ffprobe duration and an ffmpeg thumbnail). Without these tools only an icon is shown (`peek-sample_mp4.png`) |
| Archive listing | Partial. zip/jar/nupkg/tar/tar.gz/tgz are listed (`peek-sample_zip.png`); .rar, .7z and .gz are not |
| SQLite previewer (.db/.sqlite/.sqlite3) | Missing (shown as Unknown) |
| Drive / special-folder previewers | N/A on Linux (Windows shell) |
| Shell preview-handler previewer | N/A (Windows COM preview handlers) |
| Unsupported file: name, type, size, date modified; folder size + file/folder counts | Partial. Shows mime and "No preview available". No date modified, no folder size and no counts. A folder shows a flat, unsorted entry list (`peek-dir.png`) |
| Navigate between files (←/→) | Implemented for files in the same directory; works with absolute paths ("16/18" in `peek-nav-montage.png`). Explorer multi-selection is not used: `detect_selected_files` exists but main.rs calls only `detect_selected_file` (first file) |
| Open with default app (button + Enter) | Implemented with xdg-open. The button is hidden in text and image views (bug 3) |
| Pin (remember window size/position) | Missing |
| Always on top | Implemented (`always_on_top`); not in the settings UI; default is wrong |
| Close after losing focus | Implemented and confirmed: the window closed after focus moved to root (`peek-nav-H.sh` printed "closed after focus loss"); default is wrong |
| Confirm file delete (Del → trash) | Implemented (gio/trash-put). Confirmation bar confirmed (`peek-nav-delete-confirm.png`) |
| Space to activate | Partial: Space closes the window. There is no file-manager hook to open it |
| Activation shortcut Ctrl+Space | Implemented (default matches) |
| Always run not elevated, show taskbar icon, file preview tooltip | N/A / missing |
| Get selection from file manager | Partial: clipboard via xclip/wl-paste plus a simulated Ctrl+C with xdotool/ydotool. With these tools missing it exits cleanly with "No file to preview" |

### Bugs
1. **high**: A text file that is not valid UTF-8 (Latin-1, UTF-16) cannot be previewed and the program exits. `crates/peek/src/preview.rs:398` (`read_to_string`) returns an error, then main.rs:78-88 calls `exit(1)`. No window appears. During navigation the file is skipped silently. PT shows these files. Evidence: `parity_tests::non_utf8_text_file_still_previews` and `utf16_text_file_still_previews` FAILED ("stream did not contain valid UTF-8"); the latin1.txt run printed "EXITED rc=1" (log `scratch-H/logs/peek-latin1_txt.log`).
2. **medium**: Navigation is broken when Peek gets a relative path. `gui/window.rs:763-782` (`list_sibling_files`): for `foo.png`, `parent()` is `""`, `read_dir("")` fails, and the list falls back to the single file. The header shows "1/1" in a folder of 18 files (`peek-sample_png.png`, `peek-sample_rs.png`). With an absolute path it shows "16/18". A directory target is never part of its own sibling list, so it also shows "1/1" (`peek-dir.png`).
3. **medium**: The footer ("Open with default app" button and shortcut hints) is pushed off-screen for text, code, markdown and image previews. In the other views it is half clipped. Cause: `window.rs:306-310` sets the content min height to `available - FOOTER_HEIGHT`, but the text ScrollArea and image use `ui.available_height()` / `centered_and_justified`, which fill the whole panel. Evidence: `peek-sample_png.png`, `peek-sample_rs.png`, `peek-sample_md.png` (no footer) vs `peek-montage.png` (clipped footer).
4. **medium**: Wrong defaults. `lib.rs:20-27,59-70`: `always_on_top=true` and `close_after_losing_focus=true`. PT PeekProperties.cs: AlwaysOnTop=false, CloseAfterLosingFocus=false. Evidence: `default_always_on_top_is_false` and `default_close_after_losing_focus_is_false` FAILED. The focus-loss close was observed at runtime.
5. **medium**: Some settings can only be changed by editing TOML. always_on_top, close_after_losing_focus, confirm_file_delete, enable_space_to_activate, syntax_highlighting and render_markdown have no control on the settings page (crates/ui/src/views.rs:3739-3815 shows only preview lines and directory entries). See `shots/A-settings/peek.png`.
6. **medium**: `url_decode` panics on a `%` followed by a hex digit and a multi-byte character. `file_detect.rs:98` slices `&s[i+1..i+3]` without checking char boundaries. Example: `file:///tmp/%aé.txt` from the clipboard → "byte index 8 is not a char boundary". The same function also decodes `%+1` as byte 0x01, because `from_str_radix` accepts a sign. Evidence: the verbatim copy of the function in `.deps/scratch-H/ud/ud.rs` printed `"/tmp/a\u{1}b"` and panicked as above. The function is private, so a crate test could not reach it.
7. **low**: TIFF and ICO are classified as Image but cannot be decoded. `preview.rs:330` lists tiff/tif/ico, but Cargo.toml enables only the png/jpeg/gif/bmp/webp image features. Evidence: log "The image format Tiff is not supported" / "Ico is not supported"; `peek-sample_tiff.png` shows a placeholder.
8. **low**: Some PT extensions get the wrong previewer. Archive: .rar, .7z, .gz → Unknown. Audio: .ac3, .amr → Unknown. Video: .3g2 .3gp .3gp2 .3gpp .asf .m2t .m2ts .mp4v .mts .wm → Unknown. Image: .jpe .jfif .dib .qoi .heic .avif → Unknown. Evidence: `pt_archive_extensions_are_archives`, `pt_audio_extensions_are_audio`, `pt_video_extensions_are_video`, `common_image_extensions_are_images` FAILED with these lists.
9. **low**: The size format differs from PT ReadableStringHelper. `preview.rs:533-548` gives 1 decimal and "B" and can show "1024.0 KB" or "5120.0 GB". PT uses 3 significant digits, truncation, "bytes"/"byte", moves to the next unit at ≥1000, and goes up to TB/PB/EB. Examples: 1 048 575 → Rust "1024.0 KB" vs PT "0.99 MB"; 1 500 000 → "1.4 MB" vs "1.43 MB". Evidence: `format_size_matches_pt_readable_string`, `format_size_never_shows_1024_of_a_unit` FAILED.
10. **low**: The footer hint glyphs ← → ⏎ render as tofu boxes ("□ □ navigate □ open"), because the egui default fonts lack them (`peek-montage.png`, `peek-nav-montage.png`).
11. **low**: Markdown rendering problems. Bold is only drawn in a white colour, not a bold font (markdown.rs:245-247); in `peek-sample_md.png` "bold" looks like normal text. The blockquote bar uses `ui.max_rect()` (markdown.rs:320-330) and runs to the bottom of the window. The "Mime" shown for .rs/.md is `text/x-c` (from `file`), which is misleading.
12. **low**: The directory listing is in raw `read_dir` order (f0, sub, f2, f1 in `peek-dir.png`) and is not sorted. preview.rs:412-435.
13. **low (suspected)**: The single-instance lock is `$TMPDIR/mpt-peek.lock` (main.rs:99-101), shared between users on one machine. Another user's live PID blocks Peek. Not tested.

### UI differences
- PT title bar has the file icon and name, an "x of y" counter, "Open with <app>" showing the default app name, and a Pin button. Rust has a header with icon, name, size • kind • mime • dimensions, and a counter. The footer has a generic button and key hints.
- PT settings page has Activation (method combo: shortcut vs Space, plus a ShortcutControl), Behavior (6 toggles) and Preview (source-code options). The Rust page has only preview lines (25/50/100/200) and directory entries (10/20/50), which have no PT equivalent, plus a read-only shortcut.
- Unsupported-file view lacks the PT "Type / Size / Date modified" lines.

### Tests added
- `crates/peek/src/parity_tests.rs` (registered in lib.rs). `cargo test -p mpt-peek --lib`: parity file has 19 tests, **9 passed, 10 failed** (bugs 1, 4, 8, 9). The 20 existing crate tests pass.
- Screenshots in `shots/H-misc/`: `peek-*.png` (one per sample type), `peek-montage.png`, `peek-nav-*.png`, `peek-nav-montage.png`.

## 3. Light Switch (crates/light-switch vs src/modules/LightSwitch)

Oracle: `LightSwitchService/ThemeScheduler.cpp` (CalculateSunriseSunset, Almanac/Ed Williams algorithm), `LightSwitchUtils.h` (ShouldBeLight), `LightSwitchStateManager.cpp` (NormalizeMinutes, offsets, transition detection), `Settings.UI.Library/LightSwitchProperties.cs`, `SettingsXAML/Views/LightSwitchPage.xaml`. The PT sun algorithm and ShouldBeLight are ported verbatim into the test file as oracles.

### Feature parity
| PowerToys feature | Rust status |
|---|---|
| Schedule Off / Fixed hours ("Manual") / Sunset to sunrise / Follow night light | Implemented in the backend (`ScheduleMode`; night light via GNOME/KDE D-Bus). The settings UI offers only Off / Sunset-sunrise / Fixed; **Follow night light is missing from the UI** |
| Fixed hours light/dark times (TimePicker) | Implemented as free-text "HH:MM" fields; an invalid value silently falls back to 20:00/06:00 (lib.rs:126-130) |
| Lat/long + "Set location" dialog with auto-detect | Partial: two text inputs, no auto-detect, no range validation, and no location warning bar when lat/long is 0,0 |
| Sunrise/sunset offsets | Implemented; matches PT NormalizeMinutes, including wrap across midnight |
| Show computed sunrise/sunset times on the page | Missing |
| Apply to system / apps | Implemented (GNOME color-scheme + gtk-theme, KDE, XFCE, Cinnamon, MATE). Linux mapping of ChangeSystem/ChangeApps |
| Hotkey toggle | Implemented (toggles the current theme). Default differs (bug 2) |
| Manual toggle sticks until the next scheduled transition | Implemented (scheduler acts only when the scheduled value changes, lib.rs:63-90) |
| Power Display dark/light profiles | Backend implemented (`*_power_display_profile`); no UI |
| CLI (LightSwitch.Cli, named-pipe protocol) | Missing (Windows named pipes; would be N/A as designed) |
| Wallpaper per mode / GTK & KDE theme names | Linux extras (not in PT) |

### Exercised logic (all pass unless listed in Bugs)
- Sunrise/sunset compared with the PT algorithm for 10 cities × 6 dates (incl. 2024-02-29). The maximum difference is **4 min** (Reykjavik and Anchorage in January and September). Rust uses the NOAA formula, PT uses the Almanac one. This is not a functional bug. The tolerance is set to 5 min. Published almanac references (Paris June, NY December, Sydney December, Tokyo March) are all within ±3 min.
- Local-time conversion for UTC+2, UTC+11 and UTC-5, probed ±5 min around the PT boundaries: matches.
- Polar day/night (Tromsø 69.6°N, Longyearbyen 78.2°N; 21 June and 21 December): Rust matches PT, and **both stay in light mode all day, including polar night**. PT's calcTime returns -1, so sunrise equals sunset and ShouldBeLight is always true. Rust clamps cos(H), which also makes sunrise equal sunset. This is a shared design weakness, not a parity bug. No panics for lat ±90, NaN or an invalid date.
- Offsets (-45 min sunrise, +30 min sunset) and a +7 h offset that wraps past midnight: match.
- `is_dark_between` equals `!ShouldBeLight` for every minute of the day on an 11×11 grid of light/dark times, including equal times (always light): passes.
- Fixed 22:00→07:00 across New Year (Dec 31 → Jan 1), inverted schedule, equal times: pass.
- A 48 h simulation of the 30 s scheduler with fixed 20:00/08:00 gives exactly 4 flips at 20:00/08:00: pass.

### Bugs
1. **medium**: Default light time is 06:00. `crates/light-switch/src/config.rs:175-177`. PT DefaultLightTime=480 (08:00) in LightSwitchProperties.cs. Evidence: `parity_tests::default_light_time_is_0800` FAILED (`left: "06:00" right: "08:00"`).
2. **medium**: Default toggle hotkey is Super+Shift+D (`config.rs:5`). PT is Ctrl+Win+Shift+D. Evidence: `default_hotkey_is_ctrl_super_shift_d` FAILED. Group A also reported this (A2).
3. **medium**: Follow-night-light mode cannot be selected in the settings UI. `crates/ui/src/views.rs` (settings_light_switch) has only 3 segmented buttons (off / sunset-sunrise / fixed). If the TOML contains `follow-night-light`, no button is highlighted. PT has 4 combo items (LightSwitch_ModeFollowNightLight). Confirmed by code reading and `shots/A-settings/light-1.png`.
4. **low**: An invalid fixed time ("25:00", "8 PM") is accepted by the UI (main.rs:984-990 stores free text) and silently replaced by 20:00/06:00 when scheduling (lib.rs:126-130). There is no warning in the UI. PT uses a TimePicker. Confirmed by `invalid_fixed_time_is_not_silently_replaced` (parse returns None for 25:00) together with the code path.
5. **low**: Latitude/longitude are not range-checked (main.rs:960-970 accepts any f64, e.g. 123). PT NumberBox limits are -90..90 / -180..180. Confirmed by code reading.
6. **low (shared with PT)**: During polar night the theme stays light all day (see above). Test `polar_day_and_night_follow_pt_behaviour` passes, which documents that Rust matches PT here.

### UI differences
- PT layout: Shortcut group (toggle-theme ShortcutControl), Schedule group (mode expander with times, location card + "Set location" dialog, offsets card showing computed sunrise/sunset, warning bars), Behavior group (Apply dark mode to: System / Apps checkboxes; Power Display profiles). Rust shows a read-only shortcut, Schedule segmented buttons, text fields, and System/Apps toggles. It has no computed times, location dialog, night-light option or Power Display profile UI. Text is French ("Coucher au lever du soleil" is a poor translation of "Sunset to sunrise").

### Tests added
- `crates/light-switch/src/parity_tests.rs` (registered in lib.rs). `cargo test -p mpt-light-switch`: 32 tests in the crate, **30 passed, 2 failed** (bugs 1 and 2). The parity file has 15 tests: 13 pass, 2 fail.

## 4. Power Display (crates/power-display vs src/modules/powerdisplay)

Oracle: `PowerDisplay.Lib/Utils/MccsCapabilitiesParser.cs` and its tests `PowerDisplay.Lib.UnitTests/MccsCapabilitiesParserTests.cs` (sample capability strings reused verbatim), `PowerDisplay.Lib/Utils/VcpNames.cs`, `Settings.UI.Library/PowerDisplayProperties.cs`, `SettingsXAML/Views/PowerDisplayPage.xaml`.
ddcutil and xrandr are not installed. Graceful handling was tested first. Then fake `ddcutil`/`xrandr` scripts that print realistic output and log their calls (`.deps/fakebin-H/`) were used to drive the real binary (runner scripts `pd-H.sh`, `pd2-H.sh`, `pd3-H.sh`).

### Feature parity
| PowerToys feature | Rust status |
|---|---|
| Brightness 0x10 / contrast 0x12 / volume 0x62 (continuous, scaled to %) | Implemented via `ddcutil setvcp`. Fake-ddcutil run: a click on the brightness slider sent `setvcp 10 5` and a wheel notch on volume sent `setvcp 62 40` |
| Color temperature 0x14 / input source 0x60 / power state 0xD6 (discrete, names from VcpNames) | Implemented. All PT name tables for 0x14/0x60/0xD6 match exactly. Values come from capabilities, with fallback lists |
| Capabilities parsing / max compatibility mode | Implemented, with deviations (bugs 1-3) |
| Laptop backlight | Linux equivalent: /sys/class/backlight via brightnessctl → logind SetBrightness → sysfs write. Scaling is correct and monotonic (tests pass) |
| Software brightness (xrandr gamma, min 10%) | Linux extra; worked in the fake run (`xrandr --output DP-2 --brightness 0.25` for 17%) |
| Rotation | Implemented via xrandr (X11 only) |
| Profiles (save/apply/delete; apply from Light Switch) | Implemented (flyout combo + "Save current", `--apply-profile`, `profile:<name>` action). PT drag-reorder and edit dialog are missing |
| Restore settings on startup | Implemented (state file) |
| Linked levels + excluded-from-sync | Implemented |
| Per-monitor enable flags, hide monitor | Config only; not in the settings page |
| Custom VCP value mappings | Implemented in config; lookup deviates from PT (bug 4); not editable in the UI |
| Identify monitors button, profile switcher | Implemented in the flyout |
| Activation shortcut Win+Ctrl+Shift+P | Matches (Super+Ctrl+Shift+P) |
| Mouse-wheel increment 5 | Default matches, but one notch moves 2 steps (bug 5) |
| Mouse-wheel control mode over the tray icon (Off / primary / all) | Missing (no tray icon for this module) |
| Monitor refresh delay, show system tray icon | Missing |
| Diagnostics / VCP code details / copy | Missing (`--list` CLI prints a summary) |
| PowerDisplay.Cli (named-pipe CLI) | Partial: `--list`, `--apply-profile`, `--help` |
| Brightness up/down hotkeys with step | Linux extra (`hotkey_step`, default 5, clamped 1..100) |
| Behaviour without ddcutil | Graceful. `--list` prints "warning: ddcutil not found: install it to control external monitors over DDC/CI" with rc 0. The flyout shows "No controllable monitors found." plus the same warning (`pd-flyout.png`). `--apply-profile nope` gives "profile \"nope\" not found" with rc 1 |

### Exercised logic (passes)
- `ddcutil detect` parsing: realistic ddcutil 2.x full output (eDP "Invalid display", DP and HDMI monitors, "Phantom display"), 1.x brief output with a USB monitor, and error texts ("No /dev/i2c devices exist", "Display 1" with no body). The stable id for a monitor without a serial is `ddc:GSM:LG HDR 4K:hdmi-1`.
- `getvcp --terse` parsing: multi-code output, ERR lines, lowercase codes, max 255 and 31 (7/31 → 23%), current > max clamped to 100, garbage lines ignored.
- Structured `capabilities` output with multi-line Values blocks; PT Dell U3011/P2416D strings; no outer parentheses; extra whitespace; mixed-case hex; invalid "GG"; a window segment after vcp().
- Percent ↔ raw round trip for DDC maxima 100/255/1000/65535. Backlight scaling with max 15 / 937 / u32::MAX is monotonic. Fake sysfs prefers firmware over platform over raw and skips max 0 and broken entries.
- Defaults vs PowerDisplayProperties: RestoreSettingsOnStartup=false, ShowProfileSwitcher=true, ShowIdentifyMonitorsButton=true, MaxCompatibilityMode=false, LinkedLevelsActive=false, MouseWheelIncrement=5. All match.

### Bugs
1. **medium**: Concatenated-hex capability strings are not parsed. Some monitors send `vcp(101214)` without spaces. `crates/power-display/src/ddc.rs:196-225` collects "101214" as one token, `u8::from_str_radix` fails, and the capabilities map is empty. In normal mode that means `caps=None`, so every readable code is shown, including codes the monitor does not advertise. PT test Parse_ConcatenatedHexFormat_ParsesCorrectly expects 10, 12, 14. Evidence: `tests/parity.rs::mccs_pt_concatenated_hex` FAILED (`{}`).
2. **low**: The last VCP code is lost when the capabilities string is truncated (no closing `)`). ddc.rs:196-225: the loop ends without a final `flush`. `(prot(monitor)vcp(10 12` gives only {0x10}. PT returns 10 and 12. Evidence: `mccs_pt_malformed_unterminated_keeps_last_code` FAILED ("last code lost: {16: []}").
3. **low**: A single-character hex token is accepted as a code. `(vcp(10 A 12))` gives 0x0A, 0x10, 0x12. PT Parse_SingleCharacterHex_Skipped expects only 10 and 12. Evidence: `mccs_pt_single_char_hex_skipped` FAILED.
4. **low**: Custom VCP name lookup deviates from PT GetValueName. (a) An empty `custom_name` returns "" instead of falling back to the built-in name. (b) A monitor-specific mapping beats an earlier apply-to-all mapping, while PT takes the first match in list order. File: config.rs:278-294. Evidence: `custom_mapping_with_empty_name_falls_back_to_builtin` FAILED (`left: ""`) and `custom_mapping_first_match_wins_like_pt` FAILED (`left: "Dock"`). Part (b) may be an intentional choice; the existing unit test `custom_value_names` asserts it.
5. **medium**: One mouse-wheel notch over a flyout slider changes the value by 2 × `mouse_wheel_increment` (+10 instead of +5). gui.rs:499-510 applies one step per frame while `raw_scroll_delta.y != 0`, and one notch is seen in two frames, or arrives as two events, under X11. Evidence: with the fake ddcutil, one XTest wheel notch on Volume (30%) sent `setvcp 62 40`; 2 notches sent `setvcp 62 50` (`pd3-H.sh`, `scratch-H/logs/ddcutil-calls.log`). The exact cause (frame repeat vs. duplicated X11 events) is unconfirmed.
6. **low**: xrandr outputs that are "connected" but have no mode/CRTC (a monitor switched off in xrandr) are listed as software-brightness targets. `xrandr --brightness` then fails for them. xrandr.rs:83-106 accepts any `connected` header. Evidence: `xrandr_connected_without_mode_is_not_a_target` FAILED (HDMI-1 included).
7. **medium (suspected)**: DRM ↔ xrandr matching is off by one on amdgpu. xf86-video-amdgpu names outputs `DisplayPort-0`/`HDMI-A-0` (0-based) while DRM connectors are `card0-DP-1` (1-based). `normalize_connector` (xrandr.rs:171-185) maps DP-1 to xrandr DisplayPort-1, which is the next port. Rotation would then go to the wrong monitor, and software dimming could be skipped for the unmatched one. Test `amdgpu_zero_based_xrandr_names_map_to_one_based_drm` FAILED. This is unconfirmed on real hardware; the existing unit test asserts `DisplayPort-0` == `card0-DP-0`.
8. **low**: The ddcutil "Values:" list printed on the same line (used by ddcutil for features without interpretation, e.g. `Values: 00 01 02 (interpretation unavailable)`) is ignored. ddc.rs:246-249. It does not affect 0x14/0x60/0xD6 in the samples. Evidence: `capabilities_inline_values_are_parsed` FAILED (`left: []`).
9. **low**: The flyout close button "✕" renders as a tofu box in egui's default font (`pd-flyout.png`, top-right).

### UI differences
- Flyout (`pd-flyout-fake.png`): a header with identify/refresh/link/close buttons; one card per monitor with sliders (Brightness/Contrast/Volume) and combos (Color temperature, Input source); a profile bar. This matches the PT flyout structure. The power state and rotation rows are off by default (`show_power_state=false`, `show_rotation=false`).
- Settings page (`shots/A-settings/pd.png`) has brightness hotkeys, step, notification, restore, linked levels and backend toggles (Linux extras). It lacks the PT MonitorRefreshDelay, MouseWheelControlMode, ShowSystemTrayIcon, the custom VCP mapping editor, the Profiles list (add/edit/reorder), and per-monitor cards (enable contrast/volume/input/rotation/color temperature/power state, hide, diagnostics, VCP details).

### Tests added
- `crates/power-display/tests/parity.rs`. `cargo test -p mpt-power-display`: the 29 existing unit tests pass. Parity file: 34 tests, **26 passed, 8 failed** (bugs 1-4, 6, 7, 8).
- Screenshots: `pd-flyout.png` (no ddcutil), `pd-flyout-fake.png` (3 monitors via fake tools), `pd-flyout-after-input.png`.

## 5. PowerToys modules with no Rust equivalent

Mapping of every folder in `PowerToys/src/modules` (the others have crates: AdvancedPaste→paste-plain, AltWindowCycle, alwaysontop, awake, cmdNotFound→command-not-found, cmdpal→command-palette, colorPicker, CropAndLock, EnvironmentVariables, fancyzones, FileLocksmith, GrabAndMove, Hosts→hosts-editor, imageresizer, keyboardmanager→key-manager, launcher→app-launcher, LightSwitch, MeasureTool→screen-ruler, MouseUtils→mouse-utils, NewPlus, peek, poweraccent→quick-accent, powerdisplay, PowerOCR→text-extractor, powerrename→bulk-rename, ShortcutGuide, Workspaces, ZoomIt).

| PowerToys module | Rust equivalent | Linux applicability |
|---|---|---|
| MouseWithoutBorders | none | Possible in principle (network KVM; Linux tools such as Barrier/Input Leap/Deskflow do this). It needs X11/Wayland input capture and injection, so it is not N/A, just unported |
| registrypreview (RegistryPreview + RegistryPreviewExt) | none | **N/A**: Windows registry (.reg files) and an Explorer extension |
| previewpane: Markdown, Monaco (source code), PDF, SVG, Gcode, Bgcode, QOI preview handlers | none as Explorer handlers | **N/A as shell extensions** (Windows Explorer preview pane COM handlers). Peek covers part of the content: markdown and code yes, PDF/SVG/QOI/Gcode no (see Peek) |
| previewpane: Pdf, Svg, Gcode, Bgcode, Qoi, Stl thumbnail providers | none | **N/A** as Explorer thumbnail handlers. The Linux equivalent would be freedesktop thumbnailers (`.thumbnailer` files), which could be ported but are not |
| previewpane: FileExplorerDllExporter, powerpreview, common | none | **N/A** (Windows shell plumbing) |
| MouseUtils/MouseButtonLock | none (mouse-utils has find_my_mouse, highlighter, crosshairs, mouse_jump, cursor_wrap, auto-hide, but no button lock) | Possible on Linux (uinput/XTest); missing |
| interface | n/a | **N/A**: the C++ module interface for the Windows runner (replaced by `mpt_common::module::PowerModule`) |

