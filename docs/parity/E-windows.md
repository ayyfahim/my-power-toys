# E-windows report (FancyZones, Always on Top, Workspaces, Crop and Lock, Grab and Move, Alt Window Cycle)

Status: complete (all six modules). No existing non-test source was modified. The only change to an existing file is the `#[cfg(test)] mod parity_tests;` line appended to `crates/fancy-zones/src/lib.rs`.

Biggest cross-module finding: in Always on Top, Alt Window Cycle and Workspaces, the last X request of an action is a `send_event`, after which the x11rb connection is dropped without a round trip. Under Xvfb these requests mostly never reach the WM: an x11rb probe delivered 0/20, and 20/20 after adding a sync. See Workspaces bug 6.

Helpers: `.deps/withx-E-windows.sh` (Xvfb :124), `.deps/E-windows/{xt.py,fakeclients.py,lostmsg.py,x11probe/}`. Screenshots: `shots/E-windows/`.

| Module | Parity tests added | Pass / Fail |
|---|---|---|
| FancyZones | `crates/fancy-zones/src/parity_tests.rs` | 13 / 21 |
| Always on Top | `crates/always-on-top/tests/parity.rs` | 2 / 6 (X11 test under Xvfb) |
| Workspaces | `crates/workspaces/tests/parity.rs` | 3 / 3 under Xvfb (4 / 2 without DISPLAY) |
| Crop and Lock | `crates/crop-and-lock/tests/parity.rs` | 6 / 0 |
| Grab and Move | `crates/grab-and-move/tests/parity.rs` | 5 / 3 |
| Alt Window Cycle | `crates/alt-window-cycle/tests/parity.rs` | 4 pass + 1 flaky X11 test (2 of 3 runs pass) |

All existing unit tests of these crates pass.

Environment notes: Xvfb :124 via `.deps/withx-E-windows.sh`; egui apps rendered with `LIBGL_ALWAYS_SOFTWARE=1`. `xrandr` is not installed, so `mpt_common::monitor::detect_monitors` fails and the apps fall back to a single "Screen" monitor; multi-monitor behaviour was therefore only tested through pure functions. No window manager runs, so `_NET_ACTIVE_WINDOW`/`_NET_WM_STATE` paths cannot be exercised end-to-end.

## 1. FancyZones (crates/fancy-zones vs src/modules/fancyzones)

### Feature parity

| PowerToys feature | Port status |
|---|---|
| Templates: No layout, Focus, Columns, Rows, Grid, Priority Grid | Partial: all six exist, but each has a fixed zone count and different geometry for Focus and Priority Grid (bugs 3-5) |
| Template zone count (1..128, default 3) | Missing for templates. Custom Columns/Rows/Grid in the editor allow 1-16 |
| Show space around zones + spacing (default on, 16 px) | Partial: there is one global `zone_gap` (default 8) and no on/off switch. The outer margin is gap/2, PowerToys uses the full spacing (bug 2) |
| Sensitivity radius (default 20) | Missing: no setting, and hit-testing has no radius (bug 6) |
| Canvas custom layouts | Partial: zones are typed in as numbers in a table (x, y, w, h in %). There is no drag/resize canvas editor |
| Grid custom layouts with split/merge/resizers | Missing: only uniform cols×rows grids. Merged cells are impossible, so PowerToys Grid(5) and Grid(7) cannot be built (bug 4) |
| Layout persistence (custom layouts, per-monitor assignment) | Implemented (TOML; tested and also confirmed through the GUI) |
| Quick layout switch Win+Ctrl+Alt+1..9 + flash | Implemented (Super+Ctrl+Alt+N) |
| Shift drag / mouse switch / Ctrl multi-zone / middle-click spanning | Implemented (polling based) |
| Overlapping zones algorithm (smallest/largest/positional/closest center) | Partial: "Positional" is approximated by closest center (geometry.rs:132-134) |
| Multi-zone selection (span) | Partial: the port uses a ">=50 % of area inside the bounding box" rule, PowerToys requires full containment (bug 7) |
| Override Win+Arrow, index mode | Implemented; matches `MoveByDirectionAndIndex` |
| Win+Arrow position mode | Partial: picks a different zone than `ChooseNextZoneByPosition` on non-uniform layouts, and does not cycle (bugs 8, 9) |
| Win+Ctrl+Alt+Arrow extend | Missing |
| Move across monitors | Implemented (index and position) |
| Show on all monitors / span zones across monitors | Implemented |
| Restore size, app last zone, excluded apps, transparent drag | Implemented (opacity needs a compositor) |
| Zone colours, opacity, numbers, system theme | Colours, opacity and numbers implemented with PowerToys defaults. `systemTheme` is missing |
| Window switching (Win+PgUp/PgDn), monitor rotation, mouse wheel layout switch, popup/child snap, disable round corners, layout name label options, display-change and zone-set-change moves, open on active monitor, editor on cursor screen | Missing |
| Editor hotkey Win+Shift+` | Different: Super+Shift+Z, plus an extra Super+Z click overlay that PowerToys does not have |
| Virtual desktops (per-desktop layouts) | Missing (N/A-ish on X11) |

### Bugs
1. **Medium**: the editor and the snap overlay share one lock file. `main.rs:14-25` uses `/tmp/mpt-fancy-zones.lock` for both modes, so pressing Super+Z while the editor is open exits silently. Evidence: `shots/E-windows/fz-lock.log` contains "Another instance is already running (pid=17), exiting".
2. **Medium**: the spacing model and its default differ from PowerToys. The default is 8, PowerToys uses 16 (`config.rs:158`). The outer margin is gap/2 (`geometry.rs:90-98`), while PowerToys uses the full spacing at screen edges and spacing/2 between zones (`LayoutConfigurator.cpp` CalculateGridZones/Columns/Rows). Every spaced zone is therefore 8 px too far out at the screen edges. Tests: `default_spacing_is_16_like_powertoys` (8≠16), `outer_margin_equals_spacing_like_powertoys` (8≠16), `columns_1_to_5_spacing_16` (25/25 mismatched, e.g. 1920×1080 n=2 port (8,8,944,1064) vs PT (16,16,936,1048)), `rows_1_to_5_spacing_16` (25/25), `grid_1_to_8_zones_without_merges_spacing_0_and_16`, `spanned_snap_rect_with_spacing_matches_powertoys` (port (8,8,1264,1064) vs PT (16,16,1253,1048)).
3. **Medium**: the default layout is "3 Columns" (`config.rs:166`). PowerToys defaults to Priority Grid with 3 zones (`LayoutData.h`). The templates also do not default to 3 zones: Focus has 1, Grid has 6 and Priority Grid has 4. Tests: `default_layout_is_priority_grid_with_3_zones`, `templates_default_to_3_zones`.
4. **Medium**: Grid templates cannot use a zone count. PowerToys `Grid(n)` picks rows/cols with its own algorithm (e.g. 3 zones gives 1×3, 5 gives 2×3 with the last cells merged). The port only has `grid(cols, rows)` (`layout.rs:141`), and the default template is 3x2. Test: `grid_with_5_or_7_zones_supported` (6≠5).
5. **Medium**: the Priority Grid and Focus geometry are wrong. Priority Grid (`layout.rs:158-169`) is 50 % left / top-right / two bottom quarters, while the PowerToys tables use 25/50/25 columns (4 zones = left column, wide middle, right split). Focus (`layout.rs:109`) is a single centred 70×80 % zone, while PowerToys uses N zones of 40 % size at (100,100), each offset by 50 px. Tests: `priority_grid_matches_powertoys_table`, `focus_layout_matches_powertoys` (port (288,108,1344,864) vs PT (100,100,768,432)).
6. **Medium**: there is no sensitivity radius. `zone_at_point` (`geometry.rs:127`) returns a single zone. PowerToys `ZonesFromPoint` captures every zone within 20 px, so dragging near a shared border selects both zones. A point in the outer margin selects nothing in PowerToys; the port snaps to the edge zone. Tests: `hit_test_near_border_selects_both_zones_sensitivity_radius_20` (port [1] vs PT [0,1]), `hit_test_in_outer_margin_selects_nothing` (port Some(0)).
7. **Low**: the span selection rule differs. `zones_in_span` (`geometry.rs:142`) includes zones that are at least 50 % inside the bounding box. PowerToys `GetCombinedZoneRange` requires full containment. This is only visible on canvas layouts. Test: `span_on_canvas_only_includes_fully_contained_zones` (port [0,1,2] vs PT [0,1]).
8. **Low**: position-based Win+Arrow picks different neighbours. `adjacent_zone_by_position` (`geometry.rs:217`) scores `along + 2*|across|`, while PowerToys uses an ellipse-distance metric. Test: `position_move_on_priority_grid_and_canvas_matches_powertoys` found 13 differing (zone, direction) pairs, e.g. canvas "from 0 Right: port=3 PT=1". Regular grids match.
9. **Low (code-read, unconfirmed at runtime)**: position mode never cycles. On a single monitor PowerToys calls `MoveByDirectionAndPosition(..., cycle=true)` and wraps around at the screen edge (`WindowKeyboardSnap.cpp`). In `snap.rs:184-196` the port just logs "no zone in direction". Index mode does wrap.
10. **Low**: zone edges are off by 1 px at odd resolutions. The port rounds f32 fractions (`geometry.rs:73-86`), while PowerToys uses integer floor of cumulative extents. Tests: `columns_1_to_5_no_spacing_odd_resolutions` (6 mismatches, e.g. 1366 px 3 columns: port 455/456/455 vs PT 455/455/456), `rows_…_odd_resolutions` (4). PowerToys Grid uses 3333/3333/3334 percents, so `grid_1_to_8_zones_divisible_no_spacing` fails even at 1920 px (PT 639/640/641 vs port 640×3).
11. **Low**: the overlay draws zones at a different place from where windows are snapped. The Super+Z overlay uses `Zone::to_pixels_with_gap` (truncation plus a `.max(100)` clamp, `layout.rs:61-74`). Snapping uses `zone_rects`+`apply_gap` (rounding). Tests: `overlay_zone_pixels_equal_snap_rect` (35 rects differ by 1 px), `small_zone_gap_never_grows_zone` (a 50×50 zone is drawn as 100×100).
12. **Low**: `Layout::zone_at`/`Zone::contains` (`layout.rs:90`) is inclusive on both edges, so a point on a shared border hits the left/top zone. This differs from the half-open `zone_at_point` and from PowerToys. Test: `hit_test_layout_zone_at_boundary_is_half_open`.
13. **Low (code-read)**: the overlay positions itself with `xdotool` (`overlay.rs:271-310`), which is not a declared dependency (it is absent here). Without it, override-redirect and monitor placement are skipped silently.
14. **Low (code-read)**: in the editor, deleting a custom layout that sits before the one being edited leaves `draft.editing` pointing at the wrong index (`editor.rs:428-433`), so "Update layout" overwrites a different layout.

### UI differences (screenshots `shots/E-windows/fz-01-editor.png`, `fz-02-editor-scrolled.png`, `fz-03-after-create.png`, `fz-04-snap-overlay.png`)
- Present in both: monitor selector, "Templates" and "Custom" sections, and the empty-state text "No custom layouts yet".
- Missing compared with PowerToys MainWindow.xaml: per-template edit dialog (Number of zones, Space around zones toggle, Distance to adjacent zones, Highlight distance/sensitivity), "Default layout" horizontal/vertical selectors, "Create new layout" dialog with a Grid/Canvas choice, the Grid editor (split/merge/resize), the Canvas editor (drag zones), and Duplicate.
- The port replaces these with an inline form: Name, Quick key, Columns/Rows/Grid/Canvas radio, Zones count, preview, "Create and apply to monitor", "Copy current monitor layout".
- Template previews show no spacing. Labels are English.
- The Super+Z overlay works (zones are coloured with the PowerToys default colours). It has no PowerToys equivalent.
- The binary with no arguments prints a usage line and exits with code 1.

### Tests added
`crates/fancy-zones/src/parity_tests.rs` (registered in `lib.rs`): 34 tests, **13 pass, 21 fail**. The failures are the findings above. Existing crate tests: 34/34 pass.

## 2. Always on Top (crates/always-on-top vs src/modules/alwaysontop)

### Feature parity

| PowerToys feature (AlwaysOnTopProperties.cs) | Port status |
|---|---|
| Pin/unpin hotkey (default Win+Ctrl+T) | Implemented, but the default is Super+T (bug 1) |
| Increase/decrease opacity hotkeys (Win+Ctrl+= / Win+Ctrl+-), 20-100 % in steps of 10, pinned windows only | Implemented (`_NET_WM_WINDOW_OPACITY`, needs a compositor). The default keys add Shift (bug 1). Steps match `Settings.h` |
| Frame enabled (default on) | Implemented (override-redirect, shaped, click-through X window) |
| Frame thickness 4 | Implemented, default 3 (bug 2) |
| Frame color #0099cc / accent color (default on) | Implemented. The accent colour comes from GNOME `accent-color` via `gsettings`. The fallback colour is #0078D4 (bug 2) |
| Frame opacity 100 % | Implemented, default 80 % (bug 2) |
| Rounded corners | Implemented (8 px radius) |
| Sound | Partial: `canberra-gtk-play` plays the bell for both pin and unpin. PowerToys plays distinct On/Off sounds plus opacity-change sounds; the port plays nothing on opacity changes |
| Excluded apps | Partial: matched against the WM_CLASS instance only (bug 4) |
| Do not activate in Game Mode | Approximated by "window is fullscreen". PowerToys blocks the whole hotkey in game mode, the port only blocks pinning |
| Show in system menu | Missing (N/A without a WM menu API) |
| Unpin all when the module is disabled | Missing (bug 3) |
| Toggle a window that is already topmost by other means (PowerToys reads `IsTopmost`) | Different: the port tracks pin state in memory, so the first press on an already-above window sends a redundant ADD |
| Virtual-desktop handling, border follows the window | Border follows the window by 100 ms polling; no virtual-desktop logic |
| Wayland | N/A: the port logs instructions for each compositor |

X11 `_NET_WM_STATE_ABOVE` behaviour: the port sends an EWMH `_NET_WM_STATE` ClientMessage (ADD/REMOVE ABOVE) to the root window. Without a window manager nobody acts on it, so the stacking order does not change. PowerToys' `SetWindowPos(HWND_TOPMOST)` always works. This is an inherent limitation of the approach. My test acts as a minimal WM (SubstructureRedirect on root) and confirms the correct ADD message is sent: `pin messages: [(2097152, 1, 228)]`. The border window is override-redirect, so it shows even without a WM.

### Bugs
1. **Medium**: the default hotkeys differ. Pin is `Super+T` (`config.rs:83`), although the code comment there says the upstream is Win+Ctrl+T. The opacity keys are `Super+Ctrl+Shift+=/-` (`config.rs:87-92`); PowerToys uses Win+Ctrl+=/- without Shift. Tests: `default_hotkey_is_win_ctrl_t`, `default_opacity_hotkeys_are_win_ctrl_plus_minus`.
2. **Low**: the frame defaults differ: thickness 3 vs 4, colour #0078D4 vs #0099cc, opacity 0.8 vs 100 % (`config.rs:103-113`). Tests: `default_frame_thickness_is_4`, `default_frame_color_is_0099cc`, `default_frame_opacity_is_100_percent`. Runtime evidence: the border was created at (97,97,406,306) around a 400×300 window at (100,100), which is 3 px.
3. **Medium**: disabling or stopping the module does not unpin windows. `stop()` (`lib.rs:253-271`) clears its `pinned` set but never sends REMOVE ABOVE, so windows stay above forever. PowerToys `CleanUp()` calls `UnpinAll()`. Test: `x11_pin_border_exclusion_and_cleanup` → "stop: expected REMOVE ABOVE for pinned 2097152, got []".
4. **Medium**: excluded apps only match the WM_CLASS instance (`x11.rs:176-182`, `lib.rs:158`). Excluding "Firefox" does not exclude a window with WM_CLASS `Navigator/Firefox`. FancyZones in the same repo checks both strings. PowerToys matches the process path and the window title. Test: same X11 test → "excluded app 'Firefox' (WM_CLASS class) was pinned".
5. **Low (code-read)**: the fallback when no `_NET_ACTIVE_WINDOW` exists uses `get_input_focus` (`x11.rs:22-30`), which can return PointerRoot (1) as a "window". My probe produced no message and returned Ok, so this is unconfirmed.
7. **High (observed under Xvfb)**: the pin/unpin request is often lost. `set_always_on_top` (`x11.rs:117-160`) opens a connection, sends the `_NET_WM_STATE` ClientMessage, flushes and drops the connection. Over 4 runs of the X11 test, the ADD message reached the WM once (`pin messages: []` three times). The root cause is the same as Workspaces bug 6 and was confirmed with a standalone x11rb probe. The opacity hotkeys write properties without a round trip either (`set_window_opacity`), so they are probably affected too (unconfirmed).
6. **Low (code-read)**: `stop()` restores opacity only if a focused window can be found (`lib.rs:262-267`). With no focused window, the opacities stay changed.

### UI
There is no module GUI; the options live in the shared settings app, which the A-settings group covers. Every PowerToys option except "Show in system menu" has a config key.

### Tests added
`crates/always-on-top/tests/parity.rs`: 8 tests, **2 pass, 6 fail**. The X11 test needs DISPLAY; I ran it under Xvfb :124. Existing unit tests: 8/8 pass.

## 3. Workspaces (crates/workspaces vs src/modules/Workspaces)

### Feature parity

| PowerToys feature | Port status |
|---|---|
| Editor hotkey Win+Ctrl+` | Implemented, but the default is Super+Ctrl+W (bug 7) |
| Snapshot of open windows (WorkspacesSnapshotTool) | Implemented via `_NET_CLIENT_LIST`, WM_CLASS, `_NET_WM_PID`, `/proc/<pid>/exe`+`cmdline` and `_NET_WM_STATE_HIDDEN` |
| Snapshot format (workspaces.json: id, name, creation-time, last-launched-time, is-shortcut-needed, move-existing-windows, monitor-configuration, applications[...] with position/monitor/minimized/maximized/command-line-arguments/is-elevated/package/pwa ids) | Different (TOML, no workspace id, no maximized flag, no monitor configuration list, no package/PWA/elevation). PowerToys files cannot be imported. Elevation, packaged apps and PWAs are N/A on Linux |
| App matching on launch | Partial: case-insensitive WM_CLASS instance match, `first()` window (bug 3) |
| Window states: minimized / maximized | Minimized is captured and shown in the editor but never applied on launch (bug 4). Maximized is not captured, and restore always un-maximizes first |
| Move existing windows / Create desktop shortcut options | Implemented. The shortcut has bugs (bugs 1, 5) |
| CLI: `WorkspacesLauncher.exe <workspaceId>` | `mpt-workspaces --launch <name or index>` and `--editor` |
| Launcher UI (per-app progress, cancel/dismiss) | Missing. Launching is synchronous, up to 10 s per new app, and only a status line is shown |
| Editor: list with app count, last launched, sort (Last launched/Created/Name) | Implemented (sort default matches) |
| Editor: search box, delete confirmation, "Launch & Edit", per-app maximized/minimized toggles, monitor preview with app placement | Search missing. Delete has no confirmation (one click on ✕). Launch & Edit missing. Minimized toggle missing. The monitor preview exists but is empty when monitor detection fails |
| Per-app CLI arguments, position/size, enable/remove | Implemented. Argument editing is broken (bug 2) |

### Bugs
1. **High**: desktop shortcuts point at a list index. `create_desktop_shortcut` writes `Exec=mpt-workspaces --launch {idx}` (`config.rs:209-221`). Deleting or reordering earlier workspaces then makes the shortcut launch a different workspace. Deleting a workspace also leaves its shortcut behind (`window.rs:185-195`). PowerToys shortcuts use the immutable workspace id. Test: `desktop_shortcut_uses_stable_id_and_unique_files` → "shortcut launches by list index: Exec=mpt-workspaces --launch 3".
2. **High**: saving a workspace corrupts CLI arguments that contain spaces. The editor shows `args.join(" ")` and saves `split_whitespace()` (`window.rs:62`, `window.rs:1091`), so just opening Edit and pressing Save turns `["--title","My Doc"]` into `["--title","My","Doc"]`. Evidence: a GUI run with a seeded config (`shots/E-windows/ws-02-edit.png`), after which the saved TOML contains `"--title", "My", "Doc"`.
3. **Medium (code-confirmed)**: with "Move existing windows", every entry of the same app takes `existing.first()` (`launcher.rs:42-44`). Two saved terminals move the same terminal twice, and the second is never launched or placed. PowerToys pairs each saved app with a distinct window. The X11 test `x11_restore_move_existing_and_window_states` reports "both xterm entries moved the same window(s)", but that observation is confounded by bug 6.
4. **Medium**: the minimized state is never restored. `launch_single_app` (`launcher.rs:39-80`) ignores `AppEntry::minimized`; PowerToys restores minimized and maximized windows. Test: same X11 test → "minimized app was not minimized on restore".
5. **Medium**: shortcut file names can collide. The slug lowercases names and replaces non-alphanumerics with `-` (`config.rs:205-208`), so "My WS" and "my-ws" share one `.desktop` file, and deleting one removes the other's shortcut. Renaming a workspace leaves the old shortcut, because `remove_desktop_shortcut` gets the new name (`window.rs:1116-1122`). Test: `desktop_shortcut_uses_stable_id_and_unique_files`.
6. **High (observed under Xvfb)**: window move requests are often lost. `x11::move_resize_window` (`x11.rs:66-97`) opens a connection, sends `_NET_WM_STATE` and then `_NET_MOVERESIZE_WINDOW`, flushes and drops the connection. A test acting as the WM received only 2-5 of 20 `_NET_MOVERESIZE_WINDOW` messages, while 20/20 of the first message arrived. Test output: "only 2/20 _NET_MOVERESIZE_WINDOW requests reached the WM". The same per-call connect/send/drop pattern in Always on Top lost the pin request in 3 of 4 runs (`pin messages: []`). A standalone probe confirmed this (`.deps/E-windows/x11probe`, x11rb 0.13.2, same Xvfb). `send_event` + `flush()` + dropping the `RustConnection` delivered **0/20** messages. Adding one round trip (`get_input_focus().reply()`) before dropping delivered **20/20**. Python/Xlib with flush+close delivered 20/20 (`E-windows/lostmsg.py`), so the X server is not the cause. Any function whose last X request is a `send_event` on a connection that is dropped right away is affected: `workspaces/src/x11.rs:66-97`, `always-on-top/src/x11.rs:117-160` (pin/unpin) and `alt-window-cycle/src/x11.rs:~190-205` (activate). FancyZones `snap_window` happens to do a round trip (`wm_class`) after moving, so it is not affected. Not yet verified on a real Xorg with a WM.
7. **Low**: the default hotkey is Super+Ctrl+W instead of Win+Ctrl+` (`config.rs:7`). Test: `default_hotkey_is_win_ctrl_backtick`.
8. **Medium**: launching while the editor is open does nothing. `main.rs:14-25` uses one lock file for `--editor` and `--launch`, so a desktop shortcut or `--launch` exits with "Another instance is already running" while the editor is open. Evidence: `shots/E-windows/ws-launch-while-editor.log`.
9. **Low**: Cancel does not discard a new capture. `capture_workspace` saves the new workspace before editing (`window.rs:146-147`), and Cancel reloads from disk, so the capture stays. Evidence: the GUI run printed "before cancel: 2 / after cancel: 2" (`ws-03-captured.png`, `ws-04-after-cancel.png`). PowerToys discards a new workspace on Cancel.
10. **Low (suspected)**: saved positions are client-area coordinates (`x11.rs:246-271`, `translate_coordinates` of the client). They are restored with `_NET_MOVERESIZE_WINDOW` gravity 0, which most WMs treat as the frame position, so windows can drift by the decoration size on each capture/restore cycle. Needs a real WM to confirm.
11. **Low (suspected)**: the capture does not filter out the editor's own window. No filter exists in `list_windows`/`capture_workspace`; PowerToys skips its own windows. This could not be confirmed without a WM populating `_NET_CLIENT_LIST`.
12. **Low**: an app's monitor is chosen from its top-left corner only (`window.rs:109-118`), so a window starting at a negative x is "unknown".

### UI differences (`shots/E-windows/ws-01-list.png`, `ws-02-edit.png`, `ws-03-captured.png`, `ws-04-after-cancel.png`)
- Similar to the WorkspacesEditor XAML: list page with "Create Workspace", sort combo, cards (name, app count, last launched, Launch/Edit/delete). The edit page has a breadcrumb "Workspaces > Edit Workspace", name box, "Create desktop shortcut" / "Move existing windows" checkboxes, app cards (enable checkbox, Left/Top/Width/Height, CLI arguments, Remove), and Cancel / Save Workspace.
- Missing: search box, "Launch & Edit", per-app Minimized/Maximized toggles, "Launch as admin" (N/A), the monitor-layout preview with app boxes (empty here because monitor detection fails), and the delete confirmation dialog.
- Rendering: the header background stops at about 700 px instead of spanning the window, and the status banner pushes the toolbar down, so buttons move after each action.
- Labels are English.

### Tests added
`crates/workspaces/tests/parity.rs`: 6 tests. Under Xvfb **3 pass, 3 fail**; without DISPLAY 4 pass, 2 fail (the X11 test is skipped). Existing unit tests: 9/9 pass.

## 4. Crop and Lock (crates/crop-and-lock vs src/modules/CropAndLock)

### Feature parity

| PowerToys feature | Port status |
|---|---|
| Thumbnail mode (live DWM thumbnail, Win+Ctrl+Shift+T) | Implemented (Composite capture, works while occluded). Confirmed under Xvfb: `cl-03-thumbnail.png` |
| Reparent mode (Win+Ctrl+Shift+R, window stays interactive) | Implemented (X11 reparent into a container, falls back to thumbnail on failure). Confirmed: "Reparented window 0x200001 into container 0x600000 (150x130 at 50,20)", `cl-04-reparent.png` |
| Screenshot mode (Win+Ctrl+Shift+S, static image) | Implemented. It also copies the PNG to the clipboard, which PowerToys does not do; this needs `xclip`/`wl-copy` (absent here, so only a warning) |
| Selection overlay: drag rect, Esc cancels, crop clamped to the window, empty rect rejected | Implemented. The geometry matches `OverlayWindow.cpp` for 484 drag combinations (test below) |
| Ignore the hotkey when a cropped window is active | Implemented (`is_own_window`) |
| Multiple cropped windows at once | Implemented (one process per crop) |
| Cropped window placed at the crop's screen position | Missing (bug 1) |
| Settings: three hotkeys | Implemented, defaults match |

### Bugs
1. **Low**: the cropped window opens at the WM's default position. `cropped.rs:95-106` sets no position for the thumbnail or screenshot window. PowerToys places the cropped window where the region was on screen. Evidence: with no WM, both windows appeared at (0,0) (`cl-02-screenshot-window.png`, `cl-03-thumbnail.png`); reparent mode is placed correctly.
2. **Low**: selections narrower than 2 px are dropped. `selection.rs:19,180-189` (`MIN_SIZE_PX = 2`) silently resets a 1-px-wide drag and keeps the overlay open. PowerToys accepts any non-zero rect, and closes the overlay when a click lands outside the window.
3. **Low**: the selection overlay covers only the target window (`selection.rs:205-246`). PowerToys covers all displays with a shade around the window. A drag that leaves the window is still clamped correctly.
4. **Info**: screenshot mode needs `xclip`/`wl-copy` for its extra clipboard copy; without them it logs a warning (`cl-screenshot.log`).

### UI
Selection overlay `cl-01-selection.png`: dimmed capture, accent border, hint "Drag to select a region • Esc to cancel", and a size label while dragging. This matches the PowerToys overlay idea. There is no settings page beyond the three hotkeys, as in PowerToys.

### Tests added
`crates/crop-and-lock/tests/parity.rs`: 6 tests, **6 pass** (crop geometry oracle, hotkeys, 1-px selection at geometry level, clamping, letterbox, CLI). Existing unit tests: 15/15 pass.

## 5. Grab and Move (crates/grab-and-move vs src/modules/GrabAndMove)

The PowerToys module exists: `src/modules/GrabAndMove/GrabAndMove/main.cpp` (3.5k lines, a low-level mouse/keyboard hook). Modifier + left-drag moves the window under the cursor; modifier + right-drag resizes it from the nearest of 8 handles. Settings are in `GrabAndMoveProperties.cs`.

### Feature parity

| PowerToys option / behaviour | Port status |
|---|---|
| `modifierKey` 0=Alt / 1=Win (default Alt) | Implemented (Alt=Mod1, Super=Mod4 passive button grabs, plus Lock/NumLock variants) |
| `useAltResize` (default true): right-drag resizes from the closest handle, re-evaluated during the drag; min 150×50 | Implemented; matches `GetClosestHandle`/`HandleDragResize` (tests) |
| Drag threshold (`SM_CXDRAG` = 4) before a press becomes a drag; click replayed otherwise | Implemented (`drag_threshold` 4, ReplayPointer) |
| Restore maximized window keeping the proportional grab point | Implemented, matches |
| `doNotActivateOnGameMode` (default true) | Approximated: active window is fullscreen |
| `excluded_apps` / `excluded_apps_for_modifier` | Implemented; matching rules differ (bug 1) |
| `shouldAbsorbAlt` (default true: swallow the Alt press so the app menu bar does not open) | Missing (no setting). Passive grabs only take the button, so a bare Alt press still reaches the app |
| `showGeometry` (default false: size/position overlay while dragging) | Missing |
| Per-module cursor feedback | Implemented (X cursor-font glyphs) |

### Bugs
1. **Low**: excluded-app matching is stricter than PowerToys (`geometry.rs:215-226`). PowerToys `find_app_name_in_path` matches an entry that starts at the executable file name and may include folders ("code" excludes `CODE-OSS.EXE`, `bin\gimp` also works). The port only accepts exact instance/class/exe names or a title substring. Tests: `excluded_app_matches_executable_name_prefix_like_powertoys`, `excluded_app_matches_full_path_like_powertoys` (both fail).
2. **Info**: on negative coordinates the handle midpoint is off by 1 px compared with C++ truncating division (`geometry.rs:84-85`); 8 tie points differ. Test: `closest_handle_matches_on_negative_coordinates`. The effect is negligible.
3. **Missing options**: `shouldAbsorbAlt` and `showGeometry` (see table).

### UI
There is no GUI; settings live in the shared settings app.

### Tests added
`crates/grab-and-move/tests/parity.rs`: 8 tests, **5 pass, 3 fail**. Existing unit tests: 11/11 pass. Not exercised end-to-end under Xvfb, because a passive grab plus a WM-driven move needs a real WM.

## 6. Alt Window Cycle (crates/alt-window-cycle vs src/modules/AltWindowCycle)

The PowerToys module exists: `src/modules/AltWindowCycle/AltWindowCycle.cpp` and `AltWindowCycleLogic.h`. Alt+` and Shift+Alt+` cycle among windows of the focused app (same process image path, resolving packaged apps behind ApplicationFrameHost). While Alt is held, a **switcher overlay** shows a grid of live thumbnails (300 px tiles, up to 6 columns, paged); releasing Alt commits, Esc cancels. Settings: `next_window_shortcut`, `previous_window_shortcut`.

### Feature parity

| PowerToys feature | Port status |
|---|---|
| Next/previous shortcuts Alt+` / Alt+Shift+` | Implemented, defaults match |
| `WrapIndex`/`BeginCycle` ordering (z-order, first press → index 1, unknown current → index 0) | Implemented; matches the oracle for 0-5 windows |
| Same-app grouping | WM_CLASS class, falling back to the `/proc/<pid>/exe` path. PowerToys uses the process path. Packaged-app resolution is N/A |
| Skip invisible, cloaked (other desktop), tool windows, owned popups | Implemented via EWMH: window type, skip-taskbar, transient-for, `_NET_WM_DESKTOP` |
| Restore minimized target | Implemented through `_NET_ACTIVE_WINDOW` (the WM de-iconifies) |
| Switcher overlay with thumbnails, hold-Alt-to-browse, release to commit, Esc to cancel | **Missing**: each press switches immediately. A 1500 ms "continue" timeout (`continue_timeout_ms`) emulates holding Alt |
| Extra options `include_minimized`, `all_desktops`, `continue_timeout_ms` | Port-only |

### Bugs
1. **High (observed under Xvfb)**: the activation request is often lost. `x11.rs:~190-205` sends `_NET_ACTIVE_WINDOW` and drops the connection without a round trip (same cause as Workspaces bug 6). Test `x11_cycles_within_same_app_in_mru_order` chose the correct window (a2) but failed in 1 of 3 runs with "left: [] right: [2097154]". The reverse press was delivered in only 1 of 3 runs.
2. **Medium (UI gap)**: there is no switcher overlay (see table).

### Tests added
`crates/alt-window-cycle/tests/parity.rs`: 5 tests. 4 logic tests pass; the X11 test is flaky (passed 2 of 3 runs) because of bug 1. Existing unit tests: 13/13 pass.

