# opencad-raster-plugin — Raster Tools for Open CAD Studio

**English** · [Italiano](README.it.md)

> **Hobby project, made just for fun.** It is not intended for professional use
> and comes with no warranty (see the GPL-3.0 licence): keep a copy of your
> drawings before using it on real work. Early version (0.x): features may
> change. What's new is in the [changelog](CHANGELOG.md) (in Italian); ideas and
> bug reports are welcome in the [Issues](https://github.com/Macchecaspita/opencad-raster-plugin/issues).

A plugin for [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
that adds a **Raster** ribbon tab for GIF, JPG, PNG, BMP, TIFF and WebP images
in DWG/DXF drawings:

- **insert** an image, sized on screen, by width, by DPI or 1 pixel = 1 unit;
- **clip** an image (keep the inside), cut a **hole** (hide the inside) or
  **split a piece** off into a new image you can move — with a **rectangle**,
  a **polygon** drawn by clicks, or an existing **closed polyline**, circle or
  ellipse;
- **resize** by dragging a corner, or to an exact width / height / scale
  factor / **DPI**;
- make **one colour transparent** (e.g. a scan's white background), picked
  with a click on the image;
- **change the file path** on disk and **find** images whose file was moved;
- **list** every image with its path, pixels, size and file status.

The interface is in **English**, or in **Italian** when Open CAD Studio runs in
Italian. `RT_LANG IT|EN|AUTO` forces a language.

Full command reference: [PLUGIN.md](PLUGIN.md).

## Quick guide

| I want to… | I do… |
|---|---|
| insert a photo or a scan | **Insert image**, pick the file and the size, click the point |
| trim a photo's edges | **Clip**, click the image, two corners of the area to keep |
| hide an area | **Hole**, click the image, two corners |
| clip along an irregular outline | **Clip ▾ › Polygon**, click the image, click the vertices, Enter |
| clip along a polyline already drawn | select image and polyline → **Clip ▾ › From polyline** |
| cut out a detail and move it | **Split piece**, then `MOVE` the piece (it stays selected) |
| bring a scan to 1:1 scale | select the image → **Size / DPI** → *DPI* mode, 300, millimetres |
| enlarge by eye | **Drag**: fixed corner, then opposite corner |
| drop a scan's white background | **Transparent colour**, click the image, click the white, *only the connected area* |
| the photo was moved to another drive | **Find in folder** and pick a file in the new folder |
| replace the photo with another | select the image → **Change path** → pick the file |

## Install

In Open CAD Studio's **Plugin Manager** (command `PLUGINS`):
**Add repository →** `Macchecaspita/opencad-raster-plugin`, pick the latest
version and press *Install*. Published versions are in
[Releases](https://github.com/Macchecaspita/opencad-raster-plugin/releases).

Or by hand: copy the built library **and** `plugin.toml` into

- Windows: `%APPDATA%\OpenCADStudio\plugins\opencad.rastertools\`
- macOS: `~/Library/Application Support/OpenCADStudio/plugins/opencad.rastertools/`
- Linux: `~/.config/OpenCADStudio/plugins/opencad.rastertools/`

and restart Open CAD Studio.

## Build

Needs Rust (https://rustup.rs). From the project folder:

```
cargo test --release
cargo build --release
```

The result is `target/release/opencad_raster_plugin.dll` (Windows),
`libopencad_raster_plugin.so` (Linux) or `libopencad_raster_plugin.dylib` (macOS).

## Publishing a new version

1. Write what's new in [CHANGELOG.md](CHANGELOG.md), under **[Non rilasciato]**.
2. In PowerShell, in the project folder:

   ```
   powershell -ExecutionPolicy Bypass -File scripts\release.ps1 0.5.0
   ```

The script sets the version in `Cargo.toml`, `plugin.toml` and `CHANGELOG.md`,
builds and tests the plugin, then — after you confirm — commits, tags and
pushes. The `release.yml` workflow builds for Windows, Linux and macOS and
publishes the release with notes taken from the changelog.

Numbering: `0.4.1` for fixes only, `0.5.0` for new features, `1.0.0` once the
plugin is considered stable.

## Compatibility

Built against `ocs_plugin_api` from tag **v2026.45** (Plugin API 16). Each
plugin build works only with the Open CAD Studio release it was built for (the
CAD checks that plugin and host exchange drawing data in the same format):
for **v2026.44 use plugin 0.5.1**. For a later CAD release, set `rev` in
`Cargo.toml` to that release tag's commit and `api_version` in `plugin.toml`.

## Layout

```
src/lib.rs          manifest, plugin entry point
src/i18n.rs         English / Italian choice
src/ribbon.rs       Raster ribbon tab
src/commands.rs     RT_* commands and drawing edits
src/tools.rs        interactive tools (pick, rectangle, polygon, drag) and dialogs
src/geom.rs         pure geometry: pixel ↔ drawing, clipping, scaling (tested)
src/imgsize.rs      pixel size from the file header (tested)
src/pixels.rs       transparent colour on RGBA pixels (tested)
scripts/release.ps1 publish a new version
CHANGELOG.md        changelog
icons/              ribbon SVG icons
```

Licence: GPL-3.0-only, like Open CAD Studio.
