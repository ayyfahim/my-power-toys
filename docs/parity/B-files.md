# B-files report: Bulk Rename & Image Resizer

Rust port: this repository at commit `3a4491e`; only test files were added.
Oracle: `PowerToys/src/modules/powerrename` and `src/modules/imageresizer`.

---

## Module 1: Bulk Rename (`crates/bulk-rename`, package `mpt-bulk-rename`) vs PowerRename

### Feature parity

| PowerToys feature | Rust status |
|---|---|
| Plain search/replace, first vs all, case sensitivity | Implemented (ASCII OK; Unicode case-insensitive path broken, bug 1) |
| Regex (ECMAScript std::wregex default, optional Boost) | Partial: Rust `regex` crate, no lookahead/backreferences (bug 4); no "Use Boost library" option |
| `$1`..`$9` capture groups, `$$` escape | Partial: multi-digit / `$0` handling differs (bug 5) |
| Apply to filename / extension / both | Partial: folders are split by dot, dotfiles mis-split (bugs 7, 8) |
| Folders skipped for "Extension only" | Missing (bug 7) |
| Include files / include folders / include subfolders toggles | Partial: `include_folders`/`include_subfolders` exist only in config + listing; no GUI toggles; no "exclude files" option at all |
| Enumerate `${}`, `start=`, `increment=`, `padding=` | Implemented; padding clamped to 64 instead of `% 260` (bug 12, low) |
| Counter advances on match even when name unchanged | Implemented (test passes) |
| Randomizer `${rstringalnum/alpha/digit=N}`, `${ruuidv4}` | Implemented, but with regex on and randomizer off the literal `${rstring…}` text is eaten (bug 6) |
| Date/time tokens `$YYYY…$fff`, `$HH/$H/$TT/$tt` 12h | Implemented (all HelpersTests date cases pass); month/day names English-only (upstream: user locale) |
| File time source Creation/Modification/Access | Implemented (Created falls back to mtime when birth time missing) |
| EXIF metadata tokens | Implemented (EXIF) |
| XMP metadata source & tokens ($DESCRIPTION, $DOCUMENT_ID, …) | Missing; `$DESCRIPTION` is also corrupted by `$D` (bug 11) |
| Lowercase / Uppercase respecting Name-only / Ext-only | Partial: always applied to the whole name (bug 3) |
| Title case (word-wise, small-word exceptions, extension untouched) | Wrong: only first character upper-cased (bug 2) |
| Capitalize each word (lower rest, punctuation boundaries, extension untouched) | Partial (bug 2) |
| Trim leading whitespace and trailing whitespace/dots | Missing (bug 9) |
| Invalid-character / too-long name detection | Missing; `/` moves the file into another folder (bug 10) |
| Unicode NFC normalisation, NBSP→space | Missing (bug 13) |
| Conflict handling (FOF_RENAMEONCOLLISION) | Missing: operation aborts (bug 14) |
| Nested folders renamed depth-first | Missing: parent renamed first, child rename fails (bug 15) |
| Undo | Partial: works for clean runs; impossible after a partial failure (bug 16) |
| MRU (autocomplete) + max items, persist state ("restore flags on launch") | Implemented (MRU dropdown "▾", state file `bulk-rename-state.toml`) |
| Filter "only show files that will be renamed", select-all checkbox | Missing (per-row checkboxes exist) |
| Apply and close | Missing (only Apply) |
| Explorer context menu, hide icon, extended menu, HEIF/AVIF store extensions | N/A on Linux (CLI args / file-manager integration instead) |

### Bugs

1. **High: panic / wrong result in case-insensitive plain replace with Unicode.** `crates/bulk-rename/src/renamer.rs:432-447` (`plain_replace_case_insensitive`) searches in `input.to_lowercase()` but slices the original `input` with those byte offsets and advances by `search.len()`. If lowercasing changes the byte length, this panics or produces the wrong name. With the default options (case-insensitive), renaming `İstanbul_foo` with foo→bar panics at `renamer.rs:442`: "byte index 14 is out of bounds of `İstanbul_foo`". This would crash the GUI preview. With `Kx_foo_y` (Kelvin sign) the result is `Kbaroo_y`. PowerRename gives `İstanbul_bar` / `Kx_bar_y`. Evidence: `parity_tests::case_insensitive_plain_unicode_length_change_no_panic`, `case_insensitive_plain_kelvin_sign` (both FAIL).
2. **Medium: Title case and Capitalize do not match PowerRename.** `renamer.rs:368-399`. TitleCase only upper-cases the first character. Upstream (`Helpers.cpp:200-290`) capitalises every word except a/an/to/the/at/by/for/in/of/on/up/and/as/but/or/nor (first and last word are always capitalised), lowers the other letters, treats punctuation as a word boundary (with an apostrophe rule), and leaves the extension untouched. CapitalizeEachWord capitalises the extension (`.Txt`), never lowers letters (`hELLO` stays `HELLO`), and ignores `'` as a boundary.
   - Got `Bar And The To.txt`, expected `Bar and the To.txt`.
   - Got `'bar I'll 'and' … Y'all.Txt`, expected `'Bar I'll 'And' … Y'all.txt`.
   - Got `HELLO WORLD.Txt`, expected `Hello World.txt`.
   Evidence: `manager_titlecase_transform`, `manager_titlecase_with_apostrophes`, `manager_capitalized_transform`, `capitalize_lowercases_rest_of_word` (all FAIL). These cases are ported from PowerRenameManagerTests.cpp.
3. **Medium: Upper/lower case ignores "Apply to".** `renamer.rs:150-151` applies formatting to the whole name. Upstream transforms only the stem for NameOnly and only the extension for ExtensionOnly. Example: NameOnly + Uppercase on `foo.foo` gives `BAR.FOO`; upstream gives `BAR.foo`. Evidence: `manager_name_only_uppercase_transform` (FAIL).
4. **Medium: ECMAScript regex features rejected.** `renamer.rs:85-93` uses the `regex` crate, which rejects lookahead and backreferences. Upstream's default std::wregex (ECMAScript) supports both. The port's own Quick Reference (`gui/window.rs:921`) lists `.+?(?=bar)`, which fails with "invalid regex pattern … look-around not supported". Lookbehind fails in both (parity OK). Evidence: `ecmascript_lookahead_supported`, `ecmascript_backreference_supported` (FAIL); screenshot `shots/B-files/bulk-rename-help.png`.
5. **Medium: capture-group reference parsing differs.** `tokens.rs:432-445` (`normalize_group_refs`) reads up to 2 digits and keeps `$0`. Upstream (`PowerRenameRegEx.cpp:464-468`) takes a single digit 1-9 and leaves `$0…` literal. Results for `(foo)(bar)` on `foobar`:
   - `$12`: expected `foo2`, got ``.
   - `$10`: expected `foo0`, got ``.
   - `$01`: expected `$01`, got `foo`.
   - `$$$11`: expected `$foo1`, got `$`.
   - `$1_$002_$223…`: expected `foo_$002_bar23_…`, got `foo_foobar2_3_…`.
   Evidence: `capturing_groups_table_std_engine` (FAIL, 6 of 9 rows).
6. **Low: with regex on, a disabled `${...}` group disappears.** The Rust regex replacer treats `${}` / `${rstringalnum=9}` as a named group and expands it to empty. Upstream keeps it literally: `VerifySimpleCounterNoEnum` gives `foobar_${}` and `VerifyRandomizerNoRandomizerRegEx` gives `foobar_${rstringalnum=9}`. Rust gives `foobar_` in both. `renamer.rs:296-302`. Evidence: `counter_disabled_regex_leaves_group_literal`, `randomizer_flag_cases` (FAIL).
7. **Medium: folders are split into stem/extension and are not skipped for Extension-only.** `split_name` (`renamer.rs:327-348`) does not check whether the item is a directory. Upstream (`Renaming.cpp:87-123`) always uses the whole folder name and excludes folders when ExtensionOnly is set. NameOnly on folder `foo.foo` gives `bar.foo` (expected `bar.bar`). ExtensionOnly + Lowercase on folder `bar.FOO` renames it to `bar.bar` (expected: not renamed). Evidence: `folder_name_is_not_split_into_stem_and_extension`, `manager_extension_only_lowercase_transform` (FAIL).
8. **Low: dotfiles mis-split.** `rfind('.')` treats `.foo` as stem `""` + ext `.foo`. C++ `fs::path(".foo").stem()` is `.foo`. NameOnly foo→bar on `.foo` leaves it unchanged (expected `.bar`). Evidence: `dotfile_stem_is_whole_name` (FAIL).
9. **Medium: no trimming of the new name.** Upstream `GetTrimmedFileName` (`Helpers.cpp:65-91`, applied in `Renaming.cpp:248`) strips leading whitespace and trailing whitespace and dots. Rust: txt→"" on `foo.txt` gives `foo.` (expected `foo`), and `" bar"` keeps its leading space. Evidence: `result_is_trimmed_of_trailing_dots_and_spaces` (FAIL).
10. **High: `/` in the replacement moves the file to another directory.** `renamer.rs:154` uses `file.with_file_name(new_name)`, and nothing validates the new name. Replacing `a` with `x/y` on `/d/a.txt` previews `/d/x/y.txt`. On Apply, `fs::rename` moves the file into subfolder `x` if it exists. Upstream flags `\ / : * ? " < > |` as ItemNameInvalidChar and does not rename (`Renaming.cpp:272-289`). Evidence: `slash_in_new_name_does_not_move_file` (FAIL).
11. **Low: `$DESCRIPTION` / `$DOCUMENT_ID` corrupted to `15ESCRIPTION`.** Upstream's `$D` negative lookahead protects the XMP patterns. Rust has no XMP pattern list (`tokens.rs:85-111`). Evidence: `dated_name_protects_xmp_patterns` (FAIL).
12. **Low: enumerate padding clamped to 64** (`tokens.rs:297`). Upstream uses `padding % MAX_PATH`. `${padding=100}` gives 64 chars instead of 100. Evidence: `counter_padding_is_modulo_max_path_not_clamped_to_64` (FAIL).
13. **Low: no NFC normalisation or NBSP→space** (upstream `SanitizeAndNormalize`, PowerRenameRegEx.cpp:24-56). NFD `Testй` is not matched by NFC `Testй`, and `Hello\u00A0World` is not matched by `Hello World`. Evidence: `unicode_nfd_source_matches_nfc_search`, `nbsp_source_matches_space_search` (FAIL).
14. **High: a conflict aborts the whole batch.** `renamer.rs:191-193` bails with "target already exists". Upstream uses `FOF_RENAMEONCOLLISION` (PowerRenameManager.cpp:16), which renames to "name (2).ext" and continues. Evidence: `rename_conflict_renames_on_collision` (FAIL: "target already exists: …/b.txt").
15. **High: renaming a folder together with its contents fails.** `list_entries` sorts parents before children (`renamer.rs:246-250`), and `execute` renames in that order using stale child paths. Upstream adds items depth-first (PowerRenameManager.cpp:754). Folder `foo/` containing `foo.txt`, foo→bar: `foo` becomes `bar`, then "failed to rename …/foo/foo.txt -> …/foo/bar.txt: No such file or directory". Evidence: `nested_folder_and_child_both_renamed` (FAIL).
16. **High: after a partial failure, the completed renames cannot be undone.** In `execute` (`renamer.rs:183-217`), `bail!`/`?` return before `self.history.push(op)`. Renames already done in that batch are lost from history, and `undo()` returns "nothing to undo". Evidence: `partial_failure_can_still_be_undone` (FAIL: "a1.txt lost after partial failure; undo returned Err(nothing to undo)").
17. **Low (code-confirmed, not run): Undo re-lists the whole parent directory.** `gui/window.rs:248-262`: after Undo, the file list is replaced by `list_entries(source_dir)`, where `source_dir` is the parent of the first CLI file. If the user passed only some files, Undo widens the list to every file in that folder.
18. **Low: missing glyphs in the GUI.** The MRU "▾" button, the "→" arrows in the Quick Reference and the Quick Reference toggle icon render as empty boxes, because the font lacks those glyphs. Evidence: `shots/B-files/bulk-rename-help.png`.
19. **Low: Apply/Undo footer is not pinned.** Opening Quick Reference pushes the file list and the Apply/Undo footer out of the fixed 900x640 window. Evidence: `shots/B-files/bulk-rename-help.png`.

Confirmed working under Xvfb (`shots/B-files/bulk-rename-enumerate.png`, `after-apply.txt`, `after-undo.txt`):
- Search `photo`, replace `img_${start=1,padding=3}` with Enumerate previews `img_001_001.jpg` and `img_002_002.jpg`.
- Apply renamed the files on disk, and Undo restored them.
- Options and MRU were persisted to `~/.config/my-power-toys/bulk-rename{,-state}.toml`.
- Side note: options are persisted only on Apply, so a toggle survives to the next launch even if nothing was renamed. This matches upstream "restore flags on launch".

### UI differences vs PowerRename (PowerRenameUILib/PowerRenameXAML/MainWindow.xaml)
- Labels: the chips are "Regex", "Match all", "Aa Case", "# Enumerate", "? Randomize". Upstream uses checkboxes "Use regular expressions", "Match all occurrences", "Case sensitive", plus icon toggles for enumeration and random string features.
- Apply-to: chips "Name / Ext / Name+Ext". Upstream has a combo box "Apply to" with Filename + extension / Filename only / Extension only.
- Text formatting: chips `— aa AA Aa "Aa Aa"`. Upstream has 4 toggle buttons and no explicit "none" button.
- Missing:
  - Include files / folders / subfolders toggles.
  - Metadata source selector (EXIF/XMP).
  - Filter (show all / only renamed) and the select-all checkbox.
  - Apply and close, and the Settings button.
  - Error states (invalid char, too long).
- Time source: chips "Created / Modified / Accessed". Upstream has the combo box "Time used for replacement".
- Help: one inline "Quick Reference" panel. Upstream has separate flyouts for RegEx, counter, random, date/time and metadata. Content is similar. The 24h/12h hints are correct.
- Window has a header "Bulk Rename / Rename files with patterns, preview and undo". No French or untranslated strings were seen.
- Settings page: upstream has the AutoComplete toggle, max items, "Restore flags on launch" and "Use Boost library". The Rust config has `mru_enabled`, `max_mru_size`, `persist_state`; there is no Boost option. The settings UI crate was not assessed by this group.

### Tests added
- `crates/bulk-rename/src/parity_tests.rs`, registered in `crates/bulk-rename/src/lib.rs` with `#[cfg(test)] mod parity_tests;`. It has 46 parity tests (many of them are tables of upstream cases).
- Run: `cargo test -p mpt-bulk-rename` reports **55 passed, 24 failed** (79 tests). The 33 pre-existing tests all pass. The parity tests are **22 pass / 24 fail**.

---

## Module 2: Image Resizer (`crates/image-resizer`, package `mpt-image-resizer`) vs PowerToys Image Resizer

### Feature parity

| PowerToys feature | Rust status |
|---|---|
| Default presets Small 854x480, Medium 1366x768, Large 1920x1080, Phone 320x568 (Fit, px) | **Wrong**: width-only presets 640/1280/1920/1080 with auto height (bug 1) |
| Initially selected size = index 0 (Small) | Wrong: Medium (bug 3) |
| User-editable presets (add / edit / remove), Custom size (Fit 1024x640 px) | Implemented (inline "Edit presets…" editor; custom default matches) |
| Fit / Fill (centre crop) / Stretch | Implemented (tests pass) |
| Units px / % / inch / cm | Partial: inch and cm always use 96 DPI instead of the image DPI (bug 5) |
| Percent uses the width % for both axes (Fit/Fill), separate for Stretch | Implemented |
| Shrink only (default **off**) | Logic implemented and matches; default is **on** (bug 2) |
| Ignore orientation (default on, skipped for auto/percent) | Implemented |
| Overwrite (replace originals) | Implemented (temp file, then rename) |
| File name format %1..%6, sanitising, "(N)" uniquifier | Implemented; a sub-directory in the format is not supported (bug 6) |
| JPEG quality (default 90, 1-100) | Implemented, default 85 (bug 2); the GUI slider is shown only when the output format is JPEG (bug 9) |
| PNG interlace (Default/On/Off) | **Missing** (no option; the `image` crate never interlaces) |
| TIFF compression (Default/None/CCITT3/CCITT4/LZW/RLE/Zip) | **Missing** |
| Fallback encoder (PNG/BMP/JPEG/TIFF/WMPhoto/GIF) | Partial: option exists (WebP instead of WMPhoto); ICO is written natively, so the fallback is never used where upstream uses it (bug 8, low) |
| Keep date modified (Use original date / Use resize date) | Implemented (also with Replace) |
| Remove metadata (keep by default) | **Missing**: metadata is always dropped (bug 4) |
| Multi-frame images (GIF/TIFF): every frame resized | **Missing**: only the first frame is kept (bug 7) |
| Stop/cancel during a batch; results page listing failures | Partial: no Stop; there is a summary line "All N images resized successfully" |
| AI super resolution | N/A (Windows AI) |
| Explorer context menu / drag-drop | N/A; CLI args + "Add files" dialog instead |
| Extra (not in upstream): output format Original/PNG/JPEG/WebP, output folder picker | Rust-only |

### Bugs

1. **High: default size presets are wrong.** `crates/image-resizer/src/resizer.rs:31-38, 221-232`: `default_sizes()` builds `SizePreset::max_width` presets of 640/1280/1920/1080 px wide with auto height. PowerToys (`ui/Properties/Settings.cs:105-111`, `Settings.UI.Library/ImageResizerProperties.cs:40-46`) uses Fit boxes 854x480, 1366x768, 1920x1080 and 320x568. On a 4000x3000 photo:
   - Medium gives 1280x960 (PowerToys 1024x768).
   - Large gives 1920x1440 (PowerToys 1440x1080).
   - Phone gives 1080x810 (PowerToys 427x320).
   The GUI also shows "Fit 1280 × auto px" (`shots/B-files/image-resizer-initial.png`) and the preset editor shows 640×0 … 1080×0 (`shots/B-files/image-resizer-edit-presets.png`). Evidence: `parity_tests::default_size_presets_match_powertoys`, `default_presets_resize_real_photo` (FAIL).
2. **Medium: wrong defaults for Shrink only and JPEG quality.** `lib.rs:38-40, 77-79, 89-92`: `shrink_only` defaults to `true` and `quality` to 85. Upstream defaults are `ImageresizerShrinkOnly=false` and `ImageresizerJpegQualityLevel=90`. The GUI starts with "Make pictures smaller but not larger" checked. Evidence: `default_options_match_powertoys` (FAIL), screenshot `image-resizer-initial.png`.
3. **Low: wrong initially selected size.** With an empty `selected_size`, `active_size()` falls back to the legacy `preset` field, whose default is Medium (`lib.rs:65-67, 116-118`). Upstream `SelectedSizeIndex=0` selects Small. Evidence: `default_options_match_powertoys` (FAIL, "initially selected size is not index 0"); GUI starts on "Medium".
4. **High: metadata is always stripped; "Remove metadata" is missing.** `resizer.rs:420-433` decodes with `image::open` and re-encodes, so EXIF, XMP, ICC, orientation and PNG text chunks are lost on every resize. Upstream keeps metadata unless "Remove metadata that doesn't affect rendering" is checked, and even then keeps Orientation and ColorSpace (`ResizeOperation.cs:41-45, 485-506`). A JPEG with EXIF Make=Canon and Orientation=6 comes out with no `Exif` APP1 block. Because the orientation tag is dropped and the pixels are not rotated, EXIF-rotated camera photos are displayed in the wrong orientation after resizing. Evidence: `metadata_kept_by_default` (FAIL: "EXIF block dropped").
5. **Medium: inch/cm units ignore the image DPI.** `resizer.rs:423-429` hard-codes `(96.0, 96.0)`. Upstream passes `decoder.DpiX/DpiY` (`ResizeOperation.cs:97`; test `TransformHonorsUnit`). For a 600x600 PNG with 300 DPI pHYs, a 1in×1in Fit gives 96 px wide instead of 300. Evidence: `inch_unit_uses_image_dpi` (FAIL: left 96, right 300). Images without DPI info give 96 px in both (`inch_unit_without_dpi_uses_96` passes). The output also loses the pHYs/DPI chunk (same root cause as bug 4).
6. **Low: a directory in the file-name format is flattened.** `format_file_name` (`resizer.rs:313-319`) turns `/` and `\` in the template into `_`. Upstream only sanitises the size name; path separators in the format create sub-folders (`ExecuteHandlesDirectoriesInFileNameFormat`, ResizeOperation.cs:776-805 calls `CreateDirectory`). `Directory/%1 (%2)` produces `Directory_Test (Test).png`. Evidence: `file_name_format_with_directory` (FAIL).
7. **Medium: only the first frame of animated GIF / multi-page TIFF is kept.** `image::open` returns frame 0 (`resizer.rs:421`). Upstream loops over `decoder.FrameCount` (`ResizeOperation.cs:376-450`; test `ExecuteTransformsEachFrame`). A 2-frame GIF comes out with 1 frame. Upstream also shows the warning "Gif files with animations may not be correctly resized"; Rust shows no warning. Evidence: `every_gif_frame_is_resized` (FAIL: frame count 1 vs 2).
8. **Low: ICO does not use the fallback encoder.** `target_format` (`resizer.rs:355-366`) keeps ICO because the `image` crate can write it, giving `Test (Test).ico`. Upstream writes `Test (Test).png` with FallbackEncoder=PNG (`ExecuteUsesFallbackEncoder`). This is a codec-availability difference. Evidence: `ico_uses_fallback_encoder` (FAIL).
9. **Low (code-confirmed): JPEG quality cannot be edited for JPEG inputs in Original mode.** The quality slider is drawn only when `output_format == Jpeg` (`gui/window.rs:1006`). The default "Original" mode still encodes JPEG inputs with `config.quality` (85), and the user cannot see or change that value.
10. **Low: missing glyphs.** The per-file remove buttons, the preset editor "↑"/"✕" buttons and the success "✓" render as empty boxes (`shots/B-files/image-resizer-initial.png`, `image-resizer-edit-presets.png`, `image-resizer-results.png`).
11. **Info (not a parity bug): the debug build is slow.** Resizing one 4000x3000 JPEG took about 60 s, and the batch showed "Resizing... 0/2" for more than 8 s (`image-resizer3.log`, `shots/B-files/image-resizer-results.png`). Release build speed was not measured.

GUI end-to-end run (`shots/B-files/image-resizer-results.png`): files passed on the CLI were listed, Small was selected, and Resize produced `photo (Small).jpg` at 640x480 and `tall (Small).png` at 600x900. The tall image was unchanged because shrink-only was on and the width-only Small preset is 640 wide. With PowerToys defaults (Small 854x480 box, shrink-only off) `tall.png` would become 320x480. Status showed "All 2 images resized successfully". The settings file `~/.config/my-power-toys/image-resizer.toml` was written with `selected_size = "Small"`. No crashes or panics.

### UI differences vs PowerToys (`ui/ImageResizerXAML/Views/InputPage.xaml` + `ImageResizerPage.xaml`)
- Upstream input page has:
  - a size ComboBox ("Select a size") with each preset described ("Fit 854 × 480 pixels"), and a Custom row with width/height/unit/fit fields;
  - checkboxes "Make pictures smaller but not larger", "Overwrite files", "Ignore the orientation of pictures", "Remove metadata that doesn't affect rendering";
  - "Open settings" and Resize/Cancel buttons;
  - a progress page with Stop and a results page listing failures.
- Rust is a single scrolling page with these sections: IMAGES (file list, Add files, Clear all), SIZE PRESET (chips plus an inline preset editor), OUTPUT FORMAT, OPTIONS (shrink, overwrite, orientation, keep date, file name, fallback encoder), OUTPUT folder, and the Resize button.
  - "Remove metadata" is missing.
  - Stop/Cancel is missing.
  - The Resize button is below the fold at the default window size and needs scrolling.
- Settings page items that Rust moved into the main window: preset editor, fallback encoder, file name format, keep date.
- Settings page items that Rust lacks: PNG interlacing, TIFF compression, the file-name parameter help (%1..%6 legend), and "Use original date / Use resize date" wording (Rust uses a checkbox).
- Labels are English and close to upstream wording ("Overwrite files (replace originals)"). No French or untranslated strings were seen.

### Tests added
- `crates/image-resizer/src/parity_tests.rs`, registered in `crates/image-resizer/src/lib.rs` with `#[cfg(test)] mod parity_tests;`. It has 22 tests that generate real PNG/JPEG/GIF/ICO files (including EXIF and pHYs injection), resize them through `resize_image_with`, and check output dimensions, file names, pixels, mtime, frames and metadata.
- Run: `cargo test -p mpt-image-resizer` reports **30 passed, 8 failed** (38 tests). The 16 pre-existing tests all pass. The parity tests are **14 pass / 8 fail**.
- Failing parity tests:
  - `default_size_presets_match_powertoys`
  - `default_presets_resize_real_photo`
  - `default_options_match_powertoys`
  - `metadata_kept_by_default`
  - `inch_unit_uses_image_dpi`
  - `every_gif_frame_is_resized`
  - `file_name_format_with_directory`
  - `ico_uses_fallback_encoder`

---

## Environment notes
- Helpers added under `.deps`:
  - `withx-B-files.sh`: Xvfb on :122.
  - `xin-B-files.py`: XTest click/type/wheel, plus `f` to set X input focus.
- Without a window manager, egui ignored keyboard input until `XSetInputFocus` was called on the window. Both egui apps rendered fine with `LIBGL_ALWAYS_SOFTWARE=1`.
- Sample data: `.deps/samples-B/`. Screenshots: `shots/B-files/`.
- Source files changed in the repo: only the two new `parity_tests.rs` files and the one-line `mod parity_tests;` registrations in each `lib.rs`. No product code was changed and nothing was committed.
