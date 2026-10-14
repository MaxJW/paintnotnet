# Roadmap

Design and rationale: [PLAN.md](PLAN.md). Each phase ends with a usable app and a check-in
where you try it. A phase is only ticked off once its acceptance checks have been run and
shown passing.

**Current state (2026-10-13): the recreation is finished and saved.** Phases 0–11 are done, verified and committed;
the git branch `recreation` and tag `recreation-2026-10-13` keep that faithful paint.net 5.x recreation as it stands
(Phase 5's feather feel still awaits your sign-off). From here `main` is **paintnotnet modern**: the user's own
modernised paint.net, "keeping things vaguely in place" but with docked side panels instead of draggable windows, a
glassy, great-looking UI, better tool controls (brush size first), super smooth performance, extra features, and better
maths wherever something we copied (paint.net 3.36, BoltBait) can be done more accurately or faster. Phases 13
(design system and docked shell) and 14 (tool controls) are done (2026-10-14). **Next:** Phase 15 (better maths,
accuracy and speed). The recreation backlog (rest of the
BoltBait pack, Sketch Blur, comparing against the 5.1.12 release files) is parked below and done on the `recreation`
branch only if wanted.

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

### 10b ✅ Built in, not plugins: the user's favourites matched to BoltBait's v6.0 dialogs
The user asked (2026-10-13) for these not to feel like plugins: one consistent, modern feel. So pack effects that extend a stock one are options of the stock effect, extras sit among the stock entries (Adjustments menu alphabetical, no plugin separator), and every dialog is generated by the same framework.
- [x] Framework: colour wells (primary/secondary/fixed defaults, optional alpha), angle dial (0° right, counter-clockwise), layer picker, collapsible option sections, effects that draw outside the selection (`unclipped`), missing values take defaults
- [x] Gaussian Blur: "Channels and blending" section (red/green/blue/alpha, blend mode) = BoltBait's Gaussian Blur+; defaults give the plain blur exactly
- [x] Hue / Saturation: "Pixel range" section (hue from/to with wrap, include grays, saturation and lightness ranges) = Hue / Saturation+
- [x] Drop Shadow: colour well (Drop Shadow Plus removed as a separate entry)
- [x] Outline Object: Width 10, Color, Antialiasing, Blur radius (v6.0 dialog)
- [x] Outline Selection: Width, Color with opacity, Location (inside/on/outside), Corners (miter/round/bevel), Line style (solid, dash, dot, dash dot, dash dot dot), Antialiasing; strokes the selection's real outline with tiny-skia and may draw outside the selection
- [x] Bevel Object / Selection: angle dial, Depth, Strength, Hard edges, Add drop shadow, Colors section (highlight, shadow)
- [x] Inner Shadow: angle dial and colour well; Switch Alpha to Gray: Mask of layer, Invert mask; new Apply Alpha Mask (mask layer, gray or transparency, replace or multiply, invert); Transparency: Transparent ↔ Opaque slider, ignore transparent pixels, replace them with the secondary colour; Color Balance: + Brightness, Contrast
- **Verified 2026-10-13:** unit tests for each (outline widths/AA/blur, stroke location and dashes, bevel drop shadow, alpha mask, transparency options, colour balance, Gaussian channels/Darken and defaults = plain blur, hue range with wrap and grays); render of Bevel Object and a dashed Outline Selection around an ellipse checked by eye. CI passes (307 tests).
- Sources: control names and defaults from the screenshots of the v6.0 dialogs in BoltBait's forum post (forums.paint.net topic 121343, read 2026-10-13); no screenshot exists for Gaussian Blur+ or Feather Object, so those controls are paintnotnet's design (Gaussian Blur+ from the pack's description: "blur only the Red, Blue, Green, or Alpha channel … blend the original picture with the blurred image"). Guesses: dropdown options beyond the one visible in each screenshot, slider ranges, Bevel's Colors tab contents, Bevel's "Keep original background image" (not implemented: its meaning isn't documented). All angles here mean where the light comes from.
- [ ] Feather Object unchanged (awaiting your sign-off on the Phase 5 feel). Black and White+ not folded in yet (stock Black and White applies instantly with Ctrl+Shift+G; adding options needs a decision).
- [ ] Rest of the pack (10c): Combined Adjustments, Dream, Fill from Clipboard/File, Level Horizon, Meme Maker, Photo Adjustments, Remove Dust, Vignette+ (fold into Vignette), Calendar, Flames, Text Fun Factory, Oil Paint+ (fold into Oil Painting), Pastel, Complementary Colors, Paste Alpha, Seam Carving, Sharpen Classic/Landscape (fold into Sharpen), Chart/Graph, Dimensions, Gradients, Grid/Checkerboard, Polygon/Stars, Edge Detect Classic (fold into Edge Detect), Floyd-Steinberg Dithering, Flip Horizontal/Vertical

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
### Build load (2026-10-13) ✅
- [x] The user found CI slowed their desktop: UI tests are now one test binary (`apps/paintnotnet/tests/ui/`, plus `settings_ui` alone because it changes the global shortcut table) instead of 24; dev builds keep line-table debug info only; `cargo xtask ci` runs under `nice -n 15 ionice -c 3` with 4 cores left free (`PNN_CI_JOBS` overrides). `cargo clean` freed 209 GB (target/ was 199 GB; 3 GB after a full CI). Clippy + tests after a dependency build: 109 s.

### 11c ✅ Packaging
- [x] `packaging/arch/PKGBUILD` (`paintnotnet-git`): builds the committed HEAD of this checkout (`cd packaging/arch && makepkg -si`); installs the binary, `paintnotnet.desktop`, the icon (SVG + 16–512 px PNGs in hicolor), and LICENSE, NOTICE and the 3.36 icon attribution under `/usr/share/licenses/paintnotnet-git/`. `options=('!lto')`: makepkg's LTO (on in CachyOS's makepkg.conf) turns the bundled libwebp into GCC bitcode that Rust's linker can't read.
- [x] `.desktop` file named after the Wayland app_id `paintnotnet`, so KWin matches the window to it; opens files with `%F`; MimeType lists the formats Open reads: OpenRaster, PNG, JPEG, BMP, GIF, TIFF, TGA, WebP (all already defined by shared-mime-info, so no MIME XML is needed)
- [x] App icon: original artwork (canvas tile, transparency checkerboard, gradient brush stroke), `packaging/paintnotnet.svg`; also the X11 window icon. Not derived from paint.net's logo.
- [x] `LICENSE` (MIT, as Cargo.toml already declared) added at the root
- **Verified 2026-10-13:** `desktop-file-validate` passes; `makepkg -C` builds `paintnotnet-git-0.1.0.r27` (first try failed with undefined `WebP*` symbols → `!lto`); package contents and .PKGINFO checked; the packaged binary links only libc/libm/libgcc_s (Wayland, Vulkan and xkbcommon are loaded at run time) and its smoke test ran on Wayland/Vulkan at 164.8 fps.
- Not checked: installing it (needs `sudo pacman -U`), the launcher entry, the icon in the title bar and task manager, and "Open with" from Dolphin. A second file opened from Dolphin starts a second window (paint.net opens it as a tab in the running one; not done).
- [—] Optional: pop-out panels, tablet pressure (the user doesn't want them, 2026-10-13)

---

# paintnotnet modern (from 2026-10-13)

The user (2026-10-13): "I want to really modernise this, better ways of selecting brush sizes, actual side panels to hold
the colours and layers etc without having these draggable windows, maybe a glassy look behind them, really making this a
modern paintdotnet version which looks great, runs super smooth, and enhanced with features", and: "if there is anything
we've implemented that could be better ie maybe boltbait didn't optimise enough, or there are better ways of using our
tools with better maths or other higher res more accurate more optimised definitions, then we should do that." Earlier
the same day: "a smooth consistent and modern feel across everything" (nothing should feel like a plugin).

Ground rules for this track: paint.net's layout, shortcuts and behaviour stay recognisable unless a change is clearly
better (say so when changing one); where we improve on a paint.net or BoltBait algorithm, write down what it did and why
ours is better, and keep a test showing the difference.

## Phase 13: Design system and docked shell
- [x] Mockups (2026-10-14, https://claude.ai/artifact/RRHVupDEdejtHvbVwLuDWE): A Graphite Glass, B Studio Solid,
      C Daylight Frost. **The user chose A** (2026-10-14): dark graphite, the canvas fills the window and the panels
      float over it as frosted-glass islands (tool rail left; tool options bar on top; Colors, Layers, History stacked
      on the right), document tabs and a command search in the title bar, one blue accent, Geist + Geist Mono.
- Decisions (2026-10-14): the quick buttons (New, Open, Save, Print, Cut, Copy, Paste, Crop, Deselect, Undo, Redo,
  Grid, Rulers) go at the start of the tool options bar; icons are **Lucide** (ISC), replacing the 3.36 icons; menu
  items without a good Lucide match show no icon. A's Colors picker (square + hue, OKLCH) and brush-size presets are
  Phase 14's; Phase 13 restyles the existing controls.
### 13a ✅ Design tokens and type
- [x] Tokens in `theme.rs`: colours for A's dark look and a light counterpart (same structure), corner radii,
      spacing, type scale, motion durations, control height; mapped onto every egui widget (menus, popups, buttons,
      sliders, checkboxes, text boxes, combo boxes, windows, floating scrollbars), so dialogs, effect dialogs,
      Settings and the command palette all follow; dialog title bars no longer turn blue
- [x] Geist (Regular, Medium, SemiBold) and Geist Mono v1.7.2 (SIL OFL 1.1) embedded from `assets/fonts/geist/`
      with their licence (NOTICE, PKGBUILD installs `Geist-OFL.txt`); the desktop's UI font stays as a fallback for
      scripts Geist lacks (e.g. CJK layer names)
- **Verified 2026-10-14:** unit tests check every text colour against every surface (WCAG ≥ 7:1 body, ≥ 4.5:1
  secondary and hints, ≥ 4.5:1 on the accent) in both themes, and that each font family starts with Geist and has
  fallbacks; renders of menus, Settings and an effect dialog checked by eye.
### 13b ✅ Docked shell
- [x] Title bar: logo, menus (sized to their contents), document tabs centred (thumbnail, name, • when unsaved,
      close button on the active/hovered tab, drag to reorder, middle-click closes), "Search commands" box (opens the
      Ctrl+K palette), the four panel toggles (paint.net's top-right icons, kept so the panels stay reachable by
      mouse), Settings and Help
- [x] Islands over a full-bleed canvas (`panels.rs`, replacing the 3.36 `SnapManager` floating windows): tool rail
      top-left in groups (select, move, view, paint, pick, draw), one column, two when the window is too short; tool
      options bar along the top with the quick buttons first, wrapping onto more lines when it doesn't fit; right
      dock with Colors (sized to its contents; "More" opens the sliders below the wheel), Layers (fills the room) and
      History; Layers and History buttons moved into their title rows. Dock width drags from its left edge
      (240–520 pt), History's height from the gap above it, a section collapses by clicking its title; all saved in
      `settings.toml` (`[dock]`). F5–F8 toggle panels, Ctrl+Shift+F5–F8 reset size
- [x] The canvas fits and centres in the free area between the islands and draws under them; scrollbars are thin
      pills floating over the image
- [x] Status bar on the title bar's band colour
- **Verified 2026-10-14:** layout unit tests (design A's geometry at 1440×900, hidden panels free the canvas,
  collapsed sections, History squeezed before Layers on short windows, settings round trip); UI tests: no island
  overlaps another or the free canvas, rail one column and left, dock stacked in order; dragging the dock edge 60 pt
  widens it by 60, clicking History's title collapses it to 36 pt and both land in the saved settings; every
  earlier UI test still passes (116). Renders checked: dark, light, zoomed to 300% (pixels sampled in the gaps
  show the image running under the islands). **On the real desktop** (Wayland, Vulkan): smoke test 164.6 fps with
  an image open; a screenshot of the window shows the layout, with the rail in two columns at the default size.
- Known rough edges for Phase 14: the paintbrush's options wrap onto three lines at 1202 pt wide, and a label can
  wrap away from its control; tool and command icons are still the 3.36 bitmaps until 13d.
### 13c ✅ Glass
- [x] wgpu (`pnn-render/src/glass.rs`, `glass.wgsl`): each frame the canvas callback draws the image again at a
      quarter of its on-screen size over the surround colour, then blurs it (two separable 9-tap Gaussian
      iterations, the second at twice the step); the islands and dialogs (effect, layer/image, Settings, command
      palette) paint that blur in a rounded rectangle, 1.6× saturated and mixed with the theme's glass tint, in place
      of their fill (`apps/paintnotnet/src/glass.rs`). Mixing is in sRGB as in the CSS mockup, and the tint thickens
      the further the backdrop's brightness is from it, so panel text stays readable over white (dark theme) and
      black (light theme) images
- [x] No document (nothing to blur) or no GPU: the solid panel colour (chosen over a plain translucent fill so
      text stays readable). Menus, tooltips and the modal message boxes stay solid: egui draws their frames itself
- [—] KDE window blur protocol: not needed for A (the window is opaque); skipped
- **Verified 2026-10-14:** both shaders pass naga validation; uniform layout and backdrop sizing unit tests; a UI
  test zooms a white image under the dock and requires the glass to keep secondary text ≥ 4.5:1 (5.0 measured; it
  fails at 1.9 without the tint boost); renders of the Bevel dialog over a red object (the object shows blurred
  through it) and of the dark and light shells. **On the real desktop** (Wayland, Vulkan, RTX 3070): a screenshot
  zoomed into an image shows it blurred and tinted under every island; smoke test still 164.7 fps, mean 6.07 ms.
### 13d ✅ Icons and cursors
- [x] Lucide 1.48.0 (released 2026-09-24; ISC, Feather-derived icons MIT; `assets/icons/lucide/` with LICENSE and
      README, NOTICE, PKGBUILD installs `Lucide-LICENSE`): 109 SVGs embedded, rasterised with `resvg` (no text or
      raster features; shares the existing tiny-skia 0.12) at the display's scale, re-rasterised if it changes, and
      tinted to the theme. Every tool, toolbar, panel, title-bar, status-bar and menu icon is Lucide; 19 entries
      without a clear match (some effects, Invert Colors, Posterize, Sepia) show no icon. The KDE global menu gets
      PNGs rendered in the theme's icon colour. The 3.36 bitmaps are no longer built in (still in
      `assets/icons/pdn336` for the `recreation` branch)
- [x] Drawn tool cursors: over the canvas most tools hide the system cursor and draw a crosshair (white with a dark
      halo, readable on any colour) with the tool's icon on a small dark chip beside it, above everything else; Pan,
      the move tools and Text keep the system's grab, move and I-beam cursors
- **Verified 2026-10-14:** unit tests: every mapped icon parses and draws at 16 px, every tool and toolbar icon
  has one (only effects/adjustments may lack one), the filled variant is solid; UI test: hovering the canvas with
  the Paintbrush hides the system cursor, Pan shows the grab hand; renders of the shell, File menu, Layers and
  History checked by eye, and of the drawn cursor enlarged. Not checked: the drawn cursor on the real desktop
  (needs a real pointer), the global menu's new icons in Plasma's panel.
### 13e ✅ Motion
- [x] Durations are tokens (`theme::motion`: FAST 0.12 s for hover and egui's own fades of menus, popups and
      dialogs; NORMAL 0.18 s for panels). Collapsing, expanding, showing or hiding a dock section makes the sections
      glide to their new places; F5–F8 fade a panel in or out with a short slide (the glass fades with it, and a
      fading-out panel takes no clicks); the options bar glides when the rail comes or goes; tool buttons and Layers
      and History rows ease their hover highlight
- [x] Never in the way of drawing: window resizes and splitter drags follow at once, the canvas's free area never
      animates, and nothing animates while idle (repaints are requested only during a transition)
- **Verified 2026-10-14:** a UI test at 60 fps steps: three frames after clicking History's title its height is
  part way between open and collapsed, then exactly the title height; dragging the dock edge 40 pt widens it by
  40 the next frame. A frame captured mid-collapse checked by eye. CI passes (317 tests).

**Phase 13 done (2026-10-14).**

## Phase 14: Tool controls
- Decisions (2026-10-14): the on-canvas brush gesture is **Alt + right-drag** (sideways = size, up/down = hardness;
  Photoshop's gesture; free on every tool, and Plasma 6 moves windows with Meta, not Alt). The Colors picker style
  was chosen from two interactive mockups (square + hue strip, or hue ring around a square) on the design
  canvas; **the user chose square + hue strip** (2026-10-14). "Opacity" in the toolbar is the active colour's alpha, as paint.net treats opacity (no second setting).
### 14a ✅ Brush controls
- [x] Size (`controls.rs`): a preview square showing the brush's size, hardness and colour; a logarithmic slider
      over 1–2000 px (half way ≈ 45 px, so small sizes get most of the travel); a typed value; presets 2, 5, 10,
      25, 50, 100; the mouse wheel over the slider steps through paint.net's preset list; [ ] and Ctrl+[ ] kept.
      Hardness, Spacing and Tolerance are compact sliders with typed values; **Opacity** (the active colour's
      alpha) is shown for Paintbrush, Pencil, Line/Curve, Shapes, Text and Paint Bucket
- [x] Alt + right-drag on the canvas (Paintbrush, Eraser, Clone Stamp, Recolor, Line/Curve, Shapes): 100 pt sideways
      doubles or halves the width, 2 pt up or down is 1% hardness (brushes only); the cursor hides and a HUD at
      the press point shows the brush at its true size, a ring where the hard core ends, and the values. Nothing
      is painted and no history step is made
- [x] Toolbar groups wrap as a unit (`controls::group`, using last frame's width), so a label never ends up on a
      different line from its control; this is the first piece of 14b
- [x] Fixed: a window short enough to leave the canvas no room made the scrollbar maths panic (`View::scrollbar`)
- **Verified 2026-10-14:** unit tests for the size scale (round trip, ends, half way), preset stepping and the
  degenerate viewport; UI test: Alt + right-drag 100 pt right and 40 pt up turns 2 px / 75% into 4 px / 95% with
  no history step; the HUD rendered mid-drag (20 px, Hardness 85%) and the cursor hidden; all 119 UI tests pass.
  Not checked on the real desktop (needs a real pointer).
### 14b ✅ Toolbar redesign per tool
- [x] Every tool's options are groups that wrap as a unit; the tool button shows its icon and name (Alt+T still
      opens the list); choices with few options are segmented controls instead of dropdowns: selection mode, draw
      mode (fields only for fixed ratio/size), flood mode, gradient type, colour mode and repeat, resampling,
      curve type, shape draw type, text alignment, bold/italic/underline and antialiasing toggles
- [x] Live previews: the Fill dropdown draws each hatch in the current primary and secondary colours; the dash
      style and both line caps are drawn in their dropdowns and next to the dash control; the brush preview
      (14a) shows size, hardness and colour
- [x] Less used options moved into a "⋯ More" popover: Spacing and the hatch fill for brushes; selection edge
      quality; Layer/Image sampling for the Magic Wand and Paint Bucket; the hatch fill for Line/Curve and
      Shapes; the Text tool's size units. Finish is a button with a check icon
- **Verified 2026-10-14:** all 119 UI tests pass (one updated: the Finish button's label); renders of the
  Paintbrush, Line/Curve and dialog toolbars checked by eye (dash-dot preview and caps drawn, groups wrap whole).
### Requested fixes (2026-10-14) ✅
- [x] Turning off Settings › Menus › global menu left the menus in Plasma's panel as well as the window: releasing
      the appmenu object doesn't make KWin forget the address. Now the address is set to empty, and while off the
      exported menu is empty too (unit test for the empty export). Not checked live on Plasma yet.
- [x] The window showed KWin's yellow Wayland icon when run from the checkout: KWin finds a window's icon through
      the `.desktop` file named after its app ID, which only the package installs. `cargo xtask install-desktop`
      installs the entry (Exec pointing at the checkout's build) and the icon in `~/.local/share` (never
      overwriting a different file without `--force`); `cargo xtask uninstall-desktop` removes them. Verified: the
      title bar shows paintnotnet's icon. Setting the icon from inside the app isn't possible: winit 0.30.13 has no
      `xdg-toplevel-icon` support and doesn't expose the toplevel object
- The global menu's two `unsafe` calls stay: the appmenu request must name winit's own `wl_surface` on winit's own
  connection (Wayland objects belong to one connection), and winit only hands those out as raw pointers. There is
  no safe route on Wayland today; they stay confined to `global_menu/wayland.rs`.
### 14c ✅ Colour picker (square + hue strip)
- [x] Colors panel: overlapping primary/secondary swatches (click to make active; swap and reset kept), hex entry
      (RRGGBB or RRGGBBAA), a saturation/value square drawn from exact HSV samples, hue and alpha strips; left
      button edits the active colour, right button the other one (as paint.net's wheel did)
- [x] Three sliders with gradient tracks and typed values, switched in the title row between **OKLCH** (default;
      lightness, chroma, hue as the eye sees them; out-of-gamut choices keep lightness and hue and lose chroma, as
      CSS Color 4 does), HSV and RGB. Hue survives greys in both HSV and OKLCH
- [x] Recent colours: the last 10 colours picked anywhere (this panel, the Color Picker tool, X, the palette),
      added when a drag ends; click for the active colour, right-click for the other. Kept for the session only
- [x] Palette: two rows, "All" shows the rest; add-colour and palettes menu as before
- [—] The wheel and its paint.net modifier keys (Ctrl = hue only, Shift = snap hue to 15°…) went with it
- **Verified 2026-10-14:** OKLab unit tests against published values for white, red and blue, a round trip over
  an 8-step RGB grid (±1), gamut mapping keeps lightness and hue; recent-colour rules; UI tests: dragging the square
  from black to 75%/75% gives (191, 48, 48), a click a third along the hue strip gives (48, 191, 48) and adds it to
  Recent, a right-drag edits only the secondary, the title row switches OKLCH/HSV/RGB sliders; render checked by
  eye. CI passes (326 tests).

**Phase 14 done (2026-10-14).**

### Requested: modern dialogs (2026-10-14) ✅
- [x] The user found the message dialogs dated (they had design A's colours and font but paint.net's layout). All
      modal dialogs now share one shape: icon and larger title, a muted description, buttons on the right with the
      main action filled in the accent (Enter picks it, Esc cancels), a soft shadow and a dimmed backdrop
      (unsaved changes, exit, flatten, oversized paste, JPEG/WebP options, recovery, errors, About)
- [x] New Image: six presets (800 × 600, HD, Full HD, 4K UHD, square 1080, A4 at 300 dpi) as cards, width and
      height with a link toggle for the aspect ratio and a swap button, a live preview of the shape, the memory size,
      Create / Cancel. About now credits Lucide and Geist instead of the 3.36 icons
- **Verified 2026-10-14:** UI test: Full HD then swap then Enter creates 1080 × 1920 (it caught the aspect lock
  rescaling a swapped size to 1080 × 608, fixed); the WebP options test updated for the new labels; render of
  New Image checked by eye; all 122 UI tests pass.

## Phase 15: Better maths, accuracy and speed (audit of everything built so far)
### 15a ✅ Audit
- [x] `pnn-effects` bench example: every adjustment and effect at defaults or at a fraction of each slider's range
      (`BENCH_AT`), rendered as the app renders (256 px tiles in parallel) on a 4K image
- [x] `framework::render_rect` renders in tiles like the app (it went row by row, which made separable blurs redo
      their vertical pass per row in tests and benchmarks)
- [x] Findings and plan in `docs/phase15-audit.md`: slow at 4K (75% sliders) are Mandelbrot 5.6 s, Motion Blur
      4.9 s, Radial Blur 4.0 s, Outline / Median / Reduce Noise / Dents ~2.5 s, Julia 2.4 s, Surface Blur, Bokeh,
      Morphology 1.1–1.4 s; accuracy: Gaussian is 3.36's tent, gradients band (256-entry table), shrinking a
      selection aliases, wand tolerance is RGB; linear-light blending not recommended as a default
### 15b ✅ Speed on the CPU (exact speed-ups; the rest moves to the GPU, 15d)
- [x] Local-histogram effects (Median, Reduce Noise, Outline, Surface Blur, Sharpen): the tile's source block is
      copied once and the disc window snakes through the tile instead of being rebuilt every row; output is
      bit-identical. At 75% sliders: Median 2.5 → 1.6 s, Reduce Noise 2.5 → 1.5 s, Outline 2.6 → 1.9 s, Surface
      Blur 1.4 → 1.2 s. The rest of the cost is the O(radius) window itself: constant-time methods need a square
      window, which would change the effects' look
- [x] Motion Blur samples a copied source block (`Block`, reusable by other sampling effects): 4.9 → 3.8 s, output
      identical. Its cost is the ~1.5 bilinear samples per pixel of distance, which the GPU does in milliseconds
- [x] Mandelbrot skips points in the main cardioid and period-2 bulb (they clamp to 255 in every channel after
      1024 iterations): exact (tested over a 300 × 200 grid at three factors); zoomed out 0.26 s, but the default
      view sits on the boundary and stays ~4.8 s
- Not done on the CPU, left for 15d: Radial and Zoom Blur, Julia, the remaining Mandelbrot cost, Dents and other
  Distort effects, Bokeh, Morphology: per-sample work dominates, and the CPU options left (rotate-and-blur Motion,
  periodicity checks) change results
### 15c ✅ Accuracy
- Decisions (2026-10-14): the true Gaussian replaces the tent (no classic option); linear light is an option in the
  blurs, off by default; the wand and bucket compare in OKLab by default, RGB under More.
- [x] **Gaussian:** a true Gaussian replaces 3.36's tent (a triangle) in Gaussian Blur, Drop Shadow and the three
      effects built on it (Glow, Soften Portrait, Ink/Pencil Sketch). σ matches the tent's variance at the same radius
      (radius keeps its meaning); small σ (≤ 8) is sampled exactly, larger σ is four Kovesi boxes (cost independent
      of radius; each pass normalised so wide blurs can't overflow); final division rounds. Default radius 166 → 90 ms
      at 4K; radius 150: 388 ms. Tests: the old tent's 2-D kernel is 15%+ off-round, the new one within 3%; σ within
      3% of target and shape within 4% of the ideal curve for radii 1–120; radius 200 on 900 × 600 keeps a flat
      colour exactly (debug build, overflow-checked)
- [x] **Linear light** checkbox (off by default) in Gaussian, Motion, Zoom and Radial Blur: 16-bit linear averaging;
      tests: across a black/white edge sRGB averaging gives 128, linear ≈ 188
- [x] **Gradients:** per-pixel colour from a 4096-step float table (3.36: 256 entries) dithered to 8 bits with
      interleaved gradient noise. Test: column averages of a 4096 px ramp stay within 0.34 of ideal (3.36's table:
      off by 0.9+). 4K render 7 → 14 ms per gradient (rounding made branch-free; `round`/`fract` were library calls)
- [x] **Shrinking in the move tools:** when the transform shrinks (smaller singular value < 1), each output pixel
      averages up to 8 × 8 samples; Nearest Neighbor stays point-sampled. Test: a 1-pixel checkerboard at quarter size
      gives grey within 20 levels (123 without the fix)
- [x] **Wand and bucket tolerance:** OKLab distance by default (`Tolerance` in `pnn-core`; 0.96·t², calibrated so
      50% reaches through greys as paint.net's does; 100% matches all), "Compare colors: As seen / RGB" under More.
      Test: a dark-blue and a light-yellow step of equal RGB size: RGB treats them alike, OKLab matches the yellows only
- Gradient UI tests now accept the dither's ±1 level at exact half-way pixels.
### 15d GPU and frame budget (in progress)
- [x] Measured (release, 4K photo, 165 Hz display, `--smoke-test` modes; `PNN_PROFILE=1` now prints per-stage
      timings): pan/zoom, brush strokes (50 and 300 px) and the feather slider hold 165 fps; the live gradient ran
      29 fps and the Posterize preview 121 fps with 26 ms spikes. Every full-image change paid ~12.5 ms to reach the
      screen: the CPU built each tile's 9-level premultiplied mip chain (5 ms) and uploaded ~1200 textures (7.4 ms)
- [x] **Tiles to the screen on the GPU** (`pnn-render/src/tile_upload.rs`, `tiles.wgsl`): changed composite tiles
      are uploaded raw in 64-tile batches; compute passes premultiply them into the chunk's level 0 and build the mip
      chain with the same rounding box filter; chunk textures are now `Rgba8Unorm` (writable by compute). Upload stage
      12.5 → 3.9 ms; Posterize preview 121 → 158 fps (spikes 26 → 16 ms)
- [x] Live gradient: tiles are un-shared (the compositor shares a single plain layer's tiles) in parallel instead of
      one by one on one thread (`TiledBuffer::slots_in`); Normal blending skips the arithmetic for opaque and (over
      non-transparent) fully transparent sources, exactly (tested over 40 000 random pairs). Gradient render 16.5 →
      12 ms; live gradient 29 → 53 fps
- [ ] To decide: a live gradient (or any whole-image preview) at 165 fps needs the preview drawn on the GPU, which
      means compositing layers there too (a GPU compositor); the CPU path is now ~12 ms render + ~4 ms upload
- [ ] wgpu compute for effects still slower than ~100 ms at 4K (Motion / Radial / Zoom Blur, fractals, Dents, …)
## Phase 16: Remove Background (local AI) — requested 2026-10-13
- [ ] Effects › Object › Remove Background: BiRefNet (MIT; the model BRIA's RMBG-2.0 is built on) run with ONNX Runtime,
      loaded at run time from the system (`onnxruntime-opt-cuda` 1.29 is installed: CUDA on the RTX 3070, CPU fallback),
      through the `ort` crate (2.0.0-rc.14, `load-dynamic`, `api-29`)
- [ ] Models downloaded on first use after asking, checked by SHA-256, stored in `~/.local/share/paintnotnet/models/`:
      onnx-community/BiRefNet-ONNX @534d3c8 (fp16 490 MB for GPU, fp32 973 MB for CPU) and BiRefNet_lite-ONNX @de15b22
      (fp16 115 MB, fp32 224 MB); input 1024×1024, ImageNet mean/std normalisation (the repo's preprocessor_config.json)
- [ ] Dialog in the common style: quality (best / fast), edge softness, shift edge in/out, invert; inference once per
      layer in the background, sliders re-shape the mask live
- [ ] PKGBUILD: optdepends onnxruntime

## Phase 17: Enhanced features (to agree)
- [ ] Ideas to discuss, not yet agreed: adjustment layers / non-destructive effects, layer groups, select subject (the
      Remove Background mask as a selection), content-aware fill, smarter magic wand, more export options

---

# Recreation backlog (parked; on the `recreation` branch if wanted)
- [ ] Rest of the BoltBait pack (10c list above); Black and White+ options
- [ ] Sketch Blur (no published maths)
- [ ] Compare against the paint.net 5.1.12 release files (`~/Downloads/paint.net.5.1.12.portable.x64`; the user states
      the owners gave permission); CLAUDE.md still says never decompile 4.x/5.x, so that rule needs updating first
