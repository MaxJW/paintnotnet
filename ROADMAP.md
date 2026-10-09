# Roadmap

Design and rationale: [PLAN.md](PLAN.md). Each phase ends with a usable app and a check-in
where you try it. A phase is only ticked off once its acceptance checks have been run and
shown passing.

**Current state (2026-10-09):** Phase 0 is complete and verified. Nothing is committed yet.
**Next: Phase 1.**

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

## Phase 1: Document core and canvas
- [ ] Tiled surface (256², Arc copy-on-write, sparse), Layer, Document, `Mask` type
- [ ] CPU compositor (Normal blend), dirty-tile tracking
- [ ] wgpu canvas: tile textures, zoom (nearest ≥100%, mipmapped <100%), checkerboard, pixel grid ≥200%
- [ ] Every canvas and view keybind: scroll, Shift-scroll, Home/End, Ctrl+wheel, Ctrl +/-/0, Ctrl+B (×2 toggles back), Ctrl+Shift+B, Space-drag, middle-drag, Space+arrows
- [ ] New Image dialog, Open, Save, Save As for PNG/JPEG/WebP/BMP/TIFF/GIF/TGA
- [ ] Multiple images: image list with live thumbnails, Ctrl+Tab / Ctrl+PgUp/PgDn / Ctrl+Shift+PgUp/PgDn / Ctrl+N to switch tabs
- **Accept:** an 8000×8000 PNG opens; pan and zoom stay smooth (frame-time overlay); golden tests for compositing.

## Phase 2: The paint.net shell
- [ ] Compare against the official MainWindow and Toolbar screenshots and settle default positions
- [ ] Menu bar with every paint.net menu item; unimplemented items disabled but visible, with their shortcuts shown
- [ ] Alt mnemonics (Alt+F, E, V, I, L, A, C, H, X; Alt+T tool dropdown)
- [ ] Toolbar row 1 (common commands, grid/rulers) and row 2 (tool dropdown + per-tool options host)
- [ ] Floating Tools/History/Layers/Colors windows: drag, snap, F5–F8 toggle, Ctrl+Shift+F5–F8 reset
- [ ] Status bar: hint, progress, size, cursor position, units, zoom slider and box
- [ ] Keymap engine: command registry, multi-tap cycling, held keys, mouse chords, context routing
- [ ] Keymap conformance test against `docs/reference/keybindings.md`
- [ ] Dark theme tuned to paint.net 5; check Plasma's Alt/Meta window-drag modifier
- **Accept:** side-by-side screenshot compare looks right to you; conformance test lists exactly which commands are still unimplemented.

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
- [ ] Colors window: wheel with Ctrl/Alt/Shift behaviours, primary/secondary, X swap, C switch, palette, hex/RGB/HSV, alpha
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
