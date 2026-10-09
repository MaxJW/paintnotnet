# paint.net 5.x keyboard and mouse commands (reference)

Source: <https://paint.net/doc/latest/KeyboardMouseCommands.html> (page revision 3 Aug 2024;
the /doc/latest index covers Paint.NET 5). Fetched 2026-10-09. Rows follow the page's
grouping and order. Spacing typos on the page ("Ctrl + Shift+ Delete") are normalised.

This file is the **source of truth for the default keymap**. Phase 2 adds a conformance test
that parses these tables and checks every chord resolves to a command.

Multi-tap rule (from the page): when several tools share a key, pressing it again cycles them.
"×3" means press three times; presses must be under one second apart.

## UI / Canvas controls
| Keys / Mouse | Action |
|---|---|
| Hold Space + drag LMB | Pan (not while typing in Text tool) |
| Wheel up/down, PgUp/PgDn | Scroll up/down |
| Shift+Wheel up, Shift+PgUp, Home ×1 | Scroll left |
| Shift+Wheel down, Shift+PgDn, End ×1 | Scroll right |
| Home ×2, Shift+Home | Scroll image to top-left of editing window |
| Ctrl+Home | Scroll top-left of image to centre |
| End ×2, Shift+End | Scroll image to bottom-right of editing window |
| Ctrl+End | Scroll bottom-right of image to centre |
| Ctrl+Wheel; Ctrl+- / Ctrl++; Ctrl+0 (actual size); status-bar slider/box; View menu | Zoom |
| Ctrl+B (twice) | First: zoom to window; second: restore previous zoom |
| Space+LMB; Space+Arrows; MMB drag | Pan canvas |
| Space+Ctrl+Arrows | Pan canvas ×10 |
| Ctrl+D, Enter | Deselect |
| Arrows (tool-dependent) | Shift current tool 1 px |
| Ctrl+Arrows (tool-dependent) | Shift current tool 10 px |
| Alt | Show menu hotkeys |
| Alt + underlined letter | Use menu hotkey |
| F5 / F6 / F7 / F8 | Show/hide Tools / History / Layers / Colors window |
| Esc, Enter, Ctrl+D, toolbar **Finish** | Commit text or tool drawing to layer |

## Image list and navigation
| Keys | Action |
|---|---|
| Ctrl+PgDn, Ctrl+Tab | Next image |
| Ctrl+PgUp, Ctrl+Shift+Tab | Previous image |
| Ctrl+Shift+PgUp | Move image tab left |
| Ctrl+Shift+PgDn | Move image tab right |
| Ctrl+*N*, Alt+*N* | Switch to image number *N* |
| Alt+- | Context menu for current image |

## Tool bar
| Keys | Action |
|---|---|
| Alt+T | Open tool dropdown |
| [ / ] | Brush width −1 / +1 |
| Ctrl+[ / Ctrl+] | Brush width −5 / +5 |
| Wheel over brush width box | Step through width presets |
| Up/Down in brush width box | Step through width presets |

## File menu
| Keys | Action |
|---|---|
| Alt+F | Open File menu |
| Ctrl+N | New image |
| Ctrl+O | Open |
| Alt+F, R | Open Recent |
| Alt+F, Q | Acquire |
| Ctrl+W, Ctrl+F4 | Close image |
| Ctrl+S | Save |
| Ctrl+Shift+S | Save As |
| Ctrl+Alt+S | Save All |
| Ctrl+P | Print |
| Alt+F4, Alt+F then X | Exit |

## Edit menu
| Keys | Action |
|---|---|
| Alt+E | Open Edit menu |
| Ctrl+Z | Undo |
| Ctrl+Y | Redo |
| Ctrl+X, Shift+Delete | Cut |
| Ctrl+C, Ctrl+Insert | Copy |
| Ctrl+Shift+C | Copy Merged |
| Ctrl+V, Shift+Insert | Paste |
| Ctrl+Shift+V | Paste into New Layer |
| Ctrl+Alt+V | Paste into New Image |
| Ctrl+Alt+Shift+C | Copy Selection |
| Ctrl+Alt+Shift+V | Paste Selection (Replace) |
| Delete | Erase Selection |
| Backspace | Fill Selection |
| Ctrl+I | Invert Selection |
| Ctrl+A | Select All |
| Ctrl+D, Enter | Deselect |

## View menu
| Keys | Action |
|---|---|
| Alt+V | Open View menu |
| Ctrl++ | Zoom In |
| Ctrl+- | Zoom Out |
| Ctrl+B | Zoom to Window |
| Ctrl+Shift+B | Zoom to Selection |
| Ctrl+0 | Actual Size |

## Image menu
| Keys | Action |
|---|---|
| Alt+I | Open Image menu |
| Ctrl+Shift+X | Crop to Selection |
| Ctrl+R | Resize |
| Ctrl+Shift+R | Canvas Size |
| Ctrl+H | Rotate 90° clockwise |
| Ctrl+G | Rotate 90° counter-clockwise |
| Ctrl+H ×2, Ctrl+G ×2 | Rotate 180° |
| Ctrl+Shift+F | Flatten |

## Layers menu
| Keys | Action |
|---|---|
| Alt+L | Open Layers menu |
| Ctrl+Shift+N | Add New Layer |
| Ctrl+Shift+Delete | Delete Layer |
| Ctrl+Shift+D | Duplicate Layer |
| Ctrl+M | Merge Layer Down |
| Ctrl+, | Toggle Layer Visibility |
| Ctrl+Shift+Z | Rotate / Zoom |
| Ctrl+Alt+PgUp | Go to Top Layer |
| Alt+PgUp | Go to Layer Above |
| Alt+PgDn | Go to Layer Below |
| Ctrl+Alt+PgDn | Go to Bottom Layer |
| Ctrl+click move-up arrow | Move active layer to top |
| Ctrl+click move-down arrow | Move active layer to bottom |
| F4 | Layer Properties |

## Adjustments menu
| Keys | Action |
|---|---|
| Alt+A | Open Adjustments menu |
| Ctrl+Shift+L | Auto-Level |
| Ctrl+Shift+G | Black & White |
| Ctrl+Shift+T | Brightness / Contrast |
| Ctrl+Shift+M | Curves |
| Ctrl+Shift+U | Hue / Saturation |
| Ctrl+Alt+I | Invert Alpha |
| Ctrl+Shift+I | Invert Colors |
| Ctrl+L | Levels |
| Ctrl+Shift+P | Posterize |
| Ctrl+Shift+E | Sepia |

## Effects menu
| Keys | Action |
|---|---|
| Alt+C | Open Effects menu |
| Ctrl+F | Repeat last effect |

## Settings / Help
| Keys | Action |
|---|---|
| Alt+X | Settings dialog |
| Alt+H | Open Help menu |
| F1 | Online documentation |

## Utility windows
| Keys | Action |
|---|---|
| F5 / F6 / F7 / F8 (or the window's icon) | Toggle Tools / History / Layers / Colors |
| Ctrl+Shift+F5 / F6 / F7 / F8 (or Ctrl+Shift+icon) | Reset that window's position and size |
| (History) Ctrl+Z / Ctrl+Y | Undo / Redo |
| (Layers) click a layer | Make it the active layer |

## Colors window
| Keys / Mouse | Action |
|---|---|
| X | Swap primary and secondary |
| C | Switch active colour slot |
| LMB on wheel | Set ACTIVE colour slot |
| RMB on wheel | Set INACTIVE colour slot |
| Ctrl+drag selector | Change hue only |
| Alt+drag selector | Change saturation only |
| Shift+drag selector | Change saturation, snap hue to 15° |
| Ctrl+Shift+drag selector | Snap hue to 15° |

## Tool shortcuts
| Keys | Tool |
|---|---|
| S, Shift+S ×4 | Rectangle Select |
| S ×2, Shift+S ×3 | Lasso Select |
| S ×3, Shift+S ×2 | Ellipse Select |
| S ×4, Shift+S | Magic Wand |
| M | Move Selected Pixels |
| M ×2, Shift+M | Move Selection |
| Z | Zoom |
| H (Space to activate temporarily) | Pan |
| F | Paint Bucket |
| G | Gradient |
| B | Paintbrush |
| E | Eraser |
| P | Pencil |
| K | Color Picker |
| L | Clone Stamp |
| R | Recolor |
| T | Text |
| O | Line / Curve |
| O ×2, Shift+O | Shapes |

## Selection tools (not Magic Wand)
| Keys / Mouse | Action |
|---|---|
| Drag LMB or RMB | Create selection |
| Ctrl+drag LMB | Add (union) |
| Alt+drag LMB | Subtract |
| Ctrl+drag RMB | Invert (xor) |
| Alt+drag RMB | Intersect |
| Shift+drag | Constrain rectangle to square / ellipse to circle |
| Drag with LMB+RMB together | Move selection while drawing |
| Move Selected Pixels + Ctrl | Move a copy |

## Magic Wand
| Keys / Mouse | Action |
|---|---|
| LMB | Select similar pixels |
| Ctrl+LMB | Add |
| Alt+LMB | Subtract |
| Ctrl+RMB | Invert |
| Alt+RMB | Intersect |
| Shift + any of the above | Global flood mode |
| Drag four-arrow nub | Move click point |

## Move tools
| Keys / Mouse | Action |
|---|---|
| Drag corner/edge nub | Resize |
| Shift+drag corner nub | Resize keeping proportions |
| Alt+drag corner nub | Resize about centre |
| Shift+Alt+drag corner nub | Resize about centre, keep proportions |
| Drag (four-way arrow pointer) | Move |
| Arrows / Ctrl+Arrows | Nudge 1 px / 10 px |
| Ctrl+drag LMB | Move a copy |
| Drag LMB (double-arrow pointer) or drag RMB | Rotate |
| Ctrl + the above | Rotate a copy |
| Shift while rotating | 15° steps |

## Zoom tool
| Mouse | Action |
|---|---|
| LMB | Zoom in |
| RMB | Zoom out |
| MMB drag / wheel | Pan while zoomed |

## Pan tool
| Mouse / Keys | Action |
|---|---|
| Drag LMB or RMB; hold button + Arrows | Pan |
| Move mouse or Arrows | Reposition pan tool |

## Paint Bucket
| Keys / Mouse | Action |
|---|---|
| LMB or Backspace | Fill with primary |
| RMB or Shift+Backspace | Fill with secondary |
| Shift (in Contiguous) | Global fill |
| Shift (in Global) | Contiguous fill |
| Drag four-arrow nub | Move click point |

## Gradient
| Keys / Mouse | Action |
|---|---|
| Drag LMB | Primary → secondary |
| Drag RMB | Secondary → primary |
| Drag either nub | Move start/end |
| RMB on a nub | Swap colour roles |
| Drag four-arrow nub | Move gradient |
| Shift while dragging a nub | 15° steps |

## Paintbrush / Pencil
| Mouse | Action |
|---|---|
| LMB | Draw in primary |
| RMB | Draw in secondary |

## Eraser
| Mouse | Action |
|---|---|
| Either button | Erase |

## Color Picker
| Keys / Mouse | Action |
|---|---|
| LMB | Set primary |
| RMB | Set secondary |
| Hold Ctrl while clicking | Sample composite image |

## Recolor
| Mouse | Action |
|---|---|
| Drag LMB | Replace secondary with primary |
| Drag RMB | Replace primary with secondary |

## Clone Stamp
| Keys / Mouse | Action |
|---|---|
| Ctrl+LMB | Set source point |
| Drag LMB or RMB | Copy pixels |

## Text
| Keys / Mouse | Action |
|---|---|
| Esc or Finish | Commit text |
| Drag four-arrow nub | Move text before commit |

## Line / Curve
| Keys / Mouse | Action |
|---|---|
| Alt while dragging | Draw from centre |
| , | Start cap style |
| . | Line style |
| / | End cap style |
| Drag nub (either button), or hold nub + Arrows | Move nubs |
| Drag four-way arrow (LMB) | Move whole line |
| Arrows / Ctrl+Arrows | Move 1 px / 10 px |
| Drag RMB | Rotate |
| Shift while rotating | 15° steps |

## Shapes
| Keys / Mouse | Action |
|---|---|
| Drag LMB / RMB | Draw in primary / secondary |
| A / Shift+A | Next / previous shape |
| Drag nub (either button), or hold nub + Arrows | Move nubs |
| Shift while dragging a nub | Lock aspect ratio |
| Alt while dragging a nub | Resize about centre |
| Shift+Alt while dragging a nub | Both |
| Drag four-way arrow or drag inside shape (LMB) | Move shape |
| Arrows / Ctrl+Arrows | Move 1 px / 10 px |
| Drag RMB | Rotate |
| Shift while rotating | 15° steps |
| Ctrl+click corner −/+ (rounded rectangle) | Change radius by 5 px |

## Diagnostics
| Keys | Action |
|---|---|
| Ctrl+Alt+Shift+~ | Diagnostic aid (paint.net: forces GC, dumps GPU memory). We map it to our own debug overlay. |

## paintnotnet additions (not in paint.net, keys checked against the list above)
| Keys | Action |
|---|---|
| Shift+F6 | Edit > Feather Selection… |
| Ctrl+K | Command palette |
