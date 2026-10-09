# paintnotnet

A faithful paint.net 5.x remake for Linux in Rust (egui/eframe + wgpu). Design: `PLAN.md`.
Phase state: `ROADMAP.md`. Re-read both before starting or resuming a phase.

## Source rules
- Behaviour and maths come from **paint.net 3.36.7 source** (OpenPDN, MIT) at `reference/OpenPDN`
  (git-ignored, read-only) and, for anything newer, the 5.x docs. `docs/reference/` holds the
  extracted 5.x shortcut table and feature inventory.
- When porting OpenPDN code, add a header naming the source file and keep the MIT notice in `NOTICE`.
- Pinta (`reference/Pinta`, MIT) is a secondary reference. Verify anything taken from it against
  OpenPDN or the 5.x docs first; e.g. its blend modes differ from paint.net's. Its UI and tool
  feel are never a reference. Never decompile paint.net 4.x/5.x.
- OpenPDN's icons, `.resx` and text are CC BY-NC-ND: use them unmodified only; never commit
  them outside a clearly marked assets directory with attribution.

## Checks
- `cargo xtask ci`: fmt, clippy `-D warnings`, tests. Must pass before a phase is called done.
- `./target/debug/paintnotnet --smoke-test 5` prints window system, monitor, GPU and frame timings.
- egui_kittest UI tests: kittest feeds one queued event per frame; step once per event.
