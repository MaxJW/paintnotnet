# Roadmap

Design and rationale: [PLAN.md](PLAN.md). Each phase ends with a usable app and a check-in
where you try it. A phase is only ticked off once its acceptance checks have been run and
shown passing.

**Current state (2026-10-09):** Phases 0–2 are complete, verified and committed.
**Next: Phase 3.**

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

## Phase 3: Layers and history
- [ ] Layers menu: add, delete, duplicate, merge down, toggle visibility, import from file, flip H/V, rotate 180, Rotate/Zoom dialog, go to top/above/below/bottom, move to top/up/down/bottom
- [ ] Layer Properties (F4 or double-click): name, visible, 14 blend modes, opacity 0–255, live preview
- [ ] Layers window: thumbnails, visibility checkboxes, drag reorder, Ctrl+click on the arrows moves to top/bottom
- [ ] History with tile copy-on-write, byte budget plus disk spill; History window with click-to-jump
- [ ] Image menu: crop to selection, resize (8 resampling modes), canvas size (anchor and fill), flips, rotates, flatten
- [ ] Pencil tool (simplest tool, so layers can be exercised)
- **Accept:** golden tests for all 14 blend modes; undo/redo across 500 steps on a 4K image stays within memory budget.

## Phase 4: Selections
- [ ] Rectangle, Ellipse, Lasso, Magic Wand (tolerance, contiguous/global, layer/image sampling, premultiplied/straight alpha mode, draggable click point)
- [ ] Modes Replace/Add/Subtract/Intersect/Invert plus Ctrl/Alt and left/right mouse chords; left+right to move a selection while drawing it
- [ ] Rectangle select Normal/Fixed Size/Fixed Ratio; Shift for square/circle
- [ ] Antialiased vs pixelated selection clipping; marching ants
- [ ] Edit menu: erase, fill, invert, select all, deselect, cut/copy/paste (+ merged, new layer, new image), copy/paste selection as JSON
- [ ] Wayland clipboard images
- [ ] Move Selected Pixels / Move Selection: nubs, Shift/Alt resize, rotate (right-drag, Shift for 15°), Ctrl to copy, arrow nudges, bilinear/nearest
- **Accept:** UI tests for every selection chord in keybindings.md; Mask pipeline used by fill, erase, copy and move.

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
