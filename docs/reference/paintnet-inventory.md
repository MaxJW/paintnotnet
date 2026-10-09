# paint.net 5.x feature inventory (reference)

Gathered 2026-10-09 from the official docs at <https://paint.net/doc/latest/>. The doc pages
carry revision dates from 2023 to 2025. Anything that is only visible in a doc screenshot is
marked **(screenshot only)** and needs confirming visually.

## Main window — [MainWindow.html](https://paint.net/doc/latest/MainWindow.html)
1. Title bar: image name (or "untitled"), version; `*` prefix = unsaved.
2. Menu bar: seven menus on the left (File, Edit, View, Image, Layers, Adjustments, Effects), six utility icons on the right (Tools, History, Layers, Colors, Settings, Help).
3. Tool bar, directly under the menus. Row 1 holds common commands; row 2 holds the active tool's options.
4. Image list: one live thumbnail per open image, like browser tabs; drag to reorder; `*` in the corner = unsaved.
5. Canvas plus the grey surround (together, the "Editing Window"). The default new image is 800×600 scaled by system DPI.
6. Tools window: active tool highlighted, tooltips show shortcuts.
7. History window: every action this session; click to jump; cleared on close.
8. Layers window: ≥1 layer; double-click → Layer Properties; drag to reorder.
9. Colors window.
10. Status bar: left = hints, centre = render progress, right = image size, cursor position, units, zoom controls.
11. Default positions and docking of the floating windows: **(screenshot only)**.

## Tool bar — [Toolbar.html](https://paint.net/doc/latest/Toolbar.html)
- Row 1: New, Open, Save, Print, Cut, Copy, Paste, Crop, Deselect, Undo, Redo, Pixel Grid toggle (hidden below 200% zoom), Rulers toggle.
- Row 2: Tool dropdown (Alt+T), then the tool's options:
  - Brush width with +/− buttons, `[` `]`; mouse wheel works on white-background controls (not Tolerance).
  - Selection Mode: **Replace, Add, Subtract, Intersect, Invert** (invert = xor).
  - Rectangle-select drawing: **Normal, Fixed Size, Fixed Ratio**.
  - Flood Mode (Magic Wand, Paint Bucket): **Contiguous, Global**.
  - Tolerance (Magic Wand, Paint Bucket, Recolor), shown as a percentage.
  - Tolerance alpha mode: **Premultiplied** (default) / **Straight**.
  - Sampling: **Layer / Image** (Magic Wand, Paint Bucket, Color Picker).
  - Color Picker: sample size (six presets, **screenshot only**); after click: **Do Not Switch** (default), **Switch to Previous**, **Switch to Pencil**.
  - Move Selected Pixels resampling: **Bilinear / Nearest Neighbor**.
  - Rasterization: antialiasing on/off.
  - Tool blend mode (split button); includes **Overwrite** (copy blending).
  - Selection clipping: **Pixelated** / **Antialiased** selection quality.
  - Text: font, size, style, alignment.
  - Finish button (Enter).

## Tools (Tools window order) — [ToolsWindow.html](https://paint.net/doc/latest/ToolsWindow.html)
| # | Tool | Key | Notable options |
|---|---|---|---|
| 1 | Rectangle Select | S | mode, Normal/Fixed Size/Fixed Ratio, clipping |
| 2 | Lasso Select | S×2 | mode, clipping |
| 3 | Ellipse Select | S×3 | mode, clipping |
| 4 | Magic Wand | S×4 / Shift+S | mode, flood, tolerance, alpha mode, sampling, movable click point |
| 5 | Move Selected Pixels | M | resampling |
| 6 | Move Selection | M×2 / Shift+M | |
| 7 | Zoom | Z | |
| 8 | Pan | H | |
| 9 | Paint Bucket | F | flood, tolerance, sampling, fill style, AA, blend |
| 10 | Gradient | G | 7 types, colour/transparency, 3 repeat modes, blend |
| 11 | Paintbrush | B | size (fractional), [pressure], hardness, spacing, smoothing, fill style |
| 12 | Eraser | E | |
| 13 | Pencil | P | 1 px, blend incl. Overwrite |
| 14 | Color Picker | K | sampling, size, after-click |
| 15 | Clone Stamp | L | |
| 16 | Recolor | R | tolerance |
| 17 | Text | T | font, size, style, align |
| 18 | Line / Curve | O | caps, styles |
| 19 | Shapes | O×2 / Shift+O | shape list, draw/fill type, rounded corners |

The order of the icon grid in the window is **(screenshot only)**; the list above is the order of the text sections.

### Paintbrush — [Paintbrush.html](https://paint.net/doc/latest/Paintbrush.html)
Brush size (decimals allowed, achieved via antialiasing), pressure-sensitivity toggle (only shown when a pen is detected), **Hardness** (strength of antialiasing, ignored when AA is off), **Spacing** (% of width), **Smoothing** (default "Smoothed path"), fill style. Ranges and defaults are not stated.

### Gradient — [GradientTool.html](https://paint.net/doc/latest/GradientTool.html)
Types: Linear, Linear (Reflected), Diamond, Radial, Conical, Spiral (Clockwise), Spiral (Anticlockwise). Modes: Color (default), Transparency (alpha only: from the primary's alpha to the inverse of the secondary's alpha). Repeat: No Repeat, Repeat Wrapped, Repeat Reflected. Blend mode as if drawn on its own layer. Nubs stay editable; right-click a nub to swap colours.

### Magic Wand — [MagicWand.html](https://paint.net/doc/latest/MagicWand.html)
Tolerance can be changed while the selection is live (it recomputes from the click point). Sampling: Image or Layer. Shift switches to global temporarily.

## Layers — [LayersMenu.html](https://paint.net/doc/latest/LayersMenu.html), [BlendModes.html](https://paint.net/doc/latest/BlendModes.html)
- Menu: Add New Layer (transparent #00000000, named "Layer N", above active), Delete Layer (not the last one), Duplicate, Merge Layer Down, Toggle Visibility | Import From File (grows canvas if needed) | Flip H, Flip V, Rotate 180°, Rotate/Zoom | Go to Top/Above/Below/Bottom | Move to Top/Up/Down/Bottom | Layer Properties.
- Properties: name, visible, blend mode, opacity 0–255.
- Blend modes (14): Normal, Multiply, Additive, Color Burn, Color Dodge, Reflect, Glow, Overlay, Difference, Negation, Lighten, Darken, Screen, Xor.

## Edit menu — [EditMenu.html](https://paint.net/doc/latest/EditMenu.html)
Undo, Redo, Cut (leaves #FFFFFF at alpha 0), Copy, Copy Merged, Paste, Paste into New Layer, Paste into New Image, Copy Selection (JSON `{"polygonList": ["x,y,x,y,…"]}`), Paste Selection ▸ (combine modes; only "Add (union)" is named), Erase Selection (transparent black), Fill Selection (primary), Invert Selection, Select All, Deselect.

## Image menu — [ImageMenu.html](https://paint.net/doc/latest/ImageMenu.html)
Crop to Selection; Resize (percentage or absolute, keep aspect, gamma correction; resampling: Bicubic (default), Bicubic (Smooth), Bilinear, Bilinear (Low Quality), Adaptive (Sharp), Lanczos, Fant, Nearest Neighbor); Canvas Size (anchor; fill Transparent/Primary/Secondary/White/Black); Flip H/V; Rotate 90° CW/CCW, 180°; Color Profile (convert/assign; sRGB, Adobe RGB, Display P3, ProPhoto; import/export .icc); Flatten.

## Adjustments — [AdjustmentsMenu.html](https://paint.net/doc/latest/AdjustmentsMenu.html)
Auto-Level, Black and White, Brightness/Contrast, Curves, Exposure, Highlights/Shadows, Hue/Saturation, Invert Alpha, Invert Colors, Levels, Posterize (2–64, linked), Sepia, Temperature and Tint. Shortcuts are listed in keybindings.md; Exposure, Highlights/Shadows and Temperature/Tint have none documented.

## Effects — [EffectsMenu.html](https://paint.net/doc/latest/EffectsMenu.html)
Top item: Repeat last (Ctrl+F). Built-ins have no icon; plugins show a puzzle icon.
- **Artistic:** Ink Sketch, Oil Painting, Pencil Sketch
- **Blurs:** Bokeh, Fragment, Gaussian, Median, Motion (edge: clamp/wrap/mirror/transparent), Radial, Sketch, Square, Surface, Zoom
- **Color:** Quantize (Median Cut / Octree, ≤256 colours, 9 dither levels)
- **Distort:** Bulge, Crystalize, Dents, Frosted Glass, Morphology (erode/dilate), Pixelate, Polar Inversion, Tile Reflection, Twist
- **Noise:** Add Noise, Reduce Noise
- **Object:** Drop Shadow (Shadow Radius, Distance, Angle, Opacity, Color, Only Draw Shadow)
- **Photo:** Glow, Red Eye Removal, Sharpen, Soften Portrait, Straighten (±45°), Vignette
- **Render:** Clouds, Julia Fractal, Mandelbrot Fractal, Turbulence
- **Stylize:** Edge Detect (Prewitt/Sobel), Emboss, Outline, Relief

Object effects need an "object": pixels surrounded by transparency
([EffectsObjectMenu.html](https://paint.net/doc/latest/EffectsObjectMenu.html)).

## BoltBait plugin pack (v5.0+) — <https://www.boltbait.com/pdn/>
The page says "36 plugin files with 44 effects" (download BoltBaitPack611.zip); its two lists add up to 46. Parameters aren't on the page, and the forum thread returned 403.

GPU: Adjustment ▸ Black and White Plus, Color Balance, Combined Adjustments, Transparency · Artistic ▸ Dream · Fill ▸ from Clipboard, from File · **Object ▸ Apply Alpha Mask, Bevel Object, Feather Object, Inner Shadow, Outline Object, Switch Alpha to Gray, Switch Gray to Alpha, Drop Shadow Plus** · Photo ▸ Level Horizon / Plumb Bob, Meme Maker, Photo Adjustments, Remove Dust, Vignette Plus · Render ▸ Calendar Creator, Flames · **Selection ▸ Bevel Selection, Blur Selection Edge, Feather Selection, Inner Shadow, Outline Selection** · Text Formations ▸ Text Fun Factory.

Classic: Adjustment ▸ Hue/Sat+ · Flip Horizontal, Flip Vertical · Artistic ▸ Oil Paint+, Pastel · Blurs ▸ Gaussian Blur+ · Color ▸ Complementary Colors · Object ▸ Paste Alpha · Photo ▸ Sharpen Classic, Seam Carving, Sharpen Landscape · Render ▸ Chart/Graph, Dimensions, Gradients, Grid/Checkerboard, Polygon/Stars · Stylize ▸ Edge Detect Classic, Floyd-Steinberg Dithering.

Forum notes: Object ▸ Feather uses transparency, while Selection ▸ Feather uses the marching ants ([forum](https://forums.paint.net/topic/116171-how-to-feather-edges/)). BoltBait rewrote both in pack v5.1, and the new Feather Object needs transparent space around the object at least as wide as the feather depth.
