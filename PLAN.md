# paintnotnet: plan

A faithful paint.net 5.x remake for Linux, written in Rust. The goal is paint.net's
keyboard, layout and feel, plus a few deliberate improvements. It is not a Photoshop.

Status: **planning complete, no code yet.** Phase tracking lives in [ROADMAP.md](ROADMAP.md).
Reference material gathered from the official docs lives in [docs/reference/](docs/reference/).

---

## 1. Target machine (surveyed 2026-10-09)

| | |
|---|---|
| OS | CachyOS (Arch-based, rolling), kernel 7.2.9-cachyos |
| Desktop | KDE Plasma 6, **Wayland** session (XWayland available), Breeze Dark |
| GPU | NVIDIA RTX 3070, proprietary driver 615.71, Vulkan 1.4, GL 4.6 |
| CPU / RAM | Ryzen 7 5700X3D (8C/16T, AVX2 + FMA), 32 GB |
| Displays | DP-1 2560×1440 @ 165 Hz (primary, scale 1); HDMI-A-1 1080×1920 portrait (scale 1) |
| Input | Keychron Q1 (US layout, locale en_GB), Glorious Model O. **No drawing tablet detected.** |
| Toolchains | gcc 16, clang 23, cmake, ninja, git, gh. **Rust is not installed.** |
| Image libs present | libavif, libheif, libjxl, libwebp, libraw, lcms2 |
| Other editors installed | Krita |

What this means for the design:
- **Wayland first.** Apps on Wayland can't position their own top-level windows. paint.net's
  floating Tools/History/Layers/Colors windows therefore have to be panels drawn *inside* the
  main window, which is also how paint.net itself behaves. Pop-out to the portrait monitor is
  an optional extra.
- **Shortcut conflicts.** None that matter. I parsed `~/.config/kglobalshortcutsrc` against
  every paint.net shortcut, and the only overlap is `Alt+F4` (KWin "Close Window"), which
  means the same thing in paint.net. Plasma 6's window-drag modifier is unset here, which
  leaves it at Plasma's default. I believe that default is Meta, which would leave paint.net's
  `Alt+drag` (subtract selection) free. **Needs a check in Phase 2.**
- **Frame budget.** Aim for smooth pan, zoom and brush at 165 Hz on 1440p.
- **Tablet pressure is low priority** because no tablet is connected. paint.net 5 supports
  pressure, so it stays on the list for later.

## 2. What we're copying (evidence)

All of this comes from the official paint.net 5.x docs at <https://paint.net/doc/latest/> and is
recorded in detail in `docs/reference/`:

- [keybindings.md](docs/reference/keybindings.md): the **complete** official keyboard and
  mouse command table (source: KeyboardMouseCommands.html, revised 3 Aug 2024). It becomes a
  conformance test.
- [paintnet-inventory.md](docs/reference/paintnet-inventory.md): tools, toolbar options,
  every menu, the 14 blend modes, adjustments, effects, and BoltBait's plugin pack.

Findings that shape the plan:

1. **"Feather" is not built into paint.net.** Effects > Object only has Drop Shadow
   ([EffectsObjectMenu.html](https://paint.net/doc/latest/EffectsObjectMenu.html)). The
   Feather you know comes from **BoltBait's plugin pack** (v5.0+, 44 effects,
   <https://www.boltbait.com/pdn/>), which has two variants: *Object > Feather Object* uses
   transparency, and *Selection > Feather Selection* uses the marching ants. You confirmed you
   use both, plus the pack in general. The pack's parameter names and ranges weren't reachable
   (the forum thread returned 403), so our feather is designed on its own merits (§5) instead of
   cloning BoltBait's sliders.
2. paint.net selections are **geometry**: polygon lists, which is why Copy Selection exports
   JSON `{"polygonList": [...]}`. They have an *antialiased* or *pixelated* clipping mode but
   no soft edge. That gap is why the feather plugins exist.
3. **14 layer blend modes**: Normal, Multiply, Additive, Color Burn, Color Dodge, Reflect, Glow,
   Overlay, Difference, Negation, Lighten, Darken, Screen, Xor. Layer opacity is 0–255.
4. Tools that stay **editable until Finish/Enter**: shapes, line/curve, text, gradient nubs,
   the magic wand click point and tolerance, and moved pixels. This "pending session" model
   is central to how paint.net feels, so it's a first-class concept in our architecture.
5. **`.pdn` support** (changed 2026-10-14; first decided as not needed). paintnotnet opens and
   saves `.pdn`. The format has no public spec (the developer says it is .NET binary
   serialization, [forum](https://forums.getpaint.net/topic/124516-pdn-file-format/)); the
   layout is ported from 3.36's `Document.cs` and `MemoryBlock.cs`, and .NET's format from
   Microsoft's open [MS-NRBF] specification. See ROADMAP Phase 16.

### Prior art

- **OpenPDN = real paint.net source** ([rivy/OpenPDN](https://github.com/rivy/OpenPDN)):
  unmodified paint.net **3.36.7**, the last release before the 2009 licence change. It has
  649 C# files and is cloned read-only into `reference/OpenPDN` (git-ignored). Licence
  (`src/Resources/Files/License.txt`):
  - **Code: MIT.**
  - **Logo, icons, `.png`/`.resx`/`.resources` assets and UI text: CC BY-NC-ND 2.5.**
    Non-commercial, unmodified, attributed use is fine for this personal project.
  - **GPC polygon clipper: non-commercial only.** We use a Rust clipper instead anyway.

  It contains the actual implementations: all 14 blend ops (`Data/UserBlendOps.Generated.cs`,
  whose names match 5.x's list exactly), `ColorBgra`, every tool (`tools/*.cs`: brush,
  selection, move with nubs and rotation, flood fill / magic wand, shapes, gradient, …),
  selection rendering, history mementos, the floating tool windows with edge snapping, the
  colour wheel, the gradient renderers, and about 40 effects and adjustments. **This is our
  primary source for behaviour and maths.** We port it into Rust with MIT attribution. Where
  5.x behaves differently (hardness, spacing, antialiased selections, the 4.0 shapes rewrite,
  sampling modes, new effects), the 5.x docs win.
- **Pinta: secondary reference, checked per feature** (MIT; cloned at `reference/Pinta`,
  commit c50533f, 2026-10-08). Its readme says it is "a GTK clone of Paint.Net 3.0" and that
  "Code from Paint.Net 3.36 is used under the MIT License". Spot checks on 2026-10-09:
  - `CreateGaussianBlurRow` is **identical** to OpenPDN's.
  - Layer blend modes **diverged**. Pinta dropped Additive, Reflect, Glow and Negation, and
    added HardLight, SoftLight, Color, Luminosity, Hue and Saturation, which are not paint.net modes.

  Rule: anything taken from Pinta must first be compared with OpenPDN, or with the 5.x docs
  where OpenPDN has nothing. Record the result in the ported file's header. Pinta's UI and tool
  feel are never a reference. Useful parts: algorithms added after 3.36, Linux/GTK
  integration lessons, and 137 test PNGs, which work as cross-check goldens only where the
  code still matches OpenPDN.
- **paint.net 4.x/5.x** is closed source. Its licence (since 3.5) prohibits reverse
  engineering and derivative works, so I won't decompile it or port its code. 5.x behaviour
  comes from the official docs, careful observation, and your memory of how it felt. In
  practice the 3.36 source plus the 5.x docs covers most of what decompiling would give.
- **storytold/photocraft** (MIT/Apache-2.0, Rust): about 30k stars, early alpha, 24 crates,
  **egui 0.36 + eframe**, wgpu 30 GPU compositor with a CPU reference compositor tested
  against it, 256² copy-on-write tiles for undo, and rayon for filters. That shows egui + wgpu
  can carry a full raster editor. We take its *patterns*, not its scope.
- **Graphite** (Rust): wrote its own GUI, abandoned it for a web UI. Lesson: **don't write a
  GUI toolkit.**
- **Source policy.** Port freely from OpenPDN's MIT code, keeping attribution in a `NOTICE`
  file and in the headers of ported files. Use 5.x docs and observation for everything newer.
  Never decompile 4.x/5.x.

## 3. Language and stack decisions

**Rust.** This is your preference, and it fits. Pixel loops are fast with SIMD (AVX2 here),
rayon parallelism is trivial, memory safety removes a whole class of crashes in a long-running
editor, and wgpu gives first-class Vulkan on your NVIDIA card.

**GUI: egui + eframe on the wgpu backend.** Recommended over iced, GTK4 (gtk4-rs/relm4) and Slint:

| Need | egui | iced | GTK4 |
|---|---|---|---|
| Floating, overlapping tool windows inside the main window (paint.net's model, and the only portable model on Wayland) | `egui::Window` does exactly this, with title-bar drag and collapse | No built-in overlapping windows; would need custom work | Would need custom overlay widgets; native sub-windows can't be placed on Wayland |
| Custom GPU canvas | `egui_wgpu` paint callbacks share eframe's wgpu device | Canvas/shader widget, workable | GL area interop, more friction from Rust |
| Pixel-exact paint.net look (custom theme, dense toolbars) | Fully custom painter; no native-look fight | Themeable | Fights libadwaita/Breeze styling (part of why Pinta felt off is GNOME styling) |
| Proven for a large editor in Rust | photocraft | — | Pinta (C#) |
| Headless UI tests | `egui_kittest` | limited | limited |

Trade-offs we accept: egui redraws every frame (cheap at this scale and it suits a 165 Hz
display), we hand-build menu mnemonics (`Alt+F`, …), and the UI is not "native Breeze".
paint.net isn't native-looking either; it has its own look.

Other crates (exact versions are pinned in Phase 0 after checking crates.io):

| Purpose | Crate |
|---|---|
| Parallel pixel work | `rayon` |
| Vector rasterization (shapes, lines, selection masks, brush stamps, gradients) | `tiny-skia` |
| Polygon boolean ops for selection modes (Replace/Add/Subtract/Intersect/Invert = XOR) | `i_overlay` (to evaluate) |
| Text shaping and layout for the Text tool | `cosmic-text` (to evaluate against `parley`) |
| Codecs: PNG/JPEG/WebP/BMP/GIF/TIFF/TGA/ICO | `image` |
| ORA (OpenRaster) container | `zip` + `quick-xml` |
| File dialogs and portals (KDE file picker on Wayland) | `rfd` with the xdg-portal backend |
| Clipboard images on Wayland | `arboard` (wayland-data-control) to evaluate; fallback `wl-clipboard-rs` |
| Settings and keymap files | `serde` + `toml` |

## 4. Architecture

```
paintnotnet/
├─ crates/
│  ├─ pnn-core      document, layers, tiled surfaces, colour, blend ops, history, selection + masks
│  ├─ pnn-sdf       distance-field engine: feather, outline, bevel, inner/drop shadow
│  ├─ pnn-effects   adjustments + effects (rayon), property schema for auto-built dialogs
│  ├─ pnn-tools     tool trait + all 19 tools, pending-session model, overlays/nubs
│  ├─ pnn-io        codecs, ORA read/write, native format, clipboard bridge
│  └─ pnn-render    wgpu canvas: tile textures, zoom/pan, checkerboard, pixel grid, ants
└─ apps/paintnotnet egui shell: menus, toolbar, floating windows, dialogs, keymap, settings
```

Six library crates is a deliberate middle ground: enough to keep GUI dependencies out of the
core and keep it unit-testable, without photocraft's 24-crate ceremony.

**Pixels.** paint.net stores 8-bit BGRA, straight (non-premultiplied) alpha. We store 8-bit
RGBA straight alpha per layer to match paint.net's numbers, and compute in f32 inside effects
where precision matters. The surface type has its bit depth isolated so 16-bit could come
later, but it is not a goal.

**Tiles.** Each layer is a sparse grid of 256×256 tiles held in `Arc`s and copied on write.
Empty tiles aren't allocated, so a mostly transparent 8000×8000 layer is cheap. Undo snapshots
share untouched tiles.

**History.** Each action records the tiles it touched (before and after Arcs) plus metadata
(name, icon, selection before and after). The History window lists the actions; clicking one
undoes or redoes everything up to it. History stays in memory with a byte budget, and old
steps spill to a temp file on disk (paint.net says history is "limited only by available disk
space and memory").

**Selections.** A selection is **geometry + edge settings**:
`Selection { polygons, clip_mode: Antialiased|Pixelated, feather: Feather { radius, profile, position } }`.
From that we lazily rasterize a `Mask` (u8 coverage per pixel, tiled like layers). **Every
operation that respects the selection takes the Mask, not a boolean region.** That includes
fill, erase, cut/copy, move pixels, paint tools, adjustments and effects. Because this is built
into the core from Phase 1, soft selections work everywhere without retrofitting.

**Compositing and display.** The CPU compositor (rayon, dirty tiles only) is the single source
of truth: deterministic and golden-testable. Dirty tiles upload to GPU textures, and the GPU
only *displays*: zoom (nearest at ≥100%, mipmapped below), pan, checkerboard, pixel grid
(≥200%, as in paint.net), marching ants, nubs and handles. GPU compositing (photocraft's
approach) is only considered if profiling demands it.

**Tools.** `trait Tool { pointer_down/move/up, key, toolbar(ui), overlay(painter), commit, cancel }`.
A tool can open a **pending session**. Shapes, gradient, magic wand, move, text and line/curve
stay editable until Finish, Enter or a tool switch, exactly as in paint.net.

**Effects.** Each adjustment or effect declares typed properties (slider ranges, angle,
checkbox, enum, colour, point). Dialogs are generated from that declaration with a live canvas
preview, as paint.net's IndirectUI does for plugins. `Ctrl+F` repeats the last effect with its
last settings. Effects receive `(src, dst, mask, roi)`, so soft selections apply for free.

**Commands and keymap.** There is one command registry, and menus, toolbar buttons, the keymap
and the command palette all call commands from it. The default keymap is generated from
`docs/reference/keybindings.md`, and a test fails if any documented shortcut doesn't resolve
to a command (unimplemented commands are listed explicitly so the gap stays visible). The
input engine also handles:
- **Multi-tap cycling:** `S` ×1–4 cycles selection tools within 1 s and `Shift+S` cycles
  backwards. `M` and `O` work the same way.
- **Held modifiers:** Space to pan, Ctrl in the colour picker to sample the image, Shift in
  fill for global mode, and Shift to constrain to 15°, squares or circles.
- **Mouse chords:** right button paints the secondary colour; Ctrl/Alt with left/right gives
  the selection modes; left and right together moves a selection while you're still drawing
  it; middle drag pans.
- **Context routing:** the active tool's session sees keys first (arrow nudges, `A` to cycle
  shapes, `,` `.` `/` for caps). The text tool captures typing.
- **Layout:** keys are matched on logical key on your US layout. Non-US layouts are flagged
  for later, as paint.net's own docs note for `~`.

## 5. The feather system (your top priority)

paint.net has no soft selections, and BoltBait's plugins bake the feather into pixels.
Ours treats feather as part of the selection. Three pieces, all on one engine:

**Engine (`pnn-sdf`).** An exact Euclidean distance transform (Felzenszwalb–Huttenlocher,
linear time, rayon over rows and columns) on the hard mask gives a signed distance field.
Feather is then a *shaping function* applied to that distance, not a blur. That matters:
- Blurring a mask bleeds colour into transparent areas and rounds off corners unpredictably.
  A distance ramp gives the same falloff width everywhere, at any radius.
- Radius costs nothing extra (the transform is O(pixels) whatever the radius), so a live
  slider stays fast on large images.
- The same field drives Outline, Bevel, Inner Shadow and Drop Shadow, which are the rest of
  the BoltBait set you rely on.

**1. Live selection feather.** Every selection tool gets a toolbar field,
`Feather: [ 0 ] px` with ▾ options:
- **Profile:** Smooth (smoothstep, default), Linear, Gaussian.
- **Position:** Inside (default; the soft edge falls inside the ants), Centered, Outside.
- **Preview:** a tinted overlay outside the selection (like a quick mask), toggled from the
  toolbar.

The feather stays editable while the selection exists: draw a lasso, try 4 px, then 12 px,
then delete. **Edit > Feather Selection…** adds the dialog form, bound to `Shift+F6`
(Photoshop's binding, which is unused in paint.net). The marching ants follow the 50% contour.

**2. Feather Object** (Effects > Object > Feather Object). This works from the layer's alpha
rather than the selection, and stays limited to the selection if there is one:
- **Radius and Profile:** as above.
- **Edge threshold:** which alpha counts as "object".
- **Treat canvas edge as transparent:** on or off. This fixes a known paint.net complaint,
  ["Feather object while ignoring canvas edge"](https://forums.paint.net/topic/122680-feather-object-while-ignoring-canvas-edge/).

It multiplies existing alpha, so antialiased edges stay clean.

**3. Companions** on the same engine: Blur Selection Edge; Outline Object/Selection; Bevel
Object/Selection; Inner Shadow (object and selection); Drop Shadow Plus. These are BoltBait's
"Selection" and "Object" families.

Success criteria: dragging the feather slider repaints the preview at 165 Hz on a 4K image.
Feather 0 gives pixel-identical results to the unfeathered path. Golden tests cover a square,
a thin line and a ring with a hole, at radii 1, 8 and 64.

## 6. Layout (matching paint.net 5)

From [MainWindow.html](https://paint.net/doc/latest/MainWindow.html) and
[Toolbar.html](https://paint.net/doc/latest/Toolbar.html):

```
┌ Menu: File Edit View Image Layers Adjustments Effects ── [Tools][History][Layers][Colors][⚙][?]┐
├ Toolbar row 1: New Open Save Print │ Cut Copy Paste │ Crop Deselect │ Undo Redo │ Grid Rulers  ┤
├ Toolbar row 2: [Tool ▾] tool options… (width, hardness, AA, blend, selection mode, feather…)  ┤
├ Image list: live thumbnails, drag to reorder, * for unsaved                                    ┤
│ ┌Tools┐                         canvas                                       ┌History─┐        │
│ │     │                                                                      │        │        │
│ └─────┘                                                                      ├Layers──┤        │
│ ┌Colors┐                                                                     │        │        │
├ Status: hint/help │ progress │ image size · cursor pos · units │ zoom slider/box                ┤
```

Where the image list and floating windows sit by default isn't described in the docs text, only
in screenshots. **Phase 2 opens by comparing against the official screenshots**, and against
your memory or any old screenshots of your own setup. Floating windows can be dragged, snap to
edges, and reset with `Ctrl+Shift+F5…F8`.

**Icons.** OpenPDN ships paint.net 3.36's 185 original icons (16×16 PNG, CC BY-NC-ND 2.5,
some from Silk). We can use them **unmodified** with attribution in a personal, non-commercial
build, so Phase 2 starts with these. They're the 3.x style, though, and 5.x redrew its icons.
If you'd rather have the 5.x look, we draw our own set later. Upscaling for HiDPI would count
as modifying them, which is fine here because your displays are at scale 1.

## 7. Our improvements (kept small and opinionated)

Rule: nothing that makes it feel like Photoshop. paint.net's simplicity is the product.

1. Live, non-destructive feather with preview (§5).
2. The BoltBait set built in, prioritised by what you actually used.
3. Wayland-native, fractional scaling handled correctly, 165 Hz-smooth canvas.
4. **Rebindable keys** with a locked "paint.net" preset you can always reset to.
5. **Command palette**, proposed on `Ctrl+K` (unused by paint.net), to search every command.
6. **Autosave and crash recovery** of open documents.
7. **Native format = OpenRaster (`.ora`)**: opens in Krita and GIMP. paint.net-only blend
   modes (Reflect, Glow, Negation, Additive, Xor) go in namespaced attributes and fall back to
   Normal in other apps.
8. **Pop-out panels** (e.g. Layers and History onto the portrait monitor), optional and off by
   default.
9. Effect dialogs with a split before/after preview.
10. Later and optional: layer groups. Discuss before doing it, as it starts down the
    Photoshop road.

## 8. Verification strategy

- **Core:** unit tests and golden-image tests (blend modes against values computed by hand
  from OpenPDN's `UserBlendOps`, selection boolean ops, feather profiles, effects).
- **Keymap:** conformance test against `docs/reference/keybindings.md`.
- **UI:** headless `egui_kittest` tests driving real key and mouse sequences (for example
  `S S` gives Lasso, and Ctrl+drag adds to the selection).
- **Visual:** screenshots of the running app (`spectacle -b -n -a -o …`) compared with
  paint.net's doc screenshots.
- **Performance:** `criterion` benchmarks for compositing, feather and blur at 4K and 8K, plus
  an on-screen frame-time overlay in debug builds.
- **Parity limit:** paint.net 5 needs Windows 10+ and Direct2D, so it probably **won't** run
  under Wine for side-by-side comparison. *(Untested; worth one try.)* A Windows VM would be
  the reliable route if exact pixel parity ever matters.

## 9. Risks

| Risk | Mitigation |
|---|---|
| Scope: about 19 tools, 13 adjustments, about 40 effects, plus 44 BoltBait effects | Phases ordered by your priorities, so each phase leaves a usable app |
| winit tablet pressure on Wayland | Deferred (no tablet). photocraft built a `tablet` crate if it's needed later |
| NVIDIA + Wayland + wgpu quirks | Phase 0 verifies on this exact machine before anything else |
| Text tool quality (shaping, hinting) | Separate shaping engine for canvas text; egui text is only for UI |
| "Feels off" in ways that are hard to name | Short loops: you try each phase and we tune feel (brush smoothing, zoom steps, nub sizes) |
| Copyright and licensing | MIT attribution for OpenPDN-ported code; 3.36 icons only unmodified and non-commercial; never decompile 4.x/5.x; never publish as "paint.net" |

## 10. Open questions (not blocking Phase 0)

1. Icons: start with the original 3.36 icons; decide later whether to redraw them in the 5.x style.
2. Name: keep **paintnotnet**?
3. GitHub remote? Local git only for now (initialised 2026-10-09).

Resolved: Rust 1.99 stable installed via rustup; git initialised.
