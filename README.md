# paintnotnet

A modern paint.net for Linux, written in Rust. Layers with blend modes, selections with live
feathering, painting and shape tools, adjustments and effects, and paint.net's keyboard
shortcuts, drawn on the GPU so large photos stay responsive.

- Opens and saves paint.net `.pdn` and OpenRaster `.ora` files with their layers; PNG, JPEG,
  WebP, BMP, GIF, TIFF and TGA
- Remove Background and Select Subject with a local AI model (nothing is uploaded)
- Docked panels, a command palette (Ctrl+K), dark and light themes, KDE Plasma's global menu

## Install

On Arch Linux and derivatives, build a package from this checkout's last commit:

```sh
git clone https://github.com/MaxJW/paintnotnet.git
cd paintnotnet/packaging/arch
makepkg -si
```

From source on any distribution (Rust stable):

```sh
cargo build --release -p paintnotnet
./target/release/paintnotnet
```

Needs a GPU with Vulkan or OpenGL and a Wayland or X11 session. Open and Save
As use the desktop's file dialogs through `xdg-desktop-portal`.

### Remove Background and Select Subject

These need ONNX Runtime from your distribution (`onnxruntime`, or `onnxruntime-opt-cuda` to use
an NVIDIA GPU). The model (BiRefNet, from Hugging Face) is downloaded the first time, after
asking; Settings > AI models shows and deletes downloaded models.

## Where things are kept

| What | Where |
|---|---|
| Settings, shortcuts, recent files | `~/.config/paintnotnet/settings.toml` |
| Crash recovery (autosave every minute) | `~/.local/state/paintnotnet/recovery/` |
| Session log (for reporting slowness or hangs) | `~/.local/state/paintnotnet/paintnotnet.log` (the previous session's: `.log.1`) |
| AI models | `~/.local/share/paintnotnet/models/` |

The `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and `XDG_DATA_HOME` variables move these as usual.

## Licences

The code is MIT (see `LICENSE`); it includes code ported from Paint.NET 3.36 (MIT). The icons
are Lucide (ISC) and the typeface is Geist (SIL OFL 1.1). Details in `NOTICE`.
