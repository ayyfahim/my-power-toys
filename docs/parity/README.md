# MyPowerToys vs Microsoft PowerToys: parity test summary

Date: 2026-11-16. Rust repo: ayyfahim/my-power-toys @ 3a4491e (main). Reference: microsoft/PowerToys (main, shallow clone).

## Method
- Built the whole workspace in a sandbox (Amazon Linux 2023, dev libs extracted locally), headless X via Xvfb, no window manager.
- Each module was compared with its PowerToys source (module code, settings XAML, defaults, unit tests). PowerToys unit-test cases were ported to Rust as the reference where possible.
- GUIs were launched under Xvfb with synthetic XTest input and screenshotted.
- No product code was changed. New tests live in `parity_tests.rs` / `tests/parity*.rs`; registration adds 2-4 lines per crate.

## Totals
- Pre-existing tests: 645 pass, 0 fail. `cargo fmt --check` and `cargo clippy -D warnings` were clean before testing.
- New parity tests: 1,220 in total. 971 pass; 249 document a difference from PowerToys and are marked
  `#[ignore = "known PowerToys parity gap, see docs/parity/<report>.md"]` so CI stays green.

## Running the parity tests
```bash
cargo test --workspace                 # normal run; known gaps are skipped
cargo test --workspace -- --ignored    # run only the known gaps (all 249 fail today)
cargo test -p mpt-bulk-rename -- --ignored parity_tests   # one crate
```
When a bug is fixed, its test starts passing under `--ignored`; remove the `#[ignore]` line so it guards
against regressions.

Some tests return early (and count as passed) unless their environment is present:
- X11 tests in alt-window-cycle, always-on-top, workspaces, zoom-it and mouse-utils need `DISPLAY`
  (an Xvfb is enough). Under a display, a few of them fail; they are described in E-windows.md and F-input.md.
  The X11 window-manager tests are timing-dependent because the bug they cover loses requests at random.
- `MPT_XVFB_TEST=1` enables the Mouse Jump Xvfb tests; `MPT_FAKE_XRANDR` points them at a fake `xrandr`.
- `MPT_PARITY_SCRATCH=1` with a scratch `XDG_CONFIG_HOME` enables the Awake tests that write config.

## Screenshots
`shots/<group>/` holds the screenshots referenced by each report. `shots/A-settings/m/` holds side-by-side
montages of the settings pages. Paths in the reports that start with `.deps/` refer to helper scripts in the
test sandbox and are not part of this repository.

## Is every feature integrated? No.
- PowerToys modules with no Rust port: Mouse Without Borders, Mouse Button Lock, Auto Hide Cursor. Registry Preview and the File Explorer preview/thumbnail handlers don't apply on Linux.
- Every ported module is only partly complete. See the per-group reports for feature tables.

## Highest-severity bugs
| # | Module | Bug | Report |
|---|---|---|---|
| 1 | Key Manager | "Add shortcut remap" always adds Ctrl+C→Ctrl+V and reloads immediately, so one click breaks copy. Mappings can't be edited in the UI. | F-input |
| 2 | Bulk Rename | Case-insensitive replace panics on Unicode names (`İstanbul_foo`). | B-files |
| 3 | Bulk Rename | A `/` in the replacement moves files to other folders. A name collision aborts the whole batch. Renaming a folder together with its children fails. A partial batch can't be undone. | B-files |
| 4 | Image Resizer | Default presets are wrong (widths 640/1280/1920/1080 instead of 854x480/1366x768/1920x1080/320x568). EXIF is always stripped, so photos can display rotated. | B-files |
| 5 | Hosts Editor | Comment lines like `# Note: ...` are parsed as disabled entries and rewritten on save. Enabling one writes a broken hosts line. | C-system |
| 6 | Daemon | A corrupt daemon.toml stops the daemon from starting. When it can't parse daemon.toml, `set_module_enabled` rewrites it, so the user's other settings are lost. | A-settings |
| 7 | Always on Top / Workspaces / Alt Window Cycle | Window-manager requests are sent, then the x11rb connection is dropped without a round trip, and most requests are lost (pin arrived in 1 of 4 runs; moves in 2-5 of 20). | E-windows |
| 8 | App Launcher / Command Palette calculator | Ignores unparsed input and returns wrong numbers (`0x1234+0x1234`=0, `2(1+1)`=2, `1-1` gives no result). There is no `^` operator and no functions, and large results saturate at i64::MAX. | G-launch |
| 9 | Quick Accent | Arrow-key activation deletes the wrong character. The module is missing from the settings UI (dashboard counts 27 of 28). 13 of 14 language tables differ from PowerToys. | F-input, A-settings |
| 10 | Peek | A non-UTF-8 text file makes Peek exit with no window. | H-misc |
| 11 | Settings UI | Language is hard-coded to French (`crates/ui/src/main.rs:138`). Theme, language and text size are not saved across restarts. | A-settings, D-screen |
| 12 | Daemon | Disabled modules still grab their hotkeys. On first run all 28 modules start; PowerToys leaves most off. | A-settings |

## UI parity
The UI does not match Microsoft's. The settings app is iced and the module windows are egui, so pixel parity isn't possible. The structural gaps are:
- **Sidebar:** one flat list with no categories or search.
- **General page:** missing run at startup, backup/restore, Quick Access and diagnostics.
- **Module pages:** many options exist in the config but have no control. Examples: Peek behaviour, Awake, Screen Ruler continuous capture, ZoomIt break options.
- **Shortcuts:** nine shown in the UI differ from PowerToys defaults.
- **Missing pages:** Quick Accent and Shortcut Guide have no settings page.
- **Rendering:** several icon glyphs render as empty boxes.

Side-by-side montages are in `shots/A-settings/m/`.

## Per-group reports
- A-settings.md: Settings UI, daemon, CLI, common
- B-files.md: Bulk Rename, Image Resizer
- C-system.md: Hosts Editor, Environment Variables, File Locksmith, New+
- D-screen.md: Color Picker, Screen Ruler, Text Extractor, ZoomIt
- E-windows.md: FancyZones, Always on Top, Workspaces, Crop and Lock, Grab and Move, Alt Window Cycle
- F-input.md: Key Manager, Quick Accent, Shortcut Guide, Mouse Utilities
- G-launch.md: App Launcher, Command Palette, Command Not Found, Advanced Paste
- H-misc.md: Awake, Peek, Light Switch, Power Display, unported modules

## Not verified in this sandbox
These paths need real hardware, tools or a window manager:
- real window-manager behaviour (EWMH);
- pkexec save paths;
- /dev/uinput injection;
- real ddcutil;
- tesseract OCR output;
- ffmpeg/PDF previews;
- Wayland.
