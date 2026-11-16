# C-system parity report (Hosts Editor, Environment Variables, File Locksmith, New+)

Work in progress; sections appended per module.

## 1. Hosts Editor (crates/hosts-editor vs src/modules/Hosts)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Parse entries (IPv4/IPv6, multiple hosts, inline `#` comment, disabled `#` entries) | Partial: parsing works, but no address/host validation (see bugs 1-4) |
| Address validation (IPv4/IPv6 regex) / host validation (`Uri.CheckHostName`, max 9 hosts) | Missing (parser and Add/Edit form only check non-empty) |
| Split lines with >9 hosts into chunks of 9 | Missing (bug 5) |
| Ignore Windows sample lines (rhino.acme.com, x.acme.com) | Missing (bug 6; low relevance on Linux) |
| "Additional lines" (non-entry lines) editor + `AdditionalLinesPosition` Top/Bottom | Missing. Port keeps comments/blank lines in place instead (arguably better), no dialog to edit them. `new_entry_placement` config exists but is a different feature and is not in any UI |
| Write format: padded columns, `# ` for disabled, 2-space indent if any disabled, `TrimEnd` | Partial: indent/`# ` correct; tab instead of padded columns (bug 8); empty comment re-emitted (bug 7) |
| NoLeadingSpaces | Implemented |
| Encoding UTF-8 / UTF-8 BOM (BOM stripped on read) | Implemented |
| Duplicate detection (same family + shared host, same address with <9 hosts, loopback exclusion, LoopbackDuplicates) | Implemented; family detection differs for zone-index IPv6 and leading-zero IPv4 (bug 9) |
| Filters: address / hosts / comment fields + "show only duplicates" + clear filters | Partial: a single combined text filter + "Duplicates only" |
| Toggle / add / edit / delete entry | Implemented (delete has inline Yes/No confirm) |
| Reorder entries (drag & drop, move up/down), ping entry, context menu | Missing |
| Backup once per session, BackupPath, DeleteBackupsMode Never/Count/Age+Count, days/count | Implemented (names `hosts.mpt-backup-<UTC ts>` instead of `hosts_PowerToysBackup_<local ts>`; default dir `~/.local/share/my-power-toys/hosts-backups` instead of the hosts dir, sensible on Linux) |
| File watching: "hosts file changed externally, reload?" | Missing (bug 10) |
| ShowStartupWarning dialog (Accept/Quit) | Implemented (+ extra "don't show again" checkbox) |
| LaunchAdministrator / NotRunningElevated warning / read-only hosts warning + "make writable" | N/A on Linux; replaced by "write directly, fall back to `pkexec cp`" |
| Open hosts file in Notepad | Missing (no "open in text editor") |

### Bugs
1. **High** – `crates/hosts-editor/src/parser.rs:288-298` (`looks_like_host_entry` only checks that the first word contains `.` or `:`). Comment lines such as `# Note: entries below are managed by ansible`, `# v1.2 of this file`, `# e.g. see hosts(5)` are parsed as *disabled entries* with IP `Note:` / `v1.2` / `e.g.`. On save the comment is rewritten (`# Note:\tentries below...`, space replaced by tab), and if the user toggles that "entry" on, the line `Note:\tentries below are managed by ansible` is written as an active hosts line. PowerToys requires `IPAddress.TryParse` + IPv4/IPv6 regex, so these stay as unparsed additional lines. Evidence: test `comment_with_colon_or_dot_is_not_disabled_entry` fails (`"# Note: ..." parsed as entry ("Note:", ...)`); GUI screenshot `shots/C-system/hosts-02-main.jpg` shows a row "Note:" with a toggle; after Save the temp hosts file contained `# Note:^Ientries below are managed by ansible`.
2. **Medium** – `parser.rs:302-322` `try_parse_entry` accepts any first token as IP for active lines. `10 host`, `host 10.1.1.1`, `  host  10.1.1.1` become entries with IP `10`/`host`. PowerToys `Not_Valid_Entry` rejects all of these. Evidence: `entrytest_not_valid_entry` fails listing 6 lines.
3. **Medium** – `gui/window.rs:139-160` Add/Edit form only checks IP/hostnames are non-empty; `not-an-ip`, `bad!host`, or 20 hosts on one line are accepted and written. PowerToys disables the Add button until `ValidIPv4/ValidIPv6` and `ValidHosts` (≤9 hosts, `Uri.CheckHostName`) pass. Evidence: code read; parser side confirmed by `invalid_hostname_is_not_entry` failing (`10.0.0.1 bad!host` parsed as entry).
4. **Low** – `parser.rs:285-298` repeated hash prefixes: `# #10.1.1.1 host#comment` (and with tab/space) is a comment in the port, a disabled entry in PowerToys (`TrimStart(' ', '#')`). Evidence: `entrytest_valid_entry_repeated_hash_prefix` fails (got None for all 3 rows).
5. **Medium** – no splitting of lines with more than 9 hosts (`Consts.MaxHostsCount`). PowerToys `LongHosts_Splitted` splits 19 hosts into 9/9/1 entries (Windows ignores hosts beyond 9 per line). Evidence: `hostsservice_long_hosts_splitted` fails: `left: [19] right: [9, 9, 1]`. (On glibc there is no 9-host limit per se, so this is a parity gap, not a resolution bug on Linux.)
6. **Low** – Windows sample lines `102.54.94.97 rhino.acme.com # source server` / `38.25.63.10 x.acme.com # x client host` are shown as entries (`Entry.Validate` excludes them). Evidence: `entrytest_sample_lines_are_not_entries` fails; visible as last row in `hosts-02-main.png`.
7. **Low** – `parser.rs:305` / `format_entry` (`parser.rs:325-331`): `10.0.0.1 a #` (empty comment) is stored as `Some("")` and written as `10.0.0.1\ta # ` (dangling `#` + trailing space). PowerToys writes no `#` when the comment is empty and `TrimEnd`s each line. Evidence: `empty_inline_comment_not_written` fails: `"10.0.0.1\ta # \n"`.
8. **Low** – `format_entry` (`parser.rs:325`) uses a single tab between address and hosts and a single space before `#`; PowerToys pads address to max+1 and hosts to max+1 so columns align. Evidence: `hostsservice_host_added_exact_format` fails (`"  10.1.1.1\thost host.local # comment..."` vs `"  10.1.1.1  host host.local     # comment..."`). Functionally harmless; original whitespace is not preserved by either implementation.
9. **Low** – `parser.rs:224-230` address family uses `std::net::IpAddr` parsing: `fe80::1%eth0` (zone index) and `010.0.0.1` (leading zero) are `Invalid`, so they are never duplicates of `fe80::2` / `10.0.0.2` sharing a host. PowerToys' regexes classify them as IPv6 / IPv4. Evidence: `duplicates_ipv6_zone_index_same_family`, `duplicates_ipv4_leading_zero_same_family` fail (`[false, false]` vs `[true, true]`).
10. **Medium** – no file watching. PowerToys' `FileSystemWatcher` raises `FileChanged` and the UI offers to reload. In the port an external edit is invisible and the next Save silently overwrites it. Evidence: GUI run: after Save, `echo "10.9.9.9 external.example" >> /etc/hosts` (bind-mounted temp file) — screenshot `hosts-05-external-change.png` shows no notification and still 8 entries; no watcher code exists in the crate (`grep notify|inotify|watch` → none).
11. **Medium (security, code-confirmed, not run)** – `gui/window.rs:207-218` pkexec fallback writes the new content to a fixed, predictable path `/tmp/mpt-hosts-editor-tmp` with `fs::write` (follows symlinks, default umask) and then runs `pkexec cp <tmp> /etc/hosts`. Another local user can pre-create that path (e.g. a world-writable file they own, or a symlink) and swap content between write and the root `cp`, getting arbitrary content into /etc/hosts. Should use a private `mkstemp`-style file or pipe via `pkexec tee`. Not executed: no pkexec in sandbox and tests run as root (direct write path always taken).
12. **Low (UI)** – delete column glyph `✕` (U+2715, `window.rs:497`) renders as a tofu box with the default egui fonts. Evidence: `hosts-02-main.png` "DEL" column shows `□`.
13. **Low** – `HostsFile::save()` (`parser.rs:191-195`) uses `serialize()` which drops the trailing newline and ignores BOM/leading-space options. The GUI uses `serialize_with`, so only external callers of the public API are affected. Code read.

Verified OK: BOM strip/write, NoLeadingSpaces, CRLF input, comments containing `#`, blank/comment lines preserved verbatim, backup created once per session with exact content (GUI run created one `hosts.mpt-backup-20261116092143`, 311 bytes), backup cleanup by count/age, default settings (warning on, loopback dup off, backup on, Age/15 days/5 count), duplicate highlighting (10.0.0.5/10.0.0.6 `dev.local` shown orange).

### UI differences vs PowerToys (HostsMainPage.xaml)
- Startup warning: same intent, Accept/Quit buttons; port adds "Don't show this warning again" (`hosts-01-startup.png`).
- PowerToys has a command bar with "New entry", "Additional lines", filter flyout (Address/Hosts/Comment/Show only duplicates/Clear), "Open hosts file", Settings; the port has one filter box, "Duplicates only" checkbox, Settings and "+ Add Entry".
- PowerToys entry list: per-row ToggleSwitch, address, hosts, comment, duplicate icon, ping status, context menu (Edit/Ping/Move up/Move down/Delete), drag-to-reorder. Port: coloured dot toggle, columns ON/IP ADDRESS/HOSTNAMES/COMMENT/DEL, click row to edit, no ping/reorder.
- PowerToys Add/Edit is a ContentDialog with validation messages; port is an inline panel with IP/Hostnames/Comment/Active.
- Settings: PowerToys keeps them in the Settings app (Activation: Launch as admin, Show startup warning; Behavior: Additional lines position, Loopback duplicates, No leading spaces, Encoding; Backup: toggle, location, delete mode, count, days). The port has an in-app settings panel (`hosts-03-settings.png`) with the same options except Additional lines position and Launch as admin, plus an extra "Show disabled entries".
- No info bars (file changed / not elevated / read-only / split entries).

### Tests added
- `crates/hosts-editor/tests/parity.rs`: 25 tests, **15 pass, 10 fail** (failures = bugs 1, 2, 4, 5, 6, 7, 8, 9×2, plus `invalid_hostname_is_not_entry` for bug 3). Existing crate unit tests: 21/21 pass.
- GUI harness: `.deps/C-system/hosts-gui.sh` (bind-mounts a temp file over /etc/hosts inside the private mount namespace; the real /etc/hosts is never written). Screenshots `shots/C-system/hosts-0[1-5]-*.png`.

## 2. Environment Variables (crates/environment-variables vs src/modules/EnvironmentVariables)

Oracles: PowerToys `ProfileVariablesSet`, `EnvironmentVariablesHelper`, `EnvironmentVariableComparisonHelper` (+ their unit tests). For "is the generated environment.d file valid", the real **systemd 252** `/usr/lib/systemd/user-environment-generators/30-systemd-environment-d-generator` installed in the sandbox is used: tests write the port's file into a temp `XDG_CONFIG_HOME` and run the generator with `env -i`.

### Feature parity
| PowerToys feature | Status |
|---|---|
| User variables list, add/edit/delete | Implemented (`~/.config/environment.d/50-my-power-toys.conf`) |
| System variables add/edit/delete (needs admin) | Implemented via `/etc/environment.d/50-my-power-toys-system.conf` + `pkexec install` (not exercised: no pkexec) ; other system sources shown read-only |
| Profiles: create/rename/delete, add variables (new or from existing), enable/apply one at a time | Implemented (profiles in `environment-variables-profiles.toml`, applied file `60-my-power-toys-profile.conf`) |
| Apply: back up overridden user variables (`NAME_PowerToys_<profile>`), unapply restores | Implemented with a `backups` table instead of renamed variables; restore works (tests pass) |
| Profile overrides user variables | Partial: user env.d files sorting after `60-` still win (bug 4) |
| "Applied variables" view (merged, which scope wins) | Implemented; expansion semantics differ from systemd (bugs 1-3) |
| List editor for PATH-like variables (add, remove, move up/down) | Implemented (':' separator, extended name list `*_PATH`, `*_DIRS`) |
| Duplicate PATH entry removal with normalisation | Partial: exact-match dedup only, no trailing-slash normalisation (bug 7) |
| Name validation (empty/whitespace/`=`/control chars, ≤259 chars) / value NUL / 32766 total length | Stricter POSIX name rule (letters, digits, `_`), which matches what systemd accepts; newlines rejected; no length limits (N/A) |
| Case-insensitive names, REG_EXPAND_SZ for `%` values, WM_SETTINGCHANGE broadcast | N/A (Linux). Replaced by `dbus-update-activation-environment --systemd` / `systemctl --user set-environment` |
| LaunchAdministrator setting | Re-purposed as "allow editing system variables" |
| Detect profile not correctly applied (`IsCorrectlyApplied`) on startup | Missing |

### Bugs
1. **Medium** – `envd.rs:174-210` (`unquote`) and `envd.rs:259-263` (`expand`): the port treats `'...'` and `\$` as a literal dollar. systemd 252 expands `$VAR` *after* unquoting, so single quotes and `\$` do not protect it; only `$$` gives a literal `$`. Consequences: (a) the Applied view shows values the session will not get; (b) values pushed into the running session (`manager.rs` `session_changes` uses the port's expansion) differ from what the next login produces; (c) the port's own way of writing a literal dollar is broken (bug 2). Evidence: `dollar_escaping_matches_systemd` fails: `MPT_SQ: port "single $HOME" vs systemd "single /h"`, `MPT_DQ: port "dq $HOME" vs "dq /h"`, `MPT_BS: port "$HOME" vs "/h"`, `MPT_DD ($$HOME): port "$/h" vs "$HOME"`.
2. **Medium** – `envd.rs:225-250` `quote_value` writes a logical literal dollar (`\$`) as `\$` inside double quotes. systemd expands it, so a user value meant to be `price $HOME` becomes `price /home/...` at login. Evidence: `port_literal_dollar_stays_literal` fails: written `MPT_LIT="price \$HOME"`, systemd gives `"price /h"`.
3. **Low** – `envd.rs:282-297`: `${VAR:-default}` substitutes when VAR is set but empty; systemd 252 substitutes only when unset (`MPT_D: port "wasempty" vs systemd ""`). `$5` is kept literally by the port, systemd treats it as a variable and drops it (`MPT_F: port "cost $5" vs systemd "cost "`). Evidence: `applied_view_matches_systemd_expansion` fails with those 2 rows (5 other rows match).
4. **Medium** – `profiles.rs:170-183` / `store.rs` precedence: an applied profile only displaces variables in the port's own user file. If another user file sorting after `60-` (e.g. `~/.config/environment.d/90-local.conf`) defines the variable, it overrides the applied profile and nothing warns about it. PowerToys profile values always replace the user variable. Evidence: `manager_on_disk_apply_unapply` fails: `left: Some((User, "emacs")) right: Some((Profile, "nvim"))` (the other assertions in that test, which check apply → persisted state → unapply → restored user file → profile file removed, pass before that line).
5. **Medium** – empty values: `envd::validate` (`envd.rs:211-223`) accepts an empty value and `quote_value("")` writes `KEY=`. systemd 252 rejects that line (`50-t.conf:2: invalid syntax (around "MPT_EMPTY"), ignoring.`). `KEY=""` and `KEY=''` are rejected too. The port shows the variable as set to "" while the login session never gets it. Evidence: `empty_value_is_written_and_kept` fails (systemd result `None`); manual generator run quoted above.
6. **Low** – `EnvFile::parse` (`envd.rs:41`) splits on lines, so a hand-written multi-line double-quoted value (valid in systemd) produces phantom variables, and `\`-newline continuations are truncated. A later save from the port then rewrites those lines as separate assignments. Evidence: `multiline_quoted_value_not_split_into_phantom_vars` fails (`port parsed {"MPT_AFTER": "1", "MPT_PHANTOM": "second"}`); `line_continuation_like_systemd` fails (`port "foo"` vs systemd `"foobar"`).
7. **Low** – `listvalue::dedup` (`listvalue.rs:47-54`) has no normalisation: `/tools/` and `/tools` are kept as two entries. PowerToys' `RemoveDuplicatePathEntries_NormalizesSeparatorsAndTrailingSeparators` treats them as one. Evidence: `dedup_preserves_order_and_normalises_trailing_slash` fails (`"/tools/:/tools:/other"`).
8. **Low** – `store.rs:221-225`: the Applied view and the session push de-duplicate list variables, but systemd does not, so the view hides what the session will really contain. Evidence: `applied_path_not_silently_deduplicated` fails (`port "/a:/b"` vs systemd `"/a:/b:/a"`).
9. **Low (UI)** – the Applied view expands `$PATH` and other variables from the *editor process* environment, so PATH shows the sandbox shell's PATH (`env-04-applied.png`) rather than the login PATH. This is documented in a hover text but can mislead. Observation only.

Verified OK: values with spaces, `"`, `\`, `` ` ``, `#`, `'`, leading/trailing spaces, Windows-style backslashes, `;`, tab, `=`, `~`, `%`, `{}` and a trailing backslash all survive the port's quoting → systemd (`written_literal_values_survive_systemd` passes, 16 values). Invalid names are ignored by both. The profile file header and values are valid for systemd even with a profile name containing quotes and `$`. Apply/unapply backup/restore works, including PATH and a single-quoted user value.

### UI differences vs PowerToys (EnvironmentVariablesMainPage.xaml)
- PowerToys: a single page with expandable sections "Profiles" (each with a toggle to apply and an edit dialog), "User", "System", plus a right-hand "Applied variables" pane always visible. Port: four tabs User / System / Profiles / Applied variables (`env-01-user.png`, `env-02-system.png`, `env-03-profiles.png`, `env-04-applied.png`).
- PowerToys profile dialog: name, enabled toggle, variable list with "Add new" / "Add existing" tabs. Port: inline "New profile name" + Create, Apply/Unapply/Rename/Delete buttons, add new or pick an existing variable from a list.
- Edit dialog: list editor (numbered entries, Add entry, Move up/down, remove) close to PowerToys' (`env-05-edit-path.png`). The per-entry remove button renders as a tofu box `□` (missing glyph).
- No title bar or centring (no WM in the sandbox; window sits at 0,0, size 900x620).
- No info bar for "profile is not correctly applied"; the footer always says "Changes apply to newly started apps; log out and back in if needed."

### Tests added
- `crates/environment-variables/tests/parity.rs`: 15 tests, **6 pass, 9 fail** (failures = bugs 1, 2, 3, 4, 5, 6×2, 7, 8). Existing crate tests: 32/32 pass.
- Screenshots: `shots/C-system/env-0[1-5]-*.png`.

## 3. File Locksmith (crates/file-locksmith vs src/modules/FileLocksmith)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Find processes with open handles to selected files | Implemented (`/proc/<pid>/fd`) |
| Folders searched recursively | Implemented (component-wise prefix match) |
| Loaded modules (DLLs) | Implemented as `/proc/<pid>/maps` (mmaps) + `exe` |
| Working directory | Implemented (`/proc/<pid>/cwd`); on Windows this is a handle too |
| Process name, PID, user, file list ("Show files") | Implemented (+ executable and command line) |
| End task | Implemented: SIGTERM, then SIGKILL after `kill_timeout_ms` (3000) |
| Auto-remove exited processes, Reload | Implemented (`refresh_interval_ms` 2000 + Reload button) |
| Restart as administrator | Implemented via `pkexec env ... --elevated` (not runnable here: no pkexec; already root) |
| Warning for processes of a system user | Partial: port warns only that ending a root process needs admin rights; no warning shown when running elevated (`fl-01-results.png`) |
| Explorer context menu "Unlock with File Locksmith", ExtendedContextMenuOnly (default false) | Replaced by Nautilus script / Dolphin service menu / Thunar custom action, `context_menu` (default true). "Extended menu only" N/A |
| Selected files list dialog / empty state "No results" | Port shows selected path under the title; empty state "No processes are using the selected files" |
| Own-process exclusion | Port excludes its own PID (PowerToys has no explicit exclusion; its UI process does not hold handles) |
| "End all" | Extra in the port (not in PowerToys) |

### Bugs
1. **Low** – `scan.rs:153-171` `normalize_targets` canonicalises the selection, so a folder selected through a symlink reports files under the resolved path (`/…/real/f.txt`), not under the selected path. PowerToys returns `dir_path + kernel_name.substr(...)`, i.e. paths below what the user selected. The process *is* found. Evidence: `symlinked_folder_selection` fails: `left: [".../real/f.txt"] right: [".../link/f.txt"]`.
2. **Low (suspected)** – `integration.rs:157-170` `thunar_insert` returns None for a uca.xml without a `</actions>` closing tag, e.g. a self-closing `<actions/>`, and silently skips Thunar integration (warn log only). Thunar's own writer always emits `<actions>…</actions>`, so this only affects hand-edited files. Evidence: `thunar_uca_without_trailing_newline_and_self_closing` fails on the `<actions/>` assertion; the no-trailing-newline round trip in the same test passes.
3. **Low (code-read)** – `integration.rs:203-207` `install_to` overwrites an existing user-owned `~/.local/share/nautilus/scripts/File Locksmith` and Dolphin `.desktop` file without checking for the marker. `uninstall_from` then deletes it, so the user's file is lost. Uninstall alone correctly leaves foreign files (`uninstall_keeps_foreign_files` passes).
4. **Low (UI)** – the expanded "Files" list shows only the file name (`report.txt`) relative to the selection (`fl-03-expanded.png`). Inside a recursive folder search, files with the same name in different subfolders cannot be told apart. PowerToys lists full paths.

Verified OK with real child processes (python3): open fd on a file, recursive folder (file 3 levels deep, name with a space), cwd inside the folder, mmap with the fd closed, deleted-but-open file (` (deleted)` suffix stripped), own process excluded (and visible with an empty exclusion list), trailing slash and `./..` in the selection, sibling file not matched, process name/cmdline/uid/user. Kill: cooperative process ends by SIGTERM (exit signal 15); a SIGTERM-ignoring process gets SIGKILL after a 300 ms timeout (signal 9, under 2 s); `end_all` mix of Terminated/Killed/NotRunning; an already-exited PID gives NotRunning. GUI: End task on one python row removed it and showed "Ended 1 process(es)" (`fl-04-after-endtask.png`, `fl-05-after-sigkill.png`). Nautilus script passes paths containing spaces, `*`, `'` and `$HOME` verbatim. The Dolphin Exec quoting for a binary path with a space is valid.

### UI differences vs PowerToys (FileLocksmithXAML/Views/MainPage.xaml)
- PowerToys: header with selected paths (tooltip), Reload, "Restart as administrator"; list rows with process icon, name, PID, user, "Show files" (dialog) and End task; system-user warning icon; "No results" empty state.
- Port (`fl-01-results.png`): title "File Locksmith (Administrator)" when root, "Change selection…", Reload, End all; collapsible rows with PID/User/file count and End task; expanded row shows executable, command and files inline. No process icons.
- Port adds a path picker screen when started without arguments (`fl-02-two-windows.png`: text box, Add, Browse files…, Browse folder…, Find processes, drag & drop).
- Window 760x520, no title bar (no WM).

### Tests added
- `crates/file-locksmith/tests/parity.rs`: 18 tests, **16 pass, 2 fail** (bugs 1, 2). Existing crate tests: 16/16 pass.
- Screenshots: `shots/C-system/fl-0[1-5]-*.png`.

## 4. New+ (crates/new-plus vs src/modules/NewPlus)

### Feature parity
| PowerToys feature | Status |
|---|---|
| Templates folder (default `%LOCALAPPDATA%\Microsoft\PowerToys\NewPlus\Templates`), change location | Implemented (`~/.local/share/my-power-toys/new-plus-templates`, `template_location`, `~/` expanded) |
| Example templates created on first run | Implemented (same 3 examples + an extra `01. Text document.txt`) |
| List: folders first, then files, sorted; hidden files excluded | Implemented (dot-files excluded, including dot-folders; PowerToys only excludes hidden/system *files*) |
| Create file / folder (recursive copy) in current folder | Implemented (CLI `create`, GUI, Nautilus scripts, Dolphin service menu) |
| Name conflicts → `Name (1).ext` | Partial: port produces `Name (2).ext`; folders with a dot treated differently (bugs 1-2) |
| Hide file extension (default on) | Implemented |
| Hide starting digits (default on), all upstream cases | Implemented (all 11 table rows pass) |
| Replace variables (default off): `$YYYY…$f` date/time tokens, `%ENV%`, `$PARENT_FOLDER_NAME`, recursive rename inside folders, hidden items skipped | Implemented; `$D`/`$H` lookahead exceptions missing (bug 4); menu-title preview missing (bug 3) |
| Created items get "now" as last-write time | Implemented (std `fs::copy` does not copy mtime) |
| Enter rename mode after creation | N/A (no file-manager API); GUI shows "Created <path>" |
| Hide built-in "New" menu (registry) | N/A |
| Explorer context menu "New+" submenu with icons | Replaced by Nautilus scripts dir `New+`, Dolphin service menu (`X-KDE-Submenu=New+`), optional XDG Templates symlink sync. **No Thunar integration** except through the optional XDG Templates sync (bug 5) |
| Integration removed on stop / when disabled | Implemented (marker-checked, user files kept) |
| Localised month/day names (`GetDateFormatEx`, user locale) | Partial: always English (`chrono` `%B`/`%A`) |

### Bugs
1. **Low** – `templates.rs:247-262` `unique_path` starts at `(2)`: a second "Text document" becomes `Text document (2).txt`. PowerToys `make_unique_path_name` starts its counter at 1 → `Text document (1).txt`. Evidence: `unique_name_counter_starts_at_one` fails (`Note (2).txt` vs `Note (1).txt`); end-to-end CLI run in `end_to_end_integration_files` fails on the same point (`My Docs/Text document (2).txt`).
2. **Low** – `unique_path` never splits folder names at a dot: folder `my.project` → `my.project (2)`; PowerToys uses `stem()`/`extension()` for folders too → `my (1).project`. The port's behaviour is arguably nicer; listed for parity. Evidence: `unique_folder_name_with_dot` fails.
3. **Low** – `Template::display_name` (`templates.rs:21-31`) never resolves variables. PowerToys `get_menu_title(..., show_resolved_variables)` shows e.g. `2026-11-16 journal` in the menu when "Replace variables" is on. The port shows `$YYYY-$MM-$DD journal` in the GUI list, Nautilus script names and Dolphin action names. Evidence: `menu_title_previews_variables` fails (`menu title not resolved: $YYYY report`); screenshot `np-03-created.png` (list shows `$YYYY-$MM-$DD journal`, the created file is `2026-11-16 journal.md`).
4. **Low** – `resolve_date_time` (`templates.rs:313-334`) replaces `$D`/`$H` everywhere, while upstream `GetDatedFileName` uses `$D(?!ATE_TAKEN_|ESCRIPTION|OCUMENT_ID)` and `$H(?!EIGHT)`. `$DESCRIPTION` becomes `5ESCRIPTION`. Evidence: `date_tokens_do_not_eat_metadata_names` fails (`left: "5ESCRIPTION" right: "$DESCRIPTION"`). Rare in template names.
5. **Medium (scope)** – `integration.rs` writes Nautilus and Dolphin integration only; Thunar gets nothing unless the user turns on `sync_xdg_templates` (default off), and that path gives no folder templates, no digit/extension/variable handling and no New+ submenu. Evidence: `thunar_integration_exists` fails; `grep -i thunar crates/new-plus/src` → no matches.
6. **Low (code-read)** – `integration.rs:157-160` checks `path.exists()` before writing a Nautilus script. A dangling symlink placed by the user under `nautilus/scripts/New+/<label>` reports "does not exist", and `fs::write` then follows it and creates the symlink target.

Verified OK: digit-stripping table (11 cases); menu title/target name for the 4 combinations of hide-extension/hide-digits; folders keep dotted names; date tokens including 12 AM at midnight, `$$` escaping, ms tokens; `%VAR%` with unknown/empty variables and `%%`; `$PARENT_FOLDER_NAME` (case-sensitive); `/` from a variable replaced by a space; folder template with variables in nested names (`proj kit/2026 sub/%NOPE% …`), dot-files not renamed; mtime = now for created files and for files inside created folders; listing order. End to end with temp HOME: `apply()` generated one Nautilus script per template (+ "Open templates folder"); running the generated "Meeting notes" script with `NAUTILUS_SCRIPT_CURRENT_URI=file://…/My%20Docs` created `My Docs/Meeting notes.md` with the template content; the Dolphin `.desktop` file is executable with `X-KDE-Submenu=New+` and `MimeType=inode/directory;`; removing a template removes its script on re-apply; `context_menu=false` and `remove_all()` (stop) delete all generated files and keep the user's own script; XDG sync creates and removes symlinks. GUI: selecting the `$YYYY…` template and clicking Create (with `replace_variables = true`) created `2026-11-16 journal.md`.

### UI differences vs PowerToys
- PowerToys has no window: New+ is an Explorer context submenu (template icons, "Open templates folder" entry) plus the Settings page (Templates location + change button + backup warning; Display options: hide extension, hide starting digits, hide built-in New; Behavior: replace variables with a variables cheat-sheet). The port adds a standalone egui window (`np-01-main.png`): template list with folder/file icons, "Create in:" path + Browse…, Create, Open templates folder, Refresh, status line.
- The port window does not show the variables cheat-sheet; settings live in the shared settings app (not reviewed here).
- The window does not pick up new templates until Refresh is clicked (`np-02-before-refresh.png` vs `np-03-created.png`); PowerToys rescans the folder each time the menu opens.
- The action buttons move up when the status line appears. This is a layout jump, not a bug: my second Create click landed on empty space.

### Tests added
- `crates/new-plus/tests/parity.rs`: 17 tests, **11 pass, 6 fail** (bugs 1×2, 2, 3, 4, 5). Existing crate tests: 9/9 pass.
- Screenshots: `shots/C-system/np-0[1-3]-*.png`.

---

## Summary
| Module | New tests | Pass | Fail | Existing unit tests |
|---|---|---|---|---|
| Hosts Editor | 25 (`crates/hosts-editor/tests/parity.rs`) | 15 | 10 | 21/21 |
| Environment Variables | 15 (`crates/environment-variables/tests/parity.rs`) | 6 | 9 | 32/32 |
| File Locksmith | 18 (`crates/file-locksmith/tests/parity.rs`) | 16 | 2 | 16/16 |
| New+ | 17 (`crates/new-plus/tests/parity.rs`) | 11 | 6 | 9/9 |

No existing source files were modified; only the four `tests/parity.rs` files were added. Helpers live in `.deps/C-system/` (`hosts-gui.sh`, `xt.py`) and `.deps/withx-C-system.sh` (Xvfb :123). No GUI crashed or panicked. All four egui apps rendered with the default backend plus `LIBGL_ALWAYS_SOFTWARE=1`. All UI strings are in English. Some symbol glyphs (✕, list remove buttons) render as tofu boxes.
