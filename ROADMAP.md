# Roadmap

Design and rationale: [PLAN.md](PLAN.md). Each phase ends with a usable app and a check-in
where you try it. A phase is only ticked off once its acceptance checks have been run and
shown passing.

**Current state (2026-10-13):** Phases 0–7 are complete, verified and committed (Phase 5
still awaits your sign-off on the feel). Phase 7 (Line/Curve, Shapes, Text) is done. Phases 8a (effect framework) and 8b (simple adjustments) are done.
**Next: Phase 8c (Curves, Levels).** After all phases, compare against the paint.net 5.1.12 release files the user provided
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

## Phase 8: Adjustments and the effect framework
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
### 8c Curves and Levels
- [ ] Curves (luminosity / RGB, editable points), Levels (histograms, in/out, gamma, per channel)

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
