# D-screen parity report: Color Picker, Screen Ruler, Text Extractor, ZoomIt

Environment: Xvfb 1600x1000 (private display `:113` via `.deps/withx-D.sh`, a copy of `withx.sh` with a different display number), no window manager, no compositor. scrot, xdotool, xclip, wl-copy, tesseract and ffmpeg are all missing. To get past the screenshot step, a test-only shim `.deps/D-shim/scrot` (it runs ImageMagick `import -window root`) was put on PATH. Input was injected with `.deps/D-shim/xt.py` (XTest via ctypes). Screenshots are in `shots/D-screen/`.

All four overlays are egui/eframe apps. They rendered fine under Xvfb with the defaults; no `WGPU_BACKEND`/`LIBGL_ALWAYS_SOFTWARE` override was needed. The settings app (`mpt-settings`, iced) ran with `ICED_BACKEND=tiny-skia`.

Only test files were added. The four `lib.rs` files each got a 3-line `#[cfg(test)] mod parity_tests;` registration. No source code was changed.

---

## 1. Color Picker (`crates/color-picker` vs `src/modules/colorPicker`, `ManagedCommon/ColorFormatHelper.cs`)

### Feature parity

| PowerToys feature | Status |
|---|---|
| Overlay with magnifier and live color tooltip | Implemented (egui; magnifier is always shown, 8x, wheel 2..20x) |
| HEX format (`%Rex%Grx%Blx`) | **Partial**: Rust gives `#FF8000`; PowerToys gives `ff8000` (bug 1) |
| RGB, HSL, HSV, HSB, HSI, HWB, CMYK, NCol, CIEXYZ, CIELAB, Oklab, Oklch, VEC4, Decimal, HEX Int | Implemented; strings match exactly for every ported upstream row |
| Custom formats and format-string editor (`%Re %Gr %Bl %Hu …`, add/rename/delete) | **Missing**: no parser exists; custom entries are dropped (bug 4) |
| Reorder formats (Move up/down), delete | Missing in the settings UI (TOML order is respected) |
| Default visible formats HEX/RGB/HSL | **Wrong**: HSV and CMYK are also enabled (bug 2) |
| Color name (`ShowColorName`, default **off**) | Partial: default is on (bug 3). Rust uses nearest of 140 CSS names; PowerToys uses its own HSL buckets ("Light gray", "Dark gray"…) (bug 5) |
| Activation: Open editor / Pick color first | Implemented (`EditorOnly` / `PickAndEdit` / `PickAndClose`) |
| Mouse actions: primary/middle/secondary (PickColorThenEditor / PickColorAndClose / Close) | Implemented in config with matching defaults; **not exposed** in the settings UI |
| ChangeCursor | Config only, not in the settings UI |
| Color history (limit 20, re-pick moves to front) | Implemented, matches `UpdateColorHistory`; editor shows only the first 8 entries; no remove action in the UI |
| Editor: 4 color variations (2 lighter, 2 darker, HSV-based with hue shift) | Different: a 12-step HSL lightness ramp plus a lightness strip |
| Editor: adjust color flyout (H/S/V sliders, hex and RGB input) | **Missing** |
| Editor settings button | Gear icon is drawn but does nothing (`gui/editor.rs` header, label with click sense but no handler) |
| Copy to clipboard | Implemented (wl-copy/xclip). Fails here because neither tool is installed; this is logged only |
| Activation shortcut Win+Shift+C | Super+Shift+C (equivalent) |

### Bugs
1. **Medium**: `src/color.rs:148`. HEX output is `#FF8000` (uppercase, with `#`). PowerToys `GetDefaultFormat("HEX") = "%Rex%Grx%Blx"` copies `ff8000`, as shown by the upstream test rows `HEX → "000000"`, `"ff0000"`, `"ffffff"`. The clipboard text differs for the default format. Evidence: `parity_tests::string_repr_hex_is_lowercase_without_hash` fails with `left: "#000000" right: "000000"`.
2. **Low**: `src/lib.rs:67-82` (`default_formats`). HSV and CMYK are enabled by default. PowerToys `ColorPickerProperties` enables only HEX, RGB and HSL. Evidence: `default_visible_formats_are_hex_rgb_hsl` fails with `left: [Hex, Rgb, Hsl, Hsv, Cmyk]`.
3. **Low**: `src/lib.rs:132` and `default_true` for `show_color_name`. Color name is on by default; PowerToys has `ShowColorName = false`. Evidence: `default_show_color_name_is_false` fails. Screenshot `cp-overlay.png` shows `#000000  Black` in the tooltip with default config.
4. **Medium (missing feature)**: `src/lib.rs` `format_list` / `ColorFormat::from_id`. There is no custom format string support. An entry such as `id = "My format"` is silently dropped from the editor. Evidence: `custom_format_entry_is_shown_in_editor` fails with `left: 1 right: 2`.
5. **Low**: `src/names.rs:150`. Color names come from the nearest CSS color, which does not match PowerToys `ColorNameHelper`. Examples: `#FAFAF0` gives "Floral White" (PowerToys "White"), `#C8C8C8` gives "Silver" (PowerToys "Light gray"), `#505050` gives "Dim Gray" (PowerToys "Dark gray"). Evidence: `color_names_achromatic_buckets` fails.
6. **Low**: `src/color.rs:55-72` (`ColorFormat::ALL`). The order is `…NCol, CIELAB, CIEXYZ…`. PowerToys order is `…NCol, CIEXYZ, CIELAB…`, and this order drives both the editor and the settings list. Evidence: `default_format_order_matches_powertoys` fails.
7. **Low**: no screenshot tool means a silent exit. `src/main.rs` logs `Failed to capture screenshot … Install one of: scrot, gnome-screenshot, maim` and exits with code 1. When the daemon starts the picker, stderr goes to `/dev/null`, so the user gets no feedback. ImageMagick `import` is not tried even though it is a common X11 capture tool. Evidence: `shots/D-screen/cp-noshot.log`.
8. **Low (UI)**: `gui/editor.rs`. At the default window size, the format card list is cut off at the CMYK card while about 100 px of empty space remains below it. The copy glyph `⎘` renders as a redo-like arrow because of font fallback. Evidence: `cp-editor.png`.
9. **Suspected, low**: `gui/overlay.rs:76`. `screen_to_pixel` uses `.round()` rather than `floor`, so a pointer at x=10.6 points samples pixel 11. This only matters when pixels_per_point is not 1 (HiDPI). It was not reproduced at scale 1.
10. **Suspected, low**: `gui/overlay.rs:330`. The wheel zoom is applied once per frame whenever `raw_scroll_delta != 0`. This is the same pattern that is confirmed as a double step in Screen Ruler (SR bug 1).

### UI differences
- **Overlay**: Rust shows the magnifier all the time, offset bottom-right of the cursor, plus a tooltip with the format and name. PowerToys shows a small swatch/value pill and opens the magnifier only on scroll. Both work as pick tools.
- **Editor**: Rust has a "Pick color" button, history circles, a 12-shade bar, a vertical lightness strip and format cards with copy buttons. PowerToys has a history list, a large color with 4 variation buttons, an adjust flyout, and format cards. Rust has no adjust flyout, no history removal, and the settings button does nothing.
- **Settings page**: the activation selector has 3 options ("Open editor / Pick a color / Pick & edit"). PowerToys has 2 options plus a Mouse actions group, which Rust lacks. Rust has no ChangeCursor checkbox, no "Add new format" button, and no move up/down/delete. The format preview uses Moccasin, as in PowerToys. The whole settings UI is in French by default: `crates/ui/src/main.rs:138` hard-codes `language: Language::Fr` and it is not taken from LANG. This affects all modules. Screenshots: `set-cp-2.png`, `set-cp-3.png`, `set-cp-top.png`.

### Tests added
- `crates/color-picker/src/parity_tests.rs`: 30 tests, **24 pass / 6 fail**. The failures are bugs 1–6.
- They port every row of `ColorRepresentationHelperTest`, the `GetStringRepresentation_{Red,White,Green,Blue}` rows, and every row of `ColorConverterTest`: HSL 28, HSV 28, CMYK 28, HSI 19, HWB/NCol 19, CIELAB 26, Oklab 26, Oklch 26, CIEXYZ 26. All numeric conversions and the CIELAB/XYZ/Oklab/Oklch/NCol strings match exactly, including banker's rounding and `-0` handling.
- Whole crate: 64 tests, 58 pass, 6 fail.

---

## 2. Screen Ruler (`crates/screen-ruler` vs `src/modules/MeasureTool`)

### Feature parity

| PowerToys feature | Status |
|---|---|
| Bounds tool (drag, inclusive dims, Shift keeps measurements) | Implemented; 201x151 for a 850..1050 / 250..400 drag (`sr-bounds.png`) |
| Spacing, Horizontal and Vertical spacing tools | Implemented; edge detection is correct (600x300 for a 600x300 rect, `sr-spacing.png`) |
| Edge detection (`EdgeDetection.cpp`, clamp to [1, dim-2], summed vs per-channel tolerance) | Implemented; identical to a C++ transliteration over more than 10,000 random cases |
| Pixel tolerance, default 30, wheel ±15 | Default OK; **the wheel steps twice per notch** (bug 1) |
| Per-color-channel edge detection | Implemented (config and UI) |
| Continuous capture | N/A on Linux (documented; the screenshot stays frozen). Config only; not in the settings UI |
| Draw feet on cross | Implemented (3 pt feet versus PowerToys 2 px) |
| Units px/in/cm/mm | Implemented, but always at 96 DPI (bug 3) |
| Unit DIP (index 4) | **Missing** (bug 4) |
| Clipboard text (`PrintToStream`) | Partial: precision differs (bug 2) |
| Measure cross color (#FF4500) | Implemented |
| Default measure style (None) | Config only; **not in the settings UI** |
| Tool keys Ctrl+1..4, Esc | Plain 1..4 (labels say "(1)"). Under Xvfb with no WM the overlay did not get keyboard focus, so toolbar clicks were used instead |
| Activation shortcut Win+Ctrl+Shift+M | Super+Shift+R (different default) |

### Bugs
1. **Medium**: `src/gui/mod.rs:388`. The mouse wheel changes tolerance by 30 per notch instead of 15. The ±15 step is applied on every frame where `raw_scroll_delta != 0`, not once per wheel event. Evidence from the run: tolerance starts at 30, one notch down gives `Tolerance 0` (`tol15crop.png`), then 6 notches up give `Tolerance 180` instead of the expected 105 (`sr-tolerance105.png`).
2. **Low**: `src/measurement.rs:114` (`clipboard_text`, which uses `format_significant(…, 4)` at line 47). PowerToys writes clipboard values with `std::wostream << float`, which gives 6 significant digits; only the on-screen label uses `%.4g`. For 1 px in inches PowerToys copies `0.0104167 in` and Rust copies `0.01042 in`. For 100 px in cm it is `2.64583` versus `2.646`. Evidence: `clipboard_uses_stream_precision_six` fails.
3. **Medium**: `src/gui/mod.rs:75`. `px2mm` is always `None` and is never assigned, so physical units always assume 96 DPI. PowerToys uses the monitor's physical px→mm ratio (`GetPhysicalPx2MmRatio`). Inches, cm and mm are therefore wrong on any monitor that is not 96 DPI. Evidence: code inspection (`grep px2mm`); the conversion function itself passes `physical_monitor_ratio_remains_preferred`.
4. **Low**: `src/config.rs:46-54`. The DIP unit is missing. Upstream `UnitsOfMeasure` index 4 = DIP, computed as `pixels*96/monitorDpi`. A config with `units_of_measure = "dip"` fails to parse, and `load_module_config(...).unwrap_or_default()` then silently resets the whole Screen Ruler config. Evidence: `dip_unit_exists` fails with `unknown variant 'dip'`.
5. **Low**: `src/main.rs:53-54`. If the screenshot fails, the process logs and returns exit code 0, with no UI feedback. Evidence: `sr-noshot.log` shows `rc=0`.
6. **Low (UI)**: `crates/ui/src/views_extra.rs` (`settings_screen_ruler`). The settings page has no Continuous capture toggle and no Default measure style selector, although both exist in `ScreenRulerConfig`. Screenshot: `set-sr-2.png`.

### UI differences
- The toolbar has the same tools, but as text buttons ("Bounds (1)", "Spacing (2)", "Horizontal (3)", "Vertical (4)", "Close (Esc)") instead of icons. It also shows a "Tolerance N" label that PowerToys does not have.
- The bounds rectangle is drawn with a translucent fill and a centered label, which is close to PowerToys.
- The settings page has an activation field, feet, per-channel, tolerance, line color and units (px/in/cm/mm). It lacks DIP, continuous capture and default style. The page is in French by default.

### Tests added
- `crates/screen-ruler/src/parity_tests.rs`: 15 tests, **13 pass / 2 fail** (bugs 2 and 4).
- They include a reference transliteration of `FindEdge`/`DetectEdges`/`PixelsClose` compared on 60 random images × 20 cursors (including out-of-range cursors) × 2 modes × 7 tolerances, and all of them match. A 200,000-sample `PixelsClose` comparison also matches. The tests also port `MeasurementLogicTests` (pixels, 96-DPI fallback, physical ratio).
- Whole crate: 30 tests, 28 pass, 2 fail.

---

## 3. Text Extractor (`crates/text-extractor` vs `src/modules/PowerOCR`)

### Feature parity

| PowerToys feature | Status |
|---|---|
| Region selection overlay over a frozen screen | Implemented (drag; dims the outside; Esc or Cancel) |
| Language selection (installed OCR languages; PreferredLanguage default = OS language) | Partial: the overlay combo uses `tesseract --list-langs`; the default is `eng`; the **settings UI offers only eng/fra/deu/spa** as hard-coded buttons (`crates/ui/src/views.rs:2993-3012`) |
| OCR engine | tesseract CLI instead of Windows.Media.Ocr (expected platform difference) |
| Single line mode | Implemented (collapses whitespace) |
| Table mode | Implemented on tesseract TSV with a heuristic for column merging |
| CJK post-processing (`OcrTextFormatter.JoinCjkAwareWords`, no spaces between CJK glyphs or before `。`) | **Missing** (bug 1) |
| RTL word order reversal | N/A (tesseract already outputs logical order) |
| Copy result to clipboard | Implemented (wl-copy/xclip) |
| Missing tesseract | No panic: `run_tesseract` returns `Err("failed to run tesseract — is tesseract-ocr installed?")` and `list_languages()` returns an empty list; but see bug 3 |

### Bugs
1. **Medium**: `src/ocr.rs:72` (`postprocess`). There is no CJK-aware joining. tesseract `chi_sim`/`jpn` output such as `你 好` or `2026 。` is copied with spaces. PowerToys produces `你好` and `2026。`. Evidence: `cjk_glyphs_are_joined_without_spaces` (`"你 好"` vs `"你好"`), `cjk_punctuation_is_tight`, and `table_cjk_fragments_in_same_cell_have_no_space` all fail.
2. **Low (design difference)**: `src/ocr.rs:140`. Table mode merges words whose x-gap is less than the median text height. Two cells 1 px apart become one cell (`A1 B1` instead of `A1\tB1`, which is what upstream `Format_ColumnsSeparatedByOnePixelGap_RemainSeparate` expects). Upstream works on OCR lines and Rust works on tesseract words, so this is a heuristic trade-off rather than a clear defect. Evidence: `table_columns_one_pixel_apart_stay_separate` fails.
3. **Low**: `src/main.rs:51-84`. With tesseract missing, the overlay still opens with language "eng" and no warning. After the user drags a region, the overlay closes, the log says `OCR failed: failed to run tesseract … No such file or directory`, and the process exits with code 1. Nothing is shown to the user. Evidence: `te.log`, `te-toolbar.png`. The same silent exit happens when there is no screenshot tool (`te-noshot.log`, rc=1).
4. **Suspected, low**: `src/main.rs:60`. `shot.view(sel.x, sel.y, sel.width, sel.height)` can panic if the selection exceeds the image. `rect_to_pixels` clamps the selection, so this was not reproduced.

### UI differences
- The toolbar has Language combo, Single line, Table and Cancel (Esc). PowerToys has a similar top bar with language, single line, table and settings. Rust has no settings shortcut.
- The settings page shows only an "OCR language" choice among 4 hard-coded languages. It has no supported-languages info card, and the snipping-tool warning card is N/A. The page is in French. Screenshot: `set-te-1.png`.

### Tests added
- `crates/text-extractor/src/parity_tests.rs`: 17 tests, **13 pass / 4 fail** (bugs 1 and 2).
- They port `OcrTextFormatterTests` and `TableTextFormatterTests` rows, and add checks for missing-tesseract handling and language-list parsing.
- Whole crate: 23 tests, 19 pass, 4 fail.

---

## 4. ZoomIt (`crates/zoom-it` vs `src/modules/ZoomIt`)

### Feature parity

| PowerToys feature | Status |
|---|---|
| Hotkeys Ctrl+1 zoom, Ctrl+2 draw, Ctrl+3 break, Ctrl+4 live zoom, Ctrl+5 record, Ctrl+6 snip, Ctrl+Shift+6 snip save | Implemented, same defaults |
| Ctrl+7 DemoType, Ctrl+8 / Ctrl+Shift+8 Panorama, Ctrl+9 Mirror, Ctrl+Alt+6 Snip OCR | **Missing** |
| Zoom levels (`g_ZoomLevels` 1.25…4, initial index 3 = 2x), telescope wheel/arrow zoom, animate, smooth | Implemented and matches the reference; max zoom is 32x versus 256x (documented) |
| Live zoom | **Partial**: `livezoom` starts the same frozen-screenshot zoom (`gui/mod.rs:314`) |
| Draw: R/G/B/O/Y/P colors, Shift+color highlighter | Implemented (`zi-draw.png`: green, blue, highlighter yellow, red arrow) |
| Draw: W/K = white/black pen; **Ctrl+W/Ctrl+K = whiteboard/blackboard** | **Wrong** (bug 1) |
| Draw: X blur pen, Shift+X strong blur | **Missing** |
| Shapes: Shift line, Ctrl rectangle, Tab ellipse, Ctrl+Shift arrow | Implemented |
| Text typing T (wheel = font size); Shift+T right-justified | T implemented (`Hi PT` typed); Shift+T right-justify **missing** |
| Undo Ctrl+Z | Implemented (undid the text in `zi-draw-undo.png`) |
| Erase E | Implemented, but it keeps whiteboard/blackboard. Upstream E also clears the blank screen |
| Ctrl+wheel / Ctrl+↑↓ pen width 2..40 | Wheel implemented; arrow keys not handled in draw mode |
| Space: center cursor | Missing (minor) |
| Break timer (10 min, opacity, 9 positions, expired time, sound, background image/stretch, show desktop) | Implemented in config/logic; formatting and arrow keys differ (bugs 2 and 3) |
| Break: R/G/B… change timer color, Ctrl+W/K background | Missing |
| Snip to clipboard / save | Implemented (copy fails here because there is no clipboard tool; this is logged) |
| Record (MP4, scaling, frame rate, audio) | Implemented via ffmpeg/wf-recorder; no GIF format, no system audio, no webcam |
| Hotkey toggles the mode off again | Different: a second hotkey press while the overlay is open is ignored (`main.rs:54`, single-instance lock); Esc or right-click exits |

### Bugs
1. **Medium**: `src/gui/draw.rs:353,366-367`. Plain `W`/`K` toggle the whiteboard/blackboard, and `Ctrl+W`/`Ctrl+K` are swallowed by `_ if ctrl => {}`. In PowerToys ZoomIt (`Zoomit.cpp` around line 9809) plain W/K select a white/black **pen** and Ctrl+W/Ctrl+K blank the screen. Evidence: `zi-draw-k.png` shows K turning the screen black; `zi-draw-ctrlk.png` shows Ctrl+K doing nothing after toggling back.
2. **Low**: `src/geometry.rs:167` (`format_timer`). ≥60 minutes is shown as `1:39:00`, while PowerToys `"% 2d:%02d"` shows `99:00` (BreakTimeout allows 1-99). Expired time is shown as `-0:07` while PowerToys shows `(- 0:07)`. Evidence: `timer_text_uses_minutes_only` and `expired_timer_text_format` fail.
3. **Low**: `src/gui/timer.rs:80-88,40`. ←/→ change the break by ±60 s. PowerToys uses ±10 s, rounded to a multiple of 10, and only ↑/↓ and the wheel change whole minutes. Rust also refuses to go below `elapsed+1`, while PowerToys can go to 0. Evidence: one Right press turned 10:00 into `10:53` after about 7 s (`zi-break-right.png`); PowerToys would show about 10:00.
4. **Low (missing actions)**: `src/lib.rs` `ACTIONS`. DemoType, Panorama, Panorama-save, Mirror and Snip-OCR are missing. Evidence: `additional_powertoys_actions_exist` fails.
5. **Low**: `src/geometry.rs:6`. `MAX_ZOOM` is 32 versus `ZOOM_LEVEL_MAX` 256. This is an intentional and documented limit. Evidence: `max_zoom_is_256` fails (`32 vs 256`).
6. **Suspected, medium**: `src/record.rs:34-42,204`. Recording state is a PID file, and liveness is checked only with `/proc/<pid>` existence. If ffmpeg dies without the state file being removed, a later Ctrl+5 can send SIGINT to an unrelated process that reused the PID. Not reproduced, because ffmpeg is not installed. With ffmpeg missing, Ctrl+5 returns `no screen recorder found …` to the daemon log only.
7. **Low (UI)**: the ZoomIt settings page (`crates/ui/src/views_extra.rs:368`) exposes hotkeys, animate, smooth, pen color, break minutes, snip copy, the screenshot folder, fps, audio and the recordings folder. It lacks initial magnification, break opacity/position/sound/background/show-desktop/expired-time, record scaling/format, font, DemoType and the live zoom options, although most of these keys exist in `ZoomItConfig`. Screenshots: `set-zi-1.png`, `set-zi-2.png`.

### Environment note (not a bug)
- Against a bare Xvfb root background (set with `display -window root`), X11 `GetImage` returns black while ImageMagick `import` returns the background. With a real window on screen, the capture is correct.
- Evidence: `crates/zoom-it/tests/parity_capture.rs` with `MPT_PARITY_EXPECT=400,300,255,128,0` failed (`[0,0,0]`) on the root background and passed with a mapped window.
- The first `zi-zoom.png` and `zi-draw.png` black screenshots came from this artefact; the later ones were taken with a real window.

### Behaviour without a compositor or screenshot tool
- ZoomIt works without any tool (direct X11 GetImage).
- Zoom, Esc, draw, break and snip all ran under Xvfb with no WM. The `ViewportCommand::Focus` call is enough for keyboard input. Snip copy fails only for lack of xclip.
- Color Picker, Screen Ruler and Text Extractor need scrot/maim/gnome-screenshot. Without one they exit immediately, and only the log says why.

### Tests added
- `crates/zoom-it/src/parity_tests.rs`: 14 tests, **10 pass / 4 fail** (bugs 2, 4 and 5).
- They cover default hotkeys and registry defaults, zoom levels, the telescope steps compared with the `WM_MOUSEWHEEL` reference, pen range, the timer grid, and timer formatting.
- `crates/zoom-it/tests/parity_capture.rs`: 1 env-driven test. It is a no-op without DISPLAY and passes under Xvfb when a real window is mapped.
- Whole crate (lib): 32 tests, 28 pass, 4 fail.

---

## Totals
| Crate | New parity tests | Pass | Fail |
|---|---|---|---|
| mpt-color-picker | 30 | 24 | 6 |
| mpt-screen-ruler | 15 | 13 | 2 |
| mpt-text-extractor | 17 | 13 | 4 |
| mpt-zoom-it | 14 (+1 integration) | 10 (+1) | 4 |

All existing tests in the four crates still pass. Run them with `cargo test -p <pkg>`.
