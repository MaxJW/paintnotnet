# Roadmap

Design and rationale: [PLAN.md](PLAN.md). Each phase ends with a usable app and a check-in
where you try it. A phase is only ticked off once its acceptance checks have been run and
shown passing.

**Current state (2026-10-13):** Phases 0–4 are complete, verified and committed.
**Next: Phase 5 (feather).** Functionality first: the user wants every feature working
before the visual refresh (Phase 12).

Reference source: paint.net 3.36.7 (OpenPDN, MIT code) is cloned at `reference/OpenPDN`
(git-ignored; re-clone with `git clone --depth 1 https://github.com/rivy/OpenPDN reference/OpenPDN`).
Port behaviour from it; never use Pinta; never decompile 4.x/5.x. See PLAN.md §2.

Priority order you gave: keybinds and layout → layers → tools → feather. Feather depends on
selections, so selections come right before it.

---

## Phase 0: Groundwork
- [x] Toolchain: rustup, Rust 1.99.0 stable (mold skipped; not needed yet)
- [x] `git init` (branch `main`), `.gitignore` (target/, reference/, scratch/)
- [x] Cargo workspace: six library crates (doc stubs + dependency edges) + `apps/paintnotnet` + `xtask`; eframe/egui 0.36.2 and wgpu 30.0.1 pinned exactly
- [x] eframe window, wgpu backend, LOW_LATENCY surface, Wayland app_id `paintnotnet`
- [x] Diagnostics overlay (Ctrl+Alt+Shift+`, the paint.net diagnostic chord; on by default in debug builds): window system, GPU, backend, driver, monitor, FPS, frame mean/p99/max
- [x] `--smoke-test SECONDS`: runs, prints one summary line, exits
- [x] `cargo xtask ci`: fmt check, clippy `-D warnings`, tests (6 tests: frame stats + headless egui_kittest chord tests)
- [x] Verified 2026-10-09: `window_system=Wayland backend=Vulkan adapter="NVIDIA GeForce RTX 3070"`, steady vsync (60.0 fps, p99 17.9 ms) on the 60 Hz HDMI-A-1 monitor where KWin placed the window
- [x] 165 Hz on DP-1, run by you (Wayland won't let the app choose its monitor): `monitor="2560x1440 @1x" fps=164.6 mean_ms=6.07 p99_ms=7.27 max_ms=7.79`
- **Accept:** window opens on Wayland, renders at display rate, `cargo xtask ci` passes. **Met.**

Lessons for Phase 2's keymap engine, learned here:
- egui has no `Key` for shifted symbols such as `~`, so chords must also match `physical_key`.
- egui recomputes `Event::Key::repeat` from its own key-down state.
- Kittest feeds one queued event per frame, so UI tests must step once per event.

## Phase 1: Document core and canvas ✅
- [x] Tiled surface (256², Arc copy-on-write, sparse), Layer, Document, `Mask` type (`pnn-core::tiles`)
- [x] All 14 blend ops ported from paint.net 3.36 `UserBlendOps` (bit-exact against 4,984 vectors from `scripts/blend_reference.py`, which transliterates the C# macros and reads the original `masTable`)
- [x] CPU compositor following paint.net's `Document.Render` (bottom-layer fast path, opacity, visibility), per-tile revisions, rayon
- [x] wgpu canvas: 4096² texture chunks, premultiplied mips, nearest ≥100% / trilinear <100%, paint.net checkerboard (8 px, 191/255), dotted black pixel grid ≥200% (ported from `SurfaceBoxGridRenderer`)
- [x] View: paint.net zoom presets 1–3200% (`ScaleFactor`), fit-to-window (`DocumentView.ZoomToWindow`, refits on resize until you zoom), zoom about cursor, clamped scrolling, scrollbars
- [x] Keys: wheel, Shift+wheel, Ctrl+wheel, Ctrl +/=/-/0, Ctrl+B (×2 restores), PgUp/PgDn, Shift+PgUp/PgDn, Home/End ×1/×2, Shift+Home/End, Ctrl+Home/End, Space+drag, middle-drag, Space(+Ctrl)+arrows
- [x] New Image dialog (Ctrl+N; white "Background" layer, keep-aspect, size estimate); Open (KDE picker via portal, multi-select, background decode); CLI `paintnotnet FILE...`
- [x] Save / Save As / Save All: PNG, JPEG (quality dialog, default 95, over white), BMP (24-bit over white), GIF, TIFF, TGA, WebP; atomic writes; unsaved-changes prompts on close and exit
- [x] Image list: live thumbnails, `*` for unsaved, click, middle-click/× to close, drag to reorder; Ctrl+Tab / Ctrl+PgUp/PgDn / Ctrl+Shift+PgUp/PgDn / Ctrl+1–9 / Alt+1–9
- [x] Title bar `*name - paintnotnet`; status bar with size, cursor position, zoom
- **Verified 2026-10-09:**
  - `cargo xtask ci` passes: 64 tests, including 7 headless UI tests driven by real key events, plus 3 offscreen GPU render tests checking canvas pixels (content, checkerboard values, grid dot pattern)
  - Release build with an 8000×8000 PNG: opened in 243 ms; 164.7–165.0 fps on DP-1 while sweeping zoom 1%–800% and panning (p99 6.5–7.0 ms)

Guessed, to tune by feel (paint.net doesn't document them):
- Space+arrow pan step 16 px (×10 with Ctrl)
- PgUp/PgDn scroll one full viewport
- Ctrl+Home/End put the corner at the centre unclamped; the next scroll clamps it back
- Max zoom is 3200% (from 3.36); paint.net 5 may go higher
- JPEG dialog has no live preview or file-size estimate yet
- Image list and status bar layout are functional, not yet paint.net-styled (Phase 2)

## Phase 2: The paint.net shell ✅
- [x] Compared against paint.net 5's official screenshots (fetched to `reference/pdn5-docs/`); layout, default window positions and sizes measured from them
- [x] Top band as in paint.net 5: menus + toolbar row 1, image thumbnails beside them, Tools/History/Layers/Colors toggles + Settings + Help (?) at the right
- [x] Full menu tree (File, Edit, View, Image, Layers, Adjustments, Effects, Help) with paint.net 3.36 icons, shortcut text, `&` mnemonics; unimplemented items visible but disabled
- [x] Alt+F/E/V/I/L/A/C/H open menus; mnemonic letters pick items (Alt+F, N → New); arrows move between items and menus; underlines in keyboard mode
- [x] Toolbar row 1 (common commands, Pixel Grid / Rulers toggles) and row 2: tool dropdown (Alt+T) and every tool's options (brush size with presets and wheel, hardness/spacing/tolerance % bars, selection modes, draw modes, flood mode, sampling, gradient types/modes/repeat, text, shapes, AA, blend modes incl. Overwrite, Finish)
- [x] Floating windows ported from paint.net's `SnapManager`: 15 px snap (30 to unsnap), 3 px gap, edge anchoring through resizes, snap to each other; F5–F8 toggle, Ctrl+Shift+F5–F8 reset; red ✕ close
- [x] Tools window (2-column grid, tooltips with letters), Layers window (thumbnails, visibility, selection, bottom bar), History window (stub until Phase 3)
- [x] Colors window: swatches, swap/reset, HSV wheel with paint.net's Ctrl/Alt/Shift behaviours, More >> RGB/HSV/hex/alpha, paint.net's 96-colour default palette (moved here from Phase 6)
- [x] Status bar: tool icon + "Name: help", selection size, cursor position, units (px/in/cm), editable zoom %, zoom to window, zoom out/in, log zoom slider
- [x] Rulers (View > Rulers) with 1/2/5 tick steps in the current units and a cursor marker
- [x] Keymap engine: command registry, multi-tap tool cycling (S/M/O with Shift backwards), repeatable keys, held Space
- [x] Keymap conformance test: parses `docs/reference/keybindings.md`; every keyboard chord accounted for and resolving to the documented command; prints the unimplemented list
- [x] paint.net Light/Dark palettes (follows system theme), system UI font (Noto Sans), compact 12 px UI
- [x] Plasma check: KWin's window-drag modifier defaults to **Meta** (`/usr/share/config.kcfg/kwin.kcfg`) and isn't overridden here, so paint.net's Alt+drag reaches the app
- **Verified 2026-10-09:** `cargo xtask ci` passes (95 tests). New UI tests: shell layout at 1202×867 (windows parked in paint.net's corners, inside the workspace, no overlaps), Alt+F→N opens New Image, S/S/Shift+S/O/O/B tool cycling, `]`/Ctrl+`]` widths, X swaps colours, dragging a window snaps it 3 px from the edge, F7/Ctrl+Shift+F7. Rendered frames compared by eye with paint.net's screenshot. Release build: 164.7 fps on DP-1 with the full shell over an 8K image.

Deferred to the phases that need them: mouse chords (Ctrl/Alt + left/right drag) and routing keys to an active tool session (Phases 3–4); paint.net's "translucent windows" option and Open Recent (Phase 11).

## Phase 3: Layers and history ✅
- [x] History as copy-on-write `Document` snapshots (`pnn-core::history`): a step costs only the tiles it changed; byte budget (8 GB) drops the oldest steps. Revisions are globally unique so restored snapshots never confuse caches
- [x] Undo (Ctrl+Z) / Redo (Ctrl+Y); History window lists every step with paint.net's names and icons, current highlighted, undone steps greyed, click to jump
- [x] Layers menu: Add New Layer ("Layer N" above active), Delete (layer below becomes active), Duplicate, Merge Layer Down (top layer's blend + opacity, as `MergeLayerDownFunction`), Toggle Visibility, Import From File (grows canvas if larger), Flip H/V, Rotate 180°, Rotate / Zoom (angle, pan, zoom, tiling, preserve background; live preview), Go to Top/Above/Below/Bottom, Move to Top/Up/Down/Bottom
- [x] Layer Properties (F4 / double-click): name, visible, 14 blend modes, opacity 0–255, live preview, Cancel restores
- [x] Layers window: drag-and-drop reorder, visibility checkbox recorded in history, accessible row labels
- [x] Image menu: Flip H/V (all), Rotate 90° CW/CCW and 180°, Resize (8 resampling modes, % or absolute, keep aspect, gamma correction), Canvas Size (3×3 anchor, fill choice; fill only on the bottom layer, as paint.net), Flatten
- [x] Saving a layered image to a flat format asks to flatten first (paint.net's wording), undoable
- [x] Pencil tool: 1 px aliased, left = primary, right = secondary, blend modes incl. Overwrite, one history step per stroke, every intermediate pointer event drawn, crosshair cursor
- **Verified 2026-10-09:** `cargo xtask ci` passes (127 tests). UI tests drive layer commands, undo/redo, History clicks, Pencil strokes, Ctrl+H×2 and F4 through real events. Acceptance: 500 steps on a 3840×2160 image hold ≤ 1 tile per step (~131 MB worst case vs ~16.5 GB for full copies), undo/redo all 500 in 0.06 s. Render checked by eye.

Guessed or approximated (paint.net doesn't document them):
- Resize kernels: Bicubic = Catmull-Rom, Bicubic (Smooth) = cubic B-spline, Bilinear (Low Quality) = bilinear without prefilter, Adaptive (Sharp) = Lanczos3 shrinking / Catmull-Rom enlarging, Fant = area average. Gamma correction defaults off.
- Duplicate keeps the layer name (3.36); visibility toggles are named "Layer Visibility".
- History disk spill (paint.net keeps history on disk) isn't implemented; the budget drops oldest steps instead.

## Phase 4: Selections ✅
### 4a ✅ Selection core, tools, Edit commands
- [x] Selection = even-odd polygon set (`pnn-core::selection`), as paint.net; combine modes are polygon booleans (`i_overlay`); coverage mask rasterised antialiased or pixelated (`tiny-skia`), cached per tab; selection stored in the document so it undoes
- [x] Rectangle / Ellipse / Lasso Select, Magic Wand (paint.net's `CheckColor` tolerance maths, premultiplied/straight alpha, contiguous scanline fill or global, Layer/Image sampling); pixel-exact stencil→polygon tracing (holes, diagonal touches)
- [x] Chords: Ctrl add, Alt subtract, Ctrl+right xor, Alt+right intersect, Shift square/circle (Shift = global for the wand); Fixed Ratio / Fixed Size; click deselects
- [x] Select All, Deselect (Ctrl+D, Enter), Invert, Erase (Delete), Fill (Backspace / Shift+Backspace secondary; overwrite like paint.net, coverage-blended at AA edges), Crop to Selection (outside cleared), Zoom to Selection; copy/paste selection JSON format implemented in core (`to_json`/`from_json`, matches the docs' example exactly)
- [x] Marching ants along the exact polygon edges (animated); status bar shows selection size; Pencil clipped to the selection
- **Verified 2026-10-09:** CI passes (145 tests). UI tests drive every chord, Magic Wand contiguous vs Shift-global, Delete/Backspace/Ctrl+I/Ctrl+A/Enter, crop + undo, Pencil clipping. Ants checked in a render.
- Differences to confirm: paint.net 4+ lets you adjust a rectangle/ellipse selection with handles after drawing (not yet); 3.36 deselected after Erase Selection, 5.x appears not to (kept).

### 4b ✅ Clipboard
- [x] Cut / Copy / Copy Merged (selection region, alpha × coverage; whole layer if nothing selected), Paste and Paste into New Layer as floating pixels in Move Selected Pixels mode at the top-left of the visible canvas, Paste into New Image (also used when nothing is open); paint.net's "larger than the canvas" prompt (Expand canvas / Keep canvas size)
- [x] Copy Selection / Paste Selection as paint.net's polygonList JSON text
- [x] Desktop clipboard via `arboard` (Wayland data-control, X11 fallback) behind a `Clipboard` trait; tests use an in-memory clipboard
- **Verified:** 5 clipboard UI tests (copy→paste→move→finish, cut, new layer/new image, expand canvas, selection JSON); opt-in `system_clipboard_round_trip` passed against KWin 6.7 (image + text, previous text restored). CI passes (153 tests).

### 4c ✅ Move tools
- [x] Move Selected Pixels / Move Selection: lift (coverage-scaled) pixels as a floating session over a pre-lift snapshot; drag inside to move, 8 nubs to scale in the selection's own frame (Shift keeps ratio, Alt about centre), right-drag or left-drag outside to rotate (Shift 15° steps), Ctrl moves a copy (and stamps the current float first), arrows 1 px / Ctrl+arrows 10 px, nothing selected = whole layer
- [x] Toolbar sampling (Bicubic / Bilinear / Nearest) for transformed pixels; integer moves copy exactly
- [x] History: Move/Scale/Rotate Pixels (Selection) per drag, Finish Pixels (Selection) on Enter, tool change or other commands; Undo/Redo drop the float and let history restore
- **Verified 2026-10-09:** 11 Phase 4 UI tests incl. move + hole, Ctrl copy, nudges, Move Selection, nub scale ×2, Shift right-drag 180°, whole-layer move. CI passes.
- Gap: paint.net draws floating pixels as an overlay; here each change redraws the layer, which is slower for very large selections (optimise if it bites).

## Phase 5: Feather ★
- [ ] `pnn-sdf`: exact EDT, signed field, profiles (smooth/linear/gaussian), rayon
- [ ] Live Feather field on all selection tools; position inside/centered/outside; preview overlay
- [ ] Edit > Feather Selection… (Shift+F6)
- [ ] Effects > Object > Feather Object (radius, profile, threshold, canvas-edge option)
- [ ] Effects > Selection > Blur Selection Edge
- [ ] All selection consumers honour soft masks (paint tools, fill, erase, cut/copy, move pixels, effects)
- **Accept:** 165 Hz slider preview on a 4K image; feather 0 ≡ hard selection (pixel-identical); golden tests (square, thin line, ring) at r = 1, 8, 64; you try it and sign off on the feel.

## Phase 6: Painting and fill tools
- [ ] Paintbrush: size (fractional), hardness, spacing, smoothing, antialiasing, blend mode, fill style, [ ] and Ctrl+[ ]
- [ ] Eraser, Paint Bucket (tolerance, flood mode, sampling, Shift toggle), Gradient (7 types, colour/transparency, 3 repeat modes, editable nubs, Shift 15°)
- [ ] Color Picker (layer/image, sample size, after-click behaviour, Ctrl samples image), Recolor, Clone Stamp (Ctrl+click source)
- [ ] Tool blend modes and the Overwrite mode

## Phase 7: Text, Line/Curve, Shapes
- [ ] Text tool: font, size, style, alignment; editable until Finish/Esc
- [ ] Line/Curve: nubs, caps (`,` `.` `/`), Alt for centre, right-drag rotate
- [ ] Shapes: full shape list, A / Shift+A to cycle, rounded-rect corner, Shift/Alt nub behaviours

## Phase 8: Adjustments and the effect framework
- [ ] Property schema → auto dialogs with live and split preview; Ctrl+F repeats the last effect
- [ ] All 13 adjustments with their shortcuts (Auto-Level, B&W, Brightness/Contrast, Curves, Exposure, Highlights/Shadows, Hue/Sat, Invert Alpha, Invert Colors, Levels, Posterize, Sepia, Temperature/Tint)

## Phase 9: Stock effects
- [ ] Artistic, Blurs, Color, Distort, Noise, Object (Drop Shadow), Photo, Render, Stylize (full list in the inventory)

## Phase 10: BoltBait-class built-ins
- [ ] Ask you which ones you actually used, then build in that order. Likely first: Outline Object/Selection, Inner Shadow, Bevel, Drop Shadow Plus, Switch Alpha↔Gray, Apply Alpha Mask, Transparency, Color Balance

## Phase 11: Files and polish
- [ ] ORA native format (with paint.net blend-mode attributes), autosave and crash recovery
- [ ] Settings dialog, rebindable keymap UI, command palette (Ctrl+K)
- [ ] Packaging: PKGBUILD for CachyOS/Arch, .desktop file, MIME associations, icon
- [ ] Optional: pop-out panels, tablet pressure

## Phase 12: Visual refresh (after functionality)
- [ ] Modernise the UI styling and icons (the user finds the 3.36 icons dated); keep paint.net's layout and behaviour
- [ ] Custom per-tool cursors (paint.net ships its own; egui can't show custom cursor images yet)
