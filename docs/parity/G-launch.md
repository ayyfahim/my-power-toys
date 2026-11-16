# G-launch report: App Launcher, Command Palette, Command Not Found, Advanced Paste

Status: complete.

Summary of new parity tests (all pre-existing tests still pass: 76 + 34 + 18 + 25):

| Crate | New test file | Pass | Fail |
|---|---|---|---|
| mpt-app-launcher | crates/app-launcher/src/parity_tests.rs | 12 | 31 |
| mpt-command-palette | crates/command-palette/src/parity_tests.rs | 0 | 5 |
| mpt-command-not-found | crates/command-not-found/tests/parity.rs | 4 | 2 |
| mpt-paste-plain | crates/paste-plain/src/parity_tests.rs | 6 | 9 |

The only non-test source edits are one `#[cfg(test)] mod parity_tests;` line each in app-launcher, command-palette and paste-plain `lib.rs`. Helpers: `.deps/withx-G-launch.sh` (Xvfb :125), `.deps/G-shim/xt.py` (XTest typing with symbols, plus `focus` = XSetInputFocus, required because there is no WM), `.deps/G-shim/bin/xclip` (fake clipboard for the paste picker). Screenshots are in `shots/G-launch/`.



## Module 1: App Launcher (crates/app-launcher vs PowerToys Run, src/modules/launcher)

Existing tests: 76 passed, 0 failed. New: `crates/app-launcher/src/parity_tests.rs` (registered in `lib.rs` with `#[cfg(test)] mod parity_tests;`): **43 tests, 12 pass, 31 fail**.

### Plugin inventory (src/modules/launcher/Plugins/*/plugin.json) -> Rust

| PowerToys plugin | Keyword | Global | Rust | Status |
|---|---|---|---|---|
| Program | `.` | yes | `plugins/program.rs` (.desktop files) | Implemented (no fuzzy matching, see bugs) |
| Calculator | `=` | yes | `plugins/calculator.rs` + `calculator.rs` | Partial: only + - * / and parentheses |
| Shell | `>` | no | `plugins/shell.rs` | Implemented |
| Folder | (none) | yes | `plugins/folder.rs` | Implemented |
| Windows Uri Handler | `//` | yes | `plugins/uri.rs` | Implemented |
| Web Search | `??` | yes | `plugins/web_search.rs` | Implemented (engine choice added) |
| Window Walker | `<` | yes | `plugins/window_walker.rs` (X11) | Implemented |
| System Commands | (none) | yes | `plugins/system.rs` | Implemented (lock/logout/shutdown/restart/suspend/hibernate; no "empty recycle bin", IP/MAC info, UEFI) |
| Time and date | `)` | yes | `plugins/time_date.rs` | Partial |
| Unit Converter | `%%` | no | `plugins/unit_converter.rs` | Partial: length, mass, temperature, volume, data only |
| ValueGenerator | `#` | no | `plugins/value_generator.rs` | Partial: no GUID v1/v3/v5, no esc:/uesc: |
| History | `!!` | no | `plugins/history.rs` | Implemented |
| Windows Indexer | `?` | **yes** | `plugins/indexer.rs` (plocate/locate/fd) | Implemented. Global is off by default, a documented deviation (`config.rs:117`) |
| PowerToys | `@` | yes | none | Missing (could open mpt modules) |
| VS Code Workspaces | `{` | no (disabled) | none | Missing |
| Windows Settings | `$` | no | none | Missing (a Linux equivalent such as GNOME settings panels is possible) |
| Windows Terminal | `_` | yes (disabled) | none | N/A (terminal profiles) |
| Registry | `:` | no | none | N/A |
| Service | `!` | no | none | N/A (a systemd equivalent is possible) |
| OneNote | `o:` | yes (disabled) | none | N/A |

Settings (PowerLauncherPage.xaml / PowerLauncherProperties.cs) -> Rust `config.rs`:

| Setting (PT default) | Rust |
|---|---|
| Activation shortcut Alt+Space | No default hotkey (Alt+Space goes to Command Palette); configurable |
| MaximumNumberOfResults = 4 | `max_results = 8` (different default) |
| ClearInputOnLaunch, SearchQueryResultsWithDelay, SearchInputDelayFast 50, SearchInputDelay 150, SearchClickedItemWeight 5, Position (Cursor/Primary/Focus) | Implemented with the same defaults, all wired |
| SearchQueryTuningEnabled (off) | Missing: the clicked-item weight is always applied |
| SearchWaitForSlowResults, TabSelectsContextButtons, UsePinyin, GenerateThumbnailsFromFiles, Theme, TitleFontSize, ShowPluginKeywords, IgnoreHotkeysInFullscreen, UseCentralizedKeyboardHook, OpenFileLocation/CopyPath/OpenConsole hotkeys | Missing |
| Per plugin: Enabled, ActionKeyword, IncludeInGlobalResult | Implemented |
| Per plugin: WeightBoost, additional plugin options (e.g. calculator trig mode, TimeDate options) | Missing |

### Bugs

1. **High**: the calculator returns wrong numbers instead of rejecting input it cannot parse. `calculator.rs:23` stops parsing at the first token it does not understand and returns the partial value, without checking for leftover input. Evidence from `calc_hex_octal_binary_literals`, `calc_implicit_multiplication`, `calc_rejects_invalid_or_unbalanced_input` and `calc_scientific_notation`: `0x1234+0x1234` -> `0` (PT 9320), `2(1+1)` -> 2 (PT 4), `3log(100)` -> 3 (PT 6), `2E-2` -> 2 (PT 0.02), `1.2E-1` -> 1.2, `5 + 3 apples` -> 8, `1+2%` -> 3. Screenshot `shots/G-launch/launcher-hex.png` shows `=0x1234+0x1234` with result **0**.
2. **High**: there is no `^` operator, and `-2 ^ 2` silently evaluates to -2 (PT 4); `-(2 ^ 2)` gives -2 (PT -4). Test `calc_power_operator`. This is the same root cause as bug 1.
3. **Medium**: plain integer subtraction such as `1-1` or `10-3` is never evaluated. The guard at `calculator.rs:17-19` rejects any string made only of digits, `.` and `-`. `10 - 3` with spaces works. Test `calc_basic_precedence_and_decimals`: `"1-1": got None` (PT returns 0, `Interpret_MustReturnResult_WhenResultIsZero`).
4. **Medium**: unbalanced parentheses are accepted. `((1 * 2)`, `(1 * 2)))` and `2 + 3 )` evaluate (`calculator.rs:77-80` ignores a missing `)`; trailing `)` is dropped). PT reports them as invalid. Test `calc_rejects_invalid_or_unbalanced_input`.
5. **Medium**: the calculator has no functions or constants (sin, cos, ln, log, log2, log10, exp, sqrt, abs, round, ceil, factorial, sign, pi, e), no hex/octal/binary literals, no scientific notation and no implicit multiplication. Tests `calc_functions`, `calc_constants`, `calc_hex_octal_binary_literals`, `calc_scientific_notation` and `calc_implicit_multiplication` fail on every row. Trig modes (degrees/radians/gradians) are also missing.
6. **Medium**: the keyword query `=5/0` shows "No results". PT shows "Expression contains division by zero" (`calc_division_by_zero_keyword_shows_error`, screenshot `launcher-div0.png`). Invalid keyword expressions such as `=pi(9+)` and `=1+(2` also show nothing; PT shows "Expression wrong or incomplete" (`calc_keyword_invalid_expression_shows_error`). Global queries with no result match PT (`calc_division_by_zero_global_gives_no_result` passes).
7. **Medium**: the decimal comma is mishandled. `10+(8*9)/1,5` -> 82, because `,5` is dropped. PT gives 58 (de-DE) or 14.8 (en-US). Test `calc_decimal_separator_comma`.
8. **Medium**: in the Unit Converter, `kb` means kilobyte. `unit_converter.rs:206` gives `kB` the alias `kb` (and `mb`/`gb`/`tb` likewise), so `1 kB in kb` = 1, while PT (UnitsNet, `HandlesByteCapitals`) gives 8 kilobit. Test `units_kb_is_kilobit_kb_is_kilobyte`, screenshot `launcher-units.png` ("1 kB = 1 kB").
9. **Medium**: Unit Converter results smaller than 5e-11 display as `0`. `format_number` (`plugins/calculator.rs:44-50`) uses `{:.10}`, so `1 nm in km` shows "0 km" (PT 1E-12). Test `units_tiny_result_not_displayed_as_zero`.
10. **Low**: Unit Converter has no feet-inch shorthand: `1'6" in cm` and `1'6 in cm` -> None (PT 45.72). Test `units_shorthand_feet_inches`.
11. **Low (missing feature)**: Unit Converter has no area, speed, energy, pressure, time or parsec units (`units_other_unitsnet_categories`).
12. **Medium**: Value Generator has no GUID v1/v3/v5. `guidv3 ns:DNS abc` gives "Unknown command" (PT 5bd670ce-29c8-3369-a8a1-10ce44c7259e). Tests `vg_guid_v1` and `vg_guid_v3_v5_exact` (10 exact vectors).
13. **Medium**: Value Generator has no `esc:data`, `uesc:data`, `esc:hex` or `uesc:hex` (`vg_data_and_hex_escaping`).
14. **Low**: `url` encoding differs from PT's HttpUtility.UrlEncode. Rust gives `http%3A%2F%2Fgoogl.de%2Fu%20oii%20d`; PT gives `http%3a%2f%2fgoogl.de%2fu+oii+d` (space as `+`, lower-case hex). Test `vg_url_encode_matches_httputility`.
15. **Low**: hashes are printed in lower-case hex; PT prints upper case (`X2` in HashRequest.cs). Test `vg_hashes_exact_uppercase_hex`. The values themselves are correct (`vg_hash_values_case_insensitive` passes).
16. **Low**: Time/date does not accept `u+10456` or `ums+10456` (PT accepts both). `time_date.rs:90-94` allows only `-`. Test `td_unix_prefixes_with_sign`.
17. **Low**: Time/date treats a bare number with 5 or more digits (`)10456`) as a Unix timestamp. PT requires a `u` or `ums` prefix (StringParserTests "10456" -> false). Test `td_plain_number_is_not_a_timestamp`.
18. **Low (missing)**: Time/date has no `ft` (Windows file time), `oa` (OLE date) or `exc`/`exf` (Excel 1900/1904) input formats, and no "Week of the month" entry. Tests `td_windows_file_time_ole_and_excel_dates`, `td_week_of_month_present`.
19. **Low**: Time/date global results are not limited to Date, Time and Now (PT default "Only Date, Time, Now on global results"). Global counts are year/time/date/now = 1/4/3/1; PT gives 0/2/2/3. Test `td_global_results_only_date_time_now`.
20. **Medium**: the Program plugin has no fuzzy matching. `text_score` (`plugins/mod.rs:283`) only does exact, prefix, word-prefix, substring and initials. Ported FuzzyMatcherTest cases fail: `inst` misses "file open in browser-test" and ranks "Install Package" above "Inste" (PT: Inste, Install Package, file...). `sql s manag`, `sql manag`, `sql serv man`, `sql studio` and `chr` (vs "Change settings for text-to-speech...") do not match. `mssms` matches via initials, which PT rejects. "Windows Terminal" and "Windows_Terminal" score the same for `term` (65 vs 65). Tests `fuzzy_match_test_inst_order`, `fuzzy_regular_precision_table` (6 mismatches) and `fuzzy_space_preceded_match_scores_higher`.
21. **Low**: default differences from plugin.json and settings: Indexer `include_in_global_result=false` (PT true, deliberate, `config.rs:117`) and `max_results` 8 (PT 4). Test `route_defaults_match_plugin_json`.
22. **Low**: the Unit Converter result icon renders as a missing-glyph box (screenshot `launcher-units.png`).
23. **Low (suspected)**: `desktop.rs:142` `strip_exec_args` splits Exec on whitespace and drops every token that starts with `%`. That removes `%%` literals and collapses quoted arguments that contain several spaces. Not confirmed with a test.

### UI vs PowerToys Run
- An egui overlay renders fine under Xvfb with `LIBGL_ALWAYS_SOFTWARE=1`. It has the same layout as PT Run: a search box with a magnifier icon and placeholder "Start typing...", a result list with plugin icon, title and subtitle, and a footer with the plugin name and Enter action. Esc closes it. Screenshots: `launcher-empty.png`, `launcher-calc.png`, `launcher-hex.png`, `launcher-units.png`, `launcher-vg.png`, `launcher-vg-hints.png`, `launcher-time.png`, `launcher-div0.png`, `launcher-system.png`.
- Without a WM, keyboard input works only after `XSetInputFocus` (`G-shim/xt.py focus`).
- PT result rows show context-menu buttons (copy, run as admin, open folder). Rust shows only a footer hint for Ctrl/Shift+Enter.
- With 8 results the bottom border and footer of the overlay are clipped (`launcher-time.png`, `launcher-vg-hints.png`).
- A panic is logged when the X server dies (winit `window.rs:1249` unwrap). That happens only at teardown and is not a product bug.

## Module 2: Command Palette (crates/command-palette vs src/modules/cmdpal)

Existing tests: 34 passed. New: `crates/command-palette/src/parity_tests.rs` (registered in `lib.rs`): **5 tests, 0 pass, 5 fail**.

### Built-in extensions (src/modules/cmdpal/ext) -> Rust providers

| CmdPal extension | Rust provider | Status |
|---|---|---|
| Apps | `providers/apps.rs` | Implemented (.desktop) |
| Calc | `providers/calculator.rs` (reuses the launcher evaluator) | Partial: + - * / () only. No trig-mode, number-format, close-on-enter or replace-query-on-enter settings |
| Shell (Run) | `providers/shell.rs` (`>`) | Implemented |
| System | `providers/system.rs` | Implemented (lock/logout/shutdown/reboot/suspend/hibernate) |
| WebSearch | `providers/web_search.rs` (`??`) | Partial: Google, Bing, DuckDuckGo, custom. Brave, Startpage and Ecosia, which the launcher supports, silently fall back to Google |
| Indexer (Files) | `providers/files.rs` | Implemented (locate/fd) |
| WindowWalker | `providers/windows.rs` (`<`) | Implemented (X11) |
| TimeDate | `providers/time_date.rs` (`)`) | Implemented (fewer formats) |
| ClipboardHistory | `providers/clipboard.rs` (`clip`, off by default) | Implemented |
| Bookmark | `providers/bookmarks.rs` (`bm`) | Partial: config-file bookmarks only. No add-bookmark page, no `{placeholder}` arguments |
| WinGet | `providers/packages.rs` (`pkg`: apt/dnf/pacman/zypper/flatpak) | Implemented as a Linux analogue |
| WindowsSettings | `providers/settings.rs` (`$`, gnome-control-center panels) | Partial (10 entries) |
| PowerToys | none | Missing |
| PerformanceMonitor | none | Missing |
| WindowsTerminal, Registry, WindowsServices, RemoteDesktop, Actions | none | N/A on Linux (systemd services or SSH hosts would be the analogues) |
| Extension SDK / third-party extensions, pages, context menus, dock, pinned commands, home page with recent commands | none | Missing |

Settings (SettingsModel.cs) differences: CmdPal's default hotkey is **Win+Alt+Space**; Rust uses Alt+Space (`lib.rs:323`), because the Rust launcher has no default hotkey. Missing settings: KeepPreviousQuery, HighlightSearchOnActivate, BackspaceGoesBack, SingleClickActivates, IgnoreShortcutWhenFullscreen, ShowSystemTrayIcon, CompactMode, per-extension enable/alias pages. Rust adds `max_results=8`, `show_provider_tags`, and provider toggles, keywords and global flags.

### Bugs

1. **Medium**: large calculator results saturate to i64::MAX. `providers/calculator.rs:52` casts with `result as i64` whenever the value is an integer, with no range check. `99999999999*99999999999` shows `= 9223372036854775807`. Test `cmdpal_calc_large_result_not_saturated`; screenshot `shots/G-launch/palette-calc-big.png`. The launcher's `format_number` guards with `< 1e15`, so the launcher is not affected.
2. **Medium**: the calculator rounds to 6 decimals (`calculator.rs:55`). CmdPal uses 10 (`CalculateEngine.RoundingDigits`). `1/3` gives `0.333333`, PT `0.3333333333`. Test `cmdpal_calc_rounds_to_10_digits`.
3. **Medium**: the CmdPal Calc QueryTests rows fail: `sqrt(16)`, `2^3`, `max(1,2)`, `min(1,2)`, `pow(2,3)`, `max(1e3,2e3)`, `max(0b1010,0o12)` and `2^64` give no result (`cmdpal_calc_query_tests`, 8 of 11 mismatch). The shared evaluator bugs from Module 1 (#1 to #5) also apply here, including `0x...` evaluating to 0.
4. **Medium**: `1-1` and `10-3` give no calculator result (shared evaluator bug #3). Test `cmdpal_calc_negative_zero_and_subtraction`.
5. **Low**: `search_engine = "brave"` (also "startpage", "ecosia") is accepted by the shared config vocabulary but opens Google (`providers/web_search.rs:14-26`). Test `cmdpal_websearch_engines_same_as_launcher`.
6. **Low**: the calculator result icon renders as a missing-glyph box (`palette-calc-big.png`).

### UI vs Command Palette
- An egui overlay with placeholder "Search apps, commands, files..." renders fine under Xvfb (`palette-empty.png`). Each result has a provider tag on the right ("calc").
- CmdPal opens on a home page that lists top-level commands, pinned items and recents. Rust shows only the search box until you type.
- CmdPal has nested pages, a details pane, a context-menu (Ctrl+K) command bar and a footer listing the primary and secondary actions. Rust has none of these.
- Settings entries are reachable only with the `$` keyword; plain `sett` shows "No matching results" (`palette-settings.png`). This matches CmdPal WindowsSettings, which is not global by default.
- Screenshots: `palette-empty.png`, `palette-calc.png`, `palette-calc-big.png`, `palette-time.png`, `palette-settings.png`, `palette-web.png`, `palette-shell.png`.

## Module 3: Command Not Found (crates/command-not-found vs src/modules/cmdNotFound)

Existing tests: 18 passed. New: `crates/command-not-found/tests/parity.rs`: **6 tests, 4 pass, 2 fail**.

| PowerToys feature (CmdNotFoundPage.xaml) | Rust | Status |
|---|---|---|
| Enable toggle / GPO force on/off | daemon module enable; no GPO | Implemented / GPO N/A |
| Install / uninstall the PowerShell 7 profile module | `--install-hooks` / `--uninstall-hooks` and named actions; marked blocks in ~/.bashrc and ~/.zshrc, fish function file; idempotent | Implemented (Linux analogue). Verified: second install prints "No changes."; uninstall restores the original .bashrc byte for byte and removes the fish file |
| PowerShell 7.4+ and Microsoft.WinGet.Client detection, compatibility check, install logs | none | N/A (Rust checks for package tools at lookup time) |
| WinGet package suggestions | apt `command-not-found`, apt-file, dnf provides, pkgfile / pacman -F, zypper, nix-locate, flatpak | Implemented. Not verifiable end to end here: dnf fails in the read-only sandbox ("Read-only file system: /var/log/dnf.log"), so only parsers were tested |
| Similar-command suggestions | Levenshtein ≤ 2 over $PATH | Implemented: `gti` -> git, gio, tic, gc, gh; `pyhton3` -> python3 |

Hook syntax: `bash -n` passes on the generated .bashrc and on a hook block whose binary path contains a quote (`/opt/it's here/...`). The zsh block passes `sh -n` (zsh is not installed). fish is not installed, so its block was checked only by reading it. Running `pyhton3` in bash with the hook loaded prints the suggestion and `$?` is 127 (test `bash_hook_runs_and_returns_127`).

### Bugs
1. **Medium**: command names that fail `is_valid_command_name` (e.g. `foo@bar`, `café`, `a:b`) produce no output at all. `main.rs:36-38` returns 127 before printing "command not found". Because the hook replaces bash's own handler, the user gets a silent failure. Without the hook, bash prints `bash: foo@bar: command not found`, and PowerShell always shows its error. Test `unusual_name_still_reports_not_found` (stderr=""). Manual run: `bash -c ". ~/.bashrc; 'foo@bar'"` printed nothing, rc=127.
2. **Medium**: `parse_debian_cnf` (`lookup.rs:88-96`) does not handle Ubuntu's aligned output `sudo apt  install htop` (two spaces, printed when a snap alternative exists), so no apt suggestion is shown. It also never offers the snap line. Test `debian_cnf_output_with_snap_alignment` (left: []). The sample output is reconstructed from Ubuntu's command-not-found format, not captured on a real Ubuntu system.
3. **Low**: when asked about a command that does exist (`mpt-command-not-found ls`), it still prints "ls: command not found". Only reachable by calling the binary by hand.

UI: there is no GUI; the PowerToys page is install/detect buttons. The Rust settings page (owned by the settings tester) only exposes the toml toggles.

## Module 4: Advanced Paste (crates/paste-plain vs src/modules/AdvancedPaste)

Existing tests: 25 passed. New: `crates/paste-plain/src/parity_tests.rs` (registered in `lib.rs`): **15 tests, 6 pass, 9 fail**.

| PowerToys feature | Rust | Status |
|---|---|---|
| Paste as plain text (Win+Ctrl+Alt+V) | `on_hotkey` -> `paste_as_plain`, default **Super+Ctrl+V** | Implemented; different default hotkey |
| Advanced Paste window (Win+Shift+V) | `picker.rs` (egui) | Implemented |
| Paste as Markdown (HTML -> Markdown via ReverseMarkdown) | `html2md` crate | Partial: different Markdown dialect, no tag clean-up (bugs 3-5) |
| Paste as JSON (JSON / XML / INI / CSV / lines) | `json.rs` (port of JsonConverter.cs) | Mostly ported; CSV, INI and the line fallback match. Differences in bugs 1-2 |
| Paste as .txt / .png / .html file (+ .jpg with quality 90) | `write_temp` + text/uri-list | Implemented |
| Image to text (OCR) | tesseract | Implemented (tesseract not available here, not run) |
| Transcode to .mp3 / .mp4 | none | **Missing** |
| Clipboard history in the picker (Windows clipboard history) | none in Advanced Paste (Command Palette has a `clip` provider) | **Missing** |
| AI: free-form prompt, custom actions (name/prompt/shortcut/IsShown), "fix spelling" | OpenAI-compatible endpoint (`ai.rs`), same system prompt | Implemented (no Azure/Foundry/local-model provider choice, no moderation) |
| Settings: ShowCustomPreview, CloseAfterLosingFocus, EnableClipboardPreview, PasteAsJpgQuality, ShowAIPaste | all present and read in picker.rs / actions.rs | Implemented |
| AutoCopySelectionForCustomActionHotkey | none | Missing |
| Markdown/JSON direct shortcuts | default Super+Ctrl+Alt+M / J | PT default is **unbound** (bug 7) |

### Bugs
1. **Low**: valid JSON is reformatted. JsonConverter.Convert returns valid JSON text unchanged (`if (IsJson(text)) return text;`); Rust `json.rs:75` pretty-prints it. `{"a":1,"b":[1,2]}` becomes 7 indented lines. Test `json_valid_json_returned_unchanged`.
2. **Low**: XML -> JSON drops the XML declaration and xmlns attributes. Newtonsoft `SerializeXmlNode` emits `"?xml": {"@version","@encoding"}` and `"@xmlns"` / `"@xmlns:p"`. `try_xml` (`json.rs:156-164`) serialises only the root element, and roxmltree does not report namespace declarations as attributes. Tests `json_xml_declaration_kept`, `json_xml_namespace_declaration_as_attribute`.
3. **Medium**: Markdown conversion leaks `<script>` content and raw `<sup>` / `<o:p>`. MarkdownConverter.CleanNodes removes these elements; `html_to_markdown` (`actions.rs:250`) does not. `<p>Text<sup>1</sup><script>alert(1)</script><o:p>office</o:p></p>` becomes `Text<sup>1</sup>alert(1)office` (PT: `Text`). Test `markdown_drops_script_sup_and_office_tags`.
4. **Low**: the Markdown dialect differs from ReverseMarkdown defaults: `<h1>` becomes a setext heading `Title\n==========` (PT `# Title`), and lists use `* ` (PT `- `). Test `markdown_reversemarkdown_defaults`. The picker run confirms it: the Markdown action wrote `Report\n==========\n\nHello **world**, a,b\n\n* one`, captured from the fake xclip in `shots/G-launch/xclip-writes.log`.
5. **Low**: Markdown does not collapse whitespace inside text nodes. `<p>a   b\r\nc</p>` gives `a b\nc` (PT `a b c`). Test `markdown_collapses_whitespace_in_text_nodes`.
6. **Medium**: `html_to_text` (`actions.rs:268-312`), used for plain text and JSON when the clipboard has only HTML, fails 7 of 16 rows from HtmlToTextHelperTests. It does not collapse whitespace (`one\t  two \n three`). Nested list items run together (`twonested`). Table cells are not tab-separated (`OneTwo\nAB`). Numeric entities are not decoded (`&#x1F680;` stays literal), and the `&nbsp;` result is trimmed away. `<title>`, `<template>` and `hidden` elements are not dropped (`hiddenvisiblehidden`). `<pre>` whitespace is trimmed. Test `html_to_text_table`.
7. **Low**: default shortcut differences. The Markdown (Super+Ctrl+Alt+M) and JSON (Super+Ctrl+Alt+J) shortcuts are bound by default (`config.rs` `default_markdown_shortcut` / `default_json_shortcut`); PT leaves them unbound. Plain text is Super+Ctrl+V vs PT Win+Ctrl+Alt+V. The module is named "Paste as Plain Text" (`lib.rs:146`), not "Advanced Paste". Test `default_shortcuts_match_powertoys`.
8. **Low**: when the clipboard holds only `text/html`, "Paste as JSON" is not offered, although `transform` can derive text from HTML (it does this for Markdown and .txt). `supports()` (`actions.rs:113`) requires the text flavour. Test `json_available_for_html_only_clipboard` (actions: `[Markdown, TxtFile, HtmlFile]`).

Passing parity checks: CF_HTML envelope stripped before Markdown; CSV with quotes, `sep=` lines and tabs; INI (including the duplicate-key fallback to the line array); the line-array fallback; Newtonsoft-style indentation for XML; plain text prefers the unmodified text flavour; deep nesting (5000 spans) does not overflow.

### UI vs Advanced Paste
- The picker (`shots/G-launch/paste-picker.png`) renders under Xvfb with a fake `xclip` (`.deps/G-shim/bin/xclip`) serving text and HTML. It shows the title "Advanced Paste", a clipboard preview with "Text · HTML" flavour tags, and numbered formats: plain text, Markdown, JSON, .txt file, .html file. The number keys 1-9 work. Key 3 wrote the expected JSON array to the clipboard. Enter/click works, and Esc cancels.
- PT has the AI prompt box at the top (hidden here because AI is off, like PT when AI is disabled), a clipboard history list, Ctrl+1..9 hints and per-format icons. PT also places the window at the cursor; Rust centres it at 1/3 of the screen height.
- After choosing a format, the actual paste fails here with "no key injection tool found — install xdotool". That is expected in the sandbox (no xdotool) and the error message is clear.
