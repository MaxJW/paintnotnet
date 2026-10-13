# Roadmap

Design and rationale: [PLAN.md](PLAN.md). Each phase ends with a usable app and a check-in
where you try it. A phase is only ticked off once its acceptance checks have been run and
shown passing.

**Current state (2026-10-13):** Phases 0–9 and the first batch of 10 are complete, verified and committed (Phase 5
still awaits your sign-off on the feel). Phase 9 (stock effects) is done except Sketch Blur, and Phase 10's first batch of
BoltBait-class effects is in. Phases 11a (OpenRaster, autosave, crash recovery) and 11b (settings, rebindable keys, command palette) are done.
Phase 11c (packaging) is done. **Next:** your call on the optional 11c items (pen pressure, pop-out windows), then more of the BoltBait pack in
whatever order you rank it. After all phases, compare against the paint.net 5.1.12 release files the user provided
(`~/Downloads/paint.net.5.1.12.portable.x64`; the user states the owners gave permission).
Functionality first: the user wants every feature working
before the visual refresh (Phase 12).

Reference source: paint.net 3.36.7 (OpenPDN, MIT code) is cloned at `reference/OpenPDN`
(git-ignored; re-clone with `git clone --depth 1 https://github.com/rivy/OpenPDN reference/OpenPDN`).
Port behaviour from it. Pinta (`reference/Pinta`, MIT) is a secondary reference only after checking it against
OpenPDN or the 5.x docs; never decompile 4.x/5.x. See PLAN.md §2.

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

## Phase 5: Feather ★ ✅ (awaiting your sign-off on the feel)
- [x] `pnn-sdf`: exact EDT (Felzenszwalb–Huttenlocher), signed field from coverage, profiles (smooth/linear/gaussian), rayon
- [x] Live Feather field on all selection tools (toolbar radius/profile/position, applies to the current selection too); position inside/centered/outside; red quick-mask preview overlay (toolbar Preview toggle, and while a feather dialog is open); ants on the 50% contour
- [x] Edit > Feather Selection… (Shift+F6)
- [x] Effects > Object > Feather Object (radius, profile, threshold, canvas-edge option)
- [x] Effects > Selection > Blur Selection Edge (fades alpha inside the edge; the selection stays hard)
- [x] All current selection consumers honour soft masks: Pencil, Fill/Erase Selection, Crop to Selection, Cut/Copy, Move Selected Pixels, dialogs' effects. Phase 6+ tools use the same `selection_mask`.
- Speed: the distance field is computed once per shape (~73 ms for a 3600×2000 ellipse on 4K) and cached; a slider step reshapes in ~1.2 ms (tiles wholly inside or outside the soft band are settled from per-tile distance ranges and share one full tile, so GPU uploads skip them); the ant outline offset (~16 ms) runs on a worker thread while the slider moves.
- **Verified 2026-10-13:** feather slider smoke test on the real display (KWin Wayland, RTX 3070, 3840×2160 image, 240 frames): **160.3 fps, mean 6.24 ms, p99 11.6 ms** against the 165 Hz vsync (6.06 ms); a plain smoke test runs at 164.6 fps, so the slider costs ~0.2 ms/frame on average with occasional 11 ms frames. Feather 0 is pixel-identical to a hard selection; golden square/thin-line/ring tests at r = 1, 8, 64; tile skipping is pixel-identical to per-pixel shaping for every profile × position. 6 feather UI tests (Shift+F6 → soft Delete, toolbar feather, moving feathered pixels, Feather Object, Blur Selection Edge, undo); preview render checked. CI passes.
- Guessed (no 3.36 source, 5.x docs give no maths): profile curves, Feather Object's threshold semantics, the overlay tint colour. Adjust after you try it.

## Phase 6: Painting and fill tools ✅
### 6a ✅ Paintbrush, Eraser, blend modes
- [x] Paintbrush (`pnn-tools::brush`): size (fractional), hardness, spacing, smoothing, antialiasing, blend mode, fill style (Solid + 6 hatches), [ ] and Ctrl+[ ]; left = primary, right = secondary; brush outline at the pointer; one history step per stroke, committed on tool change too
- [x] Eraser (alpha × (1 − coverage), colour kept, as 3.36's `EraserTool`)
- [x] Tool blend modes and the Overwrite mode (coverage-weighted replace)
- [x] Strokes honour the (soft) selection; a stroke never builds up over itself
- **Verified 2026-10-13:** 13 engine tests (dab profile, aliased symmetry, continuity, no build-up, spacing carry, smoothing through samples, erase/overwrite, hatches) and 6 UI tests (stroke + undo, secondary + no build-up, selection clip, Eraser, Overwrite, tool switch mid-stroke). Render checked. Real display, 3840×2160 image, a pointer move per frame (`PNN_SMOKE_BRUSH=<width>`): 164.8 fps at widths 20, 200 and 800 (p99 ≤ 6.5 ms). CI passes (189 tests).
- Guessed (5.x doesn't publish them): the hardness falloff (smoothstep from r·h − ½ to r + ½), sweeping between dabs up to 50% spacing, Catmull-Rom smoothing one sample behind, the hatch set and its 8×8 phase (3.36 had GDI+'s 53 hatches). Pressure is not supported (no tablet).
### 6b ✅ Paint Bucket and Gradient
- [x] Paint Bucket (3.36 `PaintBucketTool`/`FloodToolBase`): Magic Wand tolerance test, contiguous/global with Shift flipping it, flood limited to the selection and ignored outside it, Sampling Layer/Image, fill style, blend mode/Overwrite, right button swaps colours
- [x] Gradient (3.36 `GradientRenderer(s)` maths ported): Linear, Linear (Reflected), Diamond, Radial, Conical, Spiral ↻/↺; Color/Transparency mode; No Repeat / Repeat Wrapped / Repeat Reflected; blend mode/Overwrite; nubs stay editable (drag either, right-click a nub swaps colours), Shift 15°, right-drag draws swapped; Enter, Esc, toolbar Finish, a new gradient, tool or document change commit; Undo takes back a live gradient; re-renders when colours/options change
- **Verified 2026-10-13:** 7 gradient engine tests (each type, repeat modes, spirals, `ColorBgra.Blend` table, render modes, 15° snap); 8 UI tests (bucket contiguous/Shift-global, selection limit, gradient draw → nub edit → Enter, Esc + Undo, new gradient commits + right-drag swap + tool change, transparency mode, Shift snap, toolbar Finish). Render checked. CI passes (205 tests).
- Speed: a live gradient re-renders the whole layer once per frame. Real display: 1080p image 120 fps (linear) / 88 fps (conical); **4K image only ~30 fps**: gradient ~9.5 ms, display-tile conversion ~5 ms and GPU upload 6–10 ms per frame for a whole-image change. Same cost will hit live adjustment previews (Phase 8), so fix it there for all of them (render/convert only visible tiles first, or a GPU preview).
- Differences/guesses: Esc commits (5.x docs) where 3.36 cancelled; spiral and repeat maths are paintnotnet's (5.x unpublished); Paint Bucket antialiasing has no visible effect (3.36 filled a pixel region; 5.x's AA fill is unknown).
### 6c ✅ Color Picker, Recolor, Clone Stamp
- [x] Color Picker (3.36 `ColorPickerTool`): left → primary, right → secondary, also while dragging; Sampling Layer/Image, 1×1…11×11 squares (premultiplied average), Ctrl samples the image; after-click: stay / previous tool / Pencil
- [x] Recolor (3.36 `RecoloringTool`): pixels within tolerance (`ColorDifference`, `Tolerance × 256`, capped at the colours' difference) of the secondary get its RGB shift to the primary (right button: the reverse), alpha kept, coverage-weighted at soft edges; Ctrl+click picks (keeps alpha); Shift+drag picks and moves the other colour by the same step
- [x] Clone Stamp (3.36 `CloneStampTool`): Ctrl+click sets source point and layer; offset fixed by the first stroke and kept (aligned); copies from the pre-stroke source; source marker pulses
- **Verified 2026-10-13:** 5 UI tests (picker primary/secondary, 3×3 average, layer vs Ctrl image sampling + switch to previous tool, Recolor stroke + tolerance + Ctrl pick, Clone Stamp no-source/clone/aligned second stroke) and a Recolor engine test. CI passes (211 tests). The clone-source marker drawing was not checked in a render.
- Differences: 3.36's Clone Stamp scaled opacity by the primary colour's alpha⁴ (not done; 5.x unknown); Recolor and Clone Stamp use the brush engine (soft dabs, no build-up) instead of 3.36's per-dab blending.

## Phase 7: Text, Line/Curve, Shapes ✅
### 7a ✅ Line/Curve and the editable-drawing engine
- [x] `pnn-tools::vector`: Straight / Cubic Spline (GDI+ `DrawCurve` cardinal spline, tension 0.5) / Bézier through four nubs; caps Flat, Arrow, Arrow (Filled), Rounded (3.36 `LineCap2`; arrows are GDI+ `AdjustableArrowCap(5, 5)` in pen widths); GDI+ dash styles; fill styles; tiny-skia antialiasing; parts composed into a shape layer and blended once (blend modes, Overwrite), clipped to the soft selection
- [x] Line/Curve tool: drag (left primary, right secondary; Shift 15°, Alt from centre); live afterwards: drag nubs (either button), move icon, right-drag rotate (Shift 15°), arrows 1 px / Ctrl 10 px; `,` `.` `/` cycle start cap / dash / end cap and restyle the live line; toolbar curve type, style and fill; commits on Enter/Esc/Finish/new line/tool change/other commands; Undo takes back a live line
- **Verified 2026-10-13:** 5 engine tests (curve types through ends, spline through nubs vs Bézier, width/flat ends, round and arrow caps, dashes, Overwrite vs blend) and 5 UI tests (live until Enter, nub bend + move icon + arrow nudges, Shift snap + new line commits, right-drag rotation, `.` restyle + Undo). Render checked. CI passes (222 tests).
- Guessed: default curve type (Spline), move-icon placement (16 screen px past the end nub), the tangent used for arrow heads. Antialiasing is analytic, not paint.net 5's 2×/4× supersampling.
### 7b ✅ Shapes
- [x] The 29 presets in the 5.x docs' order (`pnn-tools::shapes`), shape selector, A / Shift+A pick the next/previous shape; draw modes outline / filled / filled with outline (fill in the other colour); Corner Size for the rounded shapes; dash style and fill style; Shift square / Alt from centre while drawing
- [x] Live editing: corner nubs (opposite corner anchors, Shift keeps aspect, Alt about centre, drag past the opposite corner flips), drag inside or the move icon to move, rotation point (drag it; starts at the centre), right-drag rotates about it, left-drag just outside rotates too, Shift 15°, arrows 1 / 10 px; commits like Line/Curve
- **Verified 2026-10-13:** 2 geometry tests (every shape builds and fills its box; cycling wraps) and 6 UI tests (draw modes re-render live + Enter; Shift square + corner resize; move inside + Ctrl+arrow; right-drag rotate with 15° snap; A picks the next shape + Undo; pressing away commits). All 29 rendered and checked by eye. CI passes (231 tests).
- Guessed: every shape's exact outline (5.x geometry unpublished; check against the release files later), the cloud is drawn smaller than its box, four corner nubs (docs say "corners"), the history name "Shapes", default Corner Size 20. Not done: Ctrl+click corner −/+ radius (unclear UI).
### 7c ✅ Text
- [x] `pnn-tools::text` on cosmic-text (system fonts via fontdb, shaping, swash rasterisation, font fallback): font family list, size in points at the image DPI or Fixed (96 DPI), bold, italic, underline, strikeout, left/centre/right from the click point, antialiasing (off = aliased), primary colour, blend mode/Overwrite, clipped to the selection
- [x] Text tool: click places the caret; typing, Enter (new line), Backspace, Delete, arrows, Home/End; plain keys go to the text, so letters don't switch tools (Ctrl/Alt chords still reach the keymap); blinking caret, pulsing move nub drags the text; toolbar changes restyle the live text; Esc/Finish/click elsewhere/tool change/other commands commit; Ctrl+Z takes back live text
- **Verified 2026-10-13:** 3 engine tests (anchor and alignment, bold/underline/aliased, caret across lines + size maths) and 4 UI tests (typing with 'e' not switching tools + Esc commit, Enter/Backspace/bold restyle, Ctrl+Z + click-elsewhere commit, move nub). Render checked (bold italic underlined centred two lines with Unicode). CI passes (239 tests).
- Guessed/different: line height 1.2 × size, underline/strikeout position and thickness; the 5.x docs' size formula ("points × DPI / 96") reads oddly, so standard pt × DPI / 72 is used (check later); 5.x's Smooth/Sharp rendering modes (DirectWrite) aren't offered; no mouse text selection or Ctrl+C/V within text; the caret is black rather than inverted.

## Phase 8: Adjustments and the effect framework ✅
### 8a ✅ Effect framework
- [x] `pnn-effects::framework`: typed properties (int, float, bool, choice; linked values as 3.36's `LinkValuesBasedOnBooleanRule`), a renderer prepared once per settings and rendering rows; pixel-op and channel-curve helpers; 3.36 `GetIntensityByte`
- [x] Generated dialogs (slider + number + reset per property) with a live preview rendered tile by tile on worker threads and streamed onto the canvas (cancelled when settings change); OK = one history step named after the effect (rendered synchronously if the preview hadn't finished), Cancel/Esc restores, Enter = OK; soft selections mix by coverage; Effects > Repeat (Ctrl+F) re-applies the last effect with its settings and the menu shows its name
- [x] First adjustments on it: Invert Colors, Invert Alpha, Black and White, Sepia (3.36 Desaturate + `Level` γ 1.2/1.0/0.8), Posterize (3.36 tables, linked R/G/B)
- **Verified 2026-10-13:** 3 engine tests (pixel results, Posterize tables, `Level` formula) and 4 UI tests (Ctrl+Shift+I + Ctrl+F, selection clip, Posterize live preview then OK with changed values, Cancel/Esc/Enter). Real display, Posterize levels changing every frame (`PNN_SMOKE_EFFECT=1`): 1080p 164.8 fps, 4K 123 fps (p99 22 ms). CI passes (246 tests).
- Not done: paint.net 5's split/before-after preview (no doc detail found); the gradient (6b) still renders on the UI thread and could move to this tile pipeline.
### 8b ✅ Simple adjustments
- [x] Ported from 3.36: Auto-Level (`HistogramRgb.MakeLevelsAuto`: 0.5%/99.5% percentiles and mean → `Level.AutoFromLoMdHi`), Brightness / Contrast (the 65536-entry intensity × channel table, contrast 100 thresholds), Hue / Saturation (3.36 algorithm incl. the 101–200 → 103–400 saturation mapping and `BlendConstant` lightness)
- [x] From the 5.x docs (maths guessed): Sepia's Intensity control (5.x gives Sepia a dialog), Exposure (±5 EV gain in linear light), Highlights / Shadows (luminance tone curve), Temperature and Tint (linear-light channel gains)
- **Verified 2026-10-13:** 7 more engine tests (Brightness/Contrast against 3.36 values, Hue/Sat rotation/desaturate/lightness, Auto-Level stretch, photo adjustments identity at defaults and direction) and 1 UI test (Hue/Sat dialog OK, Auto-Level). Dialog with live preview checked in a render. CI passes (252 tests).
- Differences: Hue / Saturation doesn't round HSV to whole percents as 3.36 did (that banded photos); the 5.x docs' description of Saturation "starting at zero" conflicts with their own example, so 3.36's 0–200 (default 100) is kept; Auto-Level's histogram covers the selection's bounding box, not just selected pixels; the 5.x-only adjustments have no shortcuts (none documented).
### 8c ✅ Curves and Levels
- [x] Curves (Ctrl+Shift+M): 3.36 `SplineInterpolator` tables; Luminosity (`LuminosityCurve`) or RGB (`ChannelCurve`) with Red/Green/Blue checkboxes choosing which curves an edit moves; 256×256 graph with grid and diagonal; drag to add or move a point, right-click removes one, grab radius √30 as 3.36; input/output readout; Reset
- [x] Levels (Ctrl+L): 3.36 `Level` maths; input white/black with the input histogram, output white/gamma/black with the live output histogram; R/G/B checkboxes (edits apply to checked channels); Auto (= Auto-Level's settings) and Reset
- [x] Both use the effect framework's background live preview and Repeat (Ctrl+F repeats with the same curve/levels)
- **Verified 2026-10-13:** 4 engine tests (spline through points and 3.36 identity/inversion, point editing, luminosity vs RGB, Levels + Auto + histogram mapping) and 2 UI tests (a real mouse drag on the curve graph previews live, right-click removes, Enter commits; Levels Auto previews, OK commits, Ctrl+F repeats). Both dialogs checked in renders. CI passes (263 tests).
- Differences: Curves end points move only vertically; Levels' colour swatches and gradient sliders are number boxes here, and the output grey point is shown as gamma.

### Requested along the way (2026-10-13) ✅
- [x] Effect and document dialogs (Feather Selection/Object, Blur Selection Edge, adjustments, Rotate/Zoom, Layer Properties…) are movable windows: they open centred, can be dragged aside, and remember where they were left. The canvas behind them previews live, from the moment they open (defaults shown at once, as paint.net does) and on every change; Cancel/Esc restores the layer.
- [x] WebP: open (any WebP, via `image`) and save through a Save Configuration dialog with pdn-webp's settings (Preset, Quality, Effort 0–9, Lossless; paint.net 5 bundles pdn-webp, MIT, attributed in NOTICE) configuring libwebp as pdn-webp does. JPEG's quality dialog became the same Save Configuration dialog.
- **Verified:** dialog drag test (moves with the title bar, still usable), Feather Object previews on open, changes live, Cancel reverts with no history step; WebP engine test (lossless exact incl. alpha, lossy close and smaller) and a UI test (Ctrl+S on a .webp → settings → Lossless → OK → file loads back pixel-exact). CI passes (256 tests).
- Not done: WebP metadata (EXIF/ICC/XMP) and animation, which pdn-webp keeps; document-dialog effects (Feather Object etc.) still render on the UI thread, so very large images preview slower than the adjustments do.

## Phase 9: Stock effects ✅ (except Sketch Blur)
### 9a ✅ Blurs
- [x] Effects menu items run through the effect framework (live tiled preview, movable dialog, OK/Cancel, Repeat); renderers can now render whole rectangles (`render_rect`) to share work across rows; history entries use 3.36's effect icons
- [x] Ported from 3.36: Gaussian (its tent kernel, alpha-weighted, computed with running sums: radius 200 on 4K in ~0.15 s), Motion, Radial, Zoom (fixed-point maths as 3.36), Fragment, Median (disc histogram percentile), Surface (3.36 intensity weights); 5.x-only with guessed maths: Square (alpha-weighted box), Bokeh (disc average)
- **Verified 2026-10-13:** 3 engine tests (Gaussian vs a brute-force port of 3.36's O(r²) loop within ±1 at r = 1, 3, 7; every blur keeps flat areas; median/surface keep edges) and 2 UI tests (Gaussian from the menu previews, OK, Ctrl+F; every blur opens and Cancel restores exactly). CI passes (268 tests).
- Differences/gaps: Median fixes 3.36's off-by-one percentile (3.36 brightened flat areas by one level); Radial/Zoom centres are two Offset numbers, not 3.36's pan control; Sketch Blur not implemented (no published maths); slow at extremes on 4K (Motion distance 200 ≈ 6 s, Radial ≈ 1.3 s) though the preview streams in without blocking.
### 9b ✅ Distort, Noise, Stylize, Artistic (+ Glow)
- [x] Ported from 3.36: Bulge, Dents (`WarpEffectBase` + `PerlinNoise2D`), Frosted Glass, Pixelate, Polar Inversion, Tile Reflection, Twist (RGSS supersampling, clamped/wrapped/reflected bilinear sampling); Add Noise (3.36 normal lookup), Reduce Noise; Edge Detect, Relief, Emboss (3.36 angle kernels), Outline; Ink Sketch (BoltBait's, MIT), Oil Painting, Pencil Sketch; Glow (Photo, needed by Ink Sketch)
- [x] 5.x-only, maths guessed: Crystalize (jittered Voronoi cells), Morphology (dilate/erode over a square)
- **Verified 2026-10-13:** 6 engine tests (3.36 RGSS offsets, Perlin determinism, every distortion runs + neutral settings keep the image, Pixelate cells, Morphology dilate/erode, Emboss/Relief/Add Noise/Oil Painting known values) and the UI test now opens and cancels every implemented effect (28). A contact sheet of all effects at their defaults was checked by eye. CI passes (275 tests).
- Differences: randomness (Add Noise, Frosted Glass, Dents, Crystalize) is a position hash with a Seed setting instead of 3.36's clock-seeded RNG, so previews match results; Edge Detect is 3.36's angle kernel, not 5.x's Prewitt/Sobel; Outline keeps 3.36's quirk of skipping empty blue bins for alpha.
### 9c ✅ Photo, Render, Object, Color
- [x] Ported from 3.36: Sharpen, Red Eye Removal, Soften Portrait (BoltBait's, MIT), Vignette (linear light, `SrgbUtility`), Clouds (its Perlin cells + blend mode), Julia Fractal, Mandelbrot Fractal
- [x] 5.x-only, maths guessed: Straighten (rotate ±45°, zoomed to fill), Turbulence (absolute-value cloud octaves), Drop Shadow (blurred offset alpha in the primary colour under the object; radius, distance, angle, opacity, only-shadow), Quantize (median cut, 2–256 colours, ordered dithering 0–8)
- **Verified 2026-10-13:** 4 engine tests (all run; drop shadow falls down-right under the object; Quantize limits colours, Straighten 0° is identity; Clouds seeded and spanning both colours); the UI test opens/cancels every effect (39 now); contact sheet checked. CI passes (279 tests).
- Differences: Drop Shadow's colour is the primary colour (the framework has no colour property yet); Quantize has no Octree option and dithers with a 4×4 Bayer matrix, not error diffusion; Sharpen fixes the same off-by-one percentile as Median.

## Phase 10: BoltBait-class built-ins (first batch ✅)
- [x] Built the roadmap's "likely first" list without waiting for your ranking (tell me which others you used): Outline Object, Outline Selection, Inner Shadow (Object and Selection), Bevel Object, Bevel Selection, Drop Shadow Plus, Switch Alpha to Gray, Switch Gray to Alpha (Effects > Object/Selection), Transparency and Color Balance (end of the Adjustments menu, after a separator, as plugin adjustments appear)
- [x] Shapes come from exact signed distance fields (the feather engine) of the object (alpha ≥ threshold) or the selection, so widths and softness are true pixels; the framework now passes the selection to effects
- **Verified 2026-10-13:** 4 engine tests (outline band outside the object; inner shadow and bevel stay inside and light the right rims; selection variants follow the selection; drop shadow plus, alpha↔gray, transparency, colour balance values) and the UI test opens/cancels all of them; an object/selection contact sheet was checked by eye (bevel normals smoothed with a 5×5 stencil after the first render showed striping). CI passes (283 tests).
- All of these are paintnotnet's own designs: BoltBait's pack isn't open source and its settings aren't published, so settings, defaults and looks are guesses to compare with the real plugins later.
- [ ] Still to do from the pack: Apply Alpha Mask (needs the clipboard image), Paste Alpha, Black and White+, Combined Adjustments, Hue/Sat+, Dream, Oil Paint+, Pastel, Gaussian Blur+, Photo Adjustments, Vignette+, Remove Dust, Level Horizon, Meme Maker, Calendar/Chart/Grid/Polygon renders, Text Fun Factory, and the rest of the 44 (inventory has the list)

## Phase 11: Files and polish
### 11a ✅ OpenRaster, autosave, crash recovery
- [x] OpenRaster (`.ora`) as the layered format (first in Save As): OpenRaster 0.0.6 layout (uncompressed `mimetype` first, `stack.xml` top-first, layer PNGs, `mergedimage.png`, 256 px thumbnail); name, opacity, visibility, blend mode (SVG composite-ops, `pdn:reflect/glow/negation/xor` for paint.net-only modes), active layer, DPI; reads other programs' files including offset/cropped layers and nested stacks; Open detects it by content; layered saves skip the flatten prompt
- [x] Autosave every 60 s of changed documents to `~/.local/state/paintnotnet/recovery/<pid>/` (ORA, written atomically on a worker thread); saving/closing removes a document's file, a clean exit removes the folder; at startup a dead session's images are offered back (Recover opens them unsaved, with their original paths; Discard deletes them)
- **Verified 2026-10-13:** ORA round trip (layers, names with XML specials, opacity, hidden, Glow, active layer, DPI, mimetype first), every blend mode maps both ways; **Krita** opened our file and exported a composite within 1 level of ours (Multiply at 78%), and our reader loaded Krita's re-saved ORA (cropped layer at offset 16,12, Multiply, opacity); UI tests: Ctrl+S on a layered .ora saves without flattening and reloads; autosave writes a changed document; a crashed session's image is offered back and recovered unsaved under its original name. CI passes (290 tests).
- Not checked: a real crash on the display (the dead session is simulated in tests). Autosave interval isn't configurable yet (Settings, 11b).
### 11b ✅ Settings, rebindable keys, command palette
- [x] Settings (Ctrl+Shift+X, paint.net's shortcut) with General (appearance: follow system/dark/light; autosave interval in minutes, 0 = off) and Keyboard tabs, saved to `~/.config/paintnotnet/settings.toml`
- [x] Keyboard tab: searchable list of every command with its shortcuts; Change captures the next chord, Clear unbinds, Reset restores the built-in; a chord already in use moves to the new command (with a note saying which lost it)
- [x] Command palette (Ctrl+K): type to filter every menu and shortcut command, Up/Down/Enter to run, Esc to close
- **Verified 2026-10-13:** UI tests rebind Invert Colors to Ctrl+J through the real dialog (then Ctrl+J inverts and Ctrl+Shift+I does nothing), switch appearance to Light, and run Gaussian Blur from the palette; renders of Settings › Keyboard and the palette checked by eye. CI passes (295 tests).
- The shortcut editor and the palette are paintnotnet additions (paint.net 5.x has neither); paint.net's Settings has more pages (tools, updates, plugins) that aren't copied.
### Requested tweaks (2026-10-13) ✅
- [x] Panning past the image edge: when the image is bigger than the window, it can be scrolled until its edge reaches the middle of the canvas (paint.net stops at the edge); scrollbars cover the extra room; Home×2/End×2 still line the corner up with the window corner
- [x] Mouse wheel zooms about the pointer by default (Shift+wheel and touchpad scrolling still scroll, Ctrl+wheel still zooms); Settings › General › "Mouse wheel over the image" switches back to paint.net's scroll behaviour
- [x] Menu bar: with one menu open, hovering another title switches to it without a click
- [x] KDE Plasma global menu: the menus are exported as `com.canonical.dbusmenu` (labels, mnemonics, icons, shortcuts, check marks, live enabled state, Repeat label) and linked to the window with KWin's `org_kde_kwin_appmenu` Wayland protocol; used only while Plasma's menu registrar runs, and the in-window bar hides meanwhile. Settings › General › Menus turns it off. While a dialog is open the whole menu is greyed out.
- **Verified 2026-10-13:** UI tests for wheel zoom about the pointer, Shift+wheel, the Scroll setting, middle-drag past the edge (stops with the corner mid-canvas and stays), and hover-switching File → Edit (fails without the change); global-menu unit tests (labels, tree, shortcuts, `(ia{sv}av)` layout, depth). **On the real Plasma 6.7.5 Wayland desktop:** the panel's global menu showed File … Help with the in-window bar hidden; a D-Bus `clicked` event on File › New… opened the New Image dialog; with the setting off the bar came back and the panel showed nothing. CI passes (303 tests).
- Not checked: opening the dropdowns with the mouse and how Plasma draws our shortcuts and icons in them (no way to click the panel from here); toggling the setting while running; X11 sessions (unsupported: the window keeps its own menu bar there). Two `unsafe` calls (joining winit's Wayland connection) live in `global_menu/wayland.rs`, the only unsafe code in the project.
### 11c ✅ Packaging
- [x] `packaging/arch/PKGBUILD` (`paintnotnet-git`): builds the committed HEAD of this checkout (`cd packaging/arch && makepkg -si`); installs the binary, `paintnotnet.desktop`, the icon (SVG + 16–512 px PNGs in hicolor), and LICENSE, NOTICE and the 3.36 icon attribution under `/usr/share/licenses/paintnotnet-git/`. `options=('!lto')`: makepkg's LTO (on in CachyOS's makepkg.conf) turns the bundled libwebp into GCC bitcode that Rust's linker can't read.
- [x] `.desktop` file named after the Wayland app_id `paintnotnet`, so KWin matches the window to it; opens files with `%F`; MimeType lists the formats Open reads: OpenRaster, PNG, JPEG, BMP, GIF, TIFF, TGA, WebP (all already defined by shared-mime-info, so no MIME XML is needed)
- [x] App icon: original artwork (canvas tile, transparency checkerboard, gradient brush stroke), `packaging/paintnotnet.svg`; also the X11 window icon. Not derived from paint.net's logo.
- [x] `LICENSE` (MIT, as Cargo.toml already declared) added at the root
- **Verified 2026-10-13:** `desktop-file-validate` passes; `makepkg -C` builds `paintnotnet-git-0.1.0.r27` (first try failed with undefined `WebP*` symbols → `!lto`); package contents and .PKGINFO checked; the packaged binary links only libc/libm/libgcc_s (Wayland, Vulkan and xkbcommon are loaded at run time) and its smoke test ran on Wayland/Vulkan at 164.8 fps.
- Not checked: installing it (needs `sudo pacman -U`), the launcher entry, the icon in the title bar and task manager, and "Open with" from Dolphin. A second file opened from Dolphin starts a second window (paint.net opens it as a tab in the running one; not done).
- [ ] Optional: pop-out panels, tablet pressure

## Phase 12: Visual refresh (after functionality)
- [ ] Modernise the UI styling and icons (the user finds the 3.36 icons dated); keep paint.net's layout and behaviour
- [ ] Custom per-tool cursors (paint.net ships its own; egui can't show custom cursor images yet)
