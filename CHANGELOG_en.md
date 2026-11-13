# Changelog

All notable changes to Raster Tools are recorded here.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [Semantic Versioning](https://semver.org/) —
`0.x.y` indicates an initial version, still in testing.

The release notes of each GitHub release are taken automatically from the section
of the corresponding version. Write new items under **[Unreleased]**:
the `scripts/release.ps1` script moves them into the new version at release time.

## [Unreleased]

## [0.10.1] - 2026-11-14

### Fixed
- `RT_OSM`: the tile cache is decoded from the file content, so JPEG/WebP tiles
  (typical of orthophotos) work even when saved with a .png extension.
- `RT_OSM`: with a URL other than OpenStreetMap, the layer is `BASEMAP`, the file is named
  `map_epsg…` and the credit text is "Map tiles: <host>" instead of "© OpenStreetMap contributors".

## [0.10.0] - 2026-11-13

### Added
- **Base map** (`RT_OSM`, Insert tab): select a rectangle (closed polyline),
  enter the EPSG code of the drawing (UTM, NZTM, etc.) and the plugin downloads the
  OpenStreetMap tiles, reprojects them into a north-oriented raster in the drawing's
  coordinate system and inserts it (PNG + world file next to the drawing, layer `OSM`, behind
  the drawing) with the credit "© OpenStreetMap contributors". Pixel size automatic or chosen,
  tile limit (64 by default), configurable tile source, 7-day cache.
  Quick form: `RT_OSM <EPSG> [pixel] [max-tiles]`.

## [0.9.4] - 2026-11-13

### Added
- **Alpha on** (`RT_ALPHAON`, Transparency tab): turns on the transparency flag on the
  selected images, or on all images in the drawing if nothing is selected, so the alpha
  channel of 32-bit PNG/TIFF files is used. It only changes the flag: no file is written
  or copied. `RT_TRANSPOFF` turns it off.

## [0.9.3] - 2026-11-13

### Changed
- Images inserted with `RT_ATTACH` and `RT_FOLDER` now have **transparency
  on**: the alpha channel of 32-bit PNG/TIFF files is respected on screen and in
  prints/PDFs (previously transparent areas were drawn opaque, usually black).
  For images already in the drawing, use `RT_TRANSP`; `RT_TRANSPOFF` turns it off.

## [0.9.2] - 2026-11-13

### Added
- **GeoTIFF**: for `.tif`/`.tiff` files, georeferencing is read from the tags in the
  file header (pixel scale + tie point, or transformation matrix; classic TIFF and BigTIFF;
  PixelIsArea/PixelIsPoint). Applies to *World file* in `RT_ATTACH`, `RT_WORLD` and
  `RT_FOLDER`. The `.tfw` next to the file is only needed if the TIFF has no geographic tags.
  Multiple control points (GCPs) are not supported.

## [0.9.1] - 2026-11-13

### Added
- **Insert folder** (`RT_FOLDER`, Insert tab): choose any image in the folder and
  all the images in that folder are inserted, each where its world file says, on the
  RASTER layer (created if missing), behind the drawing; then zoom extents. Images without a
  world file and those already in the drawing are skipped and reported. A single undo step.

## [0.9.0] - 2026-11-13

### Added
- **World files** (`.pgw`, `.tfw`, `.jgw`, `.gfw`, `.bpw`, `.wld`): in **Insert
  image** (`RT_ATTACH`) a new size mode *World file*: the image goes
  straight where the file next to it says (position, scale and any rotation), on the
  chosen layer, without clicks. The file is looked up in the same folder with the same
  name (case-insensitive).
- **From world file** (`RT_WORLD`, Position tab): puts images already in the drawing
  back in place (selected, or clicked) by reading the world file; clips are kept.

## [0.8.0] - 2026-11-11

### Added
- **Align 2 points** (`RT_ALIGN`): georeferences an image. Click a recognizable point
  on the image and where it should go, then a second point and its destination:
  the image moves, rotates and scales by itself. Enter after the first
  pair = move only.
- **Rotate** (`RT_ROTATE`): with the mouse (centre, reference point, new
  direction: aligns to a line) or by an exact angle (Enter).
- **Inserting a PDF** (`RT_ATTACH`): from a PDF, for example a cadastral map
  extract, the plugin extracts the image it contains at its resolution and saves it
  as `<name>_pdf.png`. New **Map scale 1:** field for the DPI mode: an
  extract at 300 DPI at scale 1:2000 comes in at its real size in metres.
- **Send to back** (`RT_BACK`): images go behind the drawing's lines; images
  inserted with `RT_ATTACH` go there automatically.
- **Clean up copies** (`RT_PURGE`): moves to a `_trasp-non-usate` subfolder the
  transparent copies that no image in the drawing uses any more.
- **Guide** (`RT_GUIDE`): button with a short guide to all the commands.

### Changed
- The diagnostic log is now **off by default**: `RT_DIAG ON` / `RT_DIAG OFF`.

## [0.7.0] - 2026-11-09

### Added
- **Colour the map with the layer colour** (`RT_TRANSP`). In the
  "Transparent colour" window there is the new option *Colour the rest with the
  layer colour*: the background disappears and everything else takes the colour of the
  image's layer, with soft edges. The black lines of a cadastral map
  become white, for example, and clearly readable on a dark background, without the light halo
  and the "jagged" lines that transparency alone left behind. It is on by default
  when the chosen colour is light (the paper of a scan).
- **Layer of the inserted image** (`RT_ATTACH`). The insert window asks for the
  layer: **RASTER** is proposed, created (white) if it does not exist in the drawing,
  or another layer of your choice.

## [0.6.2] - 2026-11-08

### Added
- Diagnostic log `rastertools-log.txt` in Open CAD Studio's settings folder:
  the commands received by the plugin and the steps of `RT_TRANSP`, to
  understand where a command stops when the command line shows nothing.

## [0.6.1] - 2026-11-08

### Fixed
- **The "Make transparent" button did nothing.** The window opened, but
  the CAD closed it immediately "behind the scenes" and the button had no effect.
  The same flaw could also block other steps opened after choosing
  the image: "Insert image" (choosing the point) and "Size / DPI" or
  "Change path" without an image selected beforehand.

## [0.6.0] - 2026-11-08

### Changed
- **Compatible with Open CAD Studio 2026.45** (Plugin API 16). 2026.45 rejects
  plugins built for the previous version (at startup it gave an error on
  plugins). This version works **only with 2026.45**: if you are still on
  2026.44, stay on 0.5.1.

## [0.5.1] - 2026-11-08

### Fixed
- **Transparent colour on large scans.** With very large images (for example
  a 7000 × 10000 pixel cadastral map) the command stopped without
  messages: saving the PNG copy took longer than the 30 seconds that
  Open CAD Studio allows a plugin command. Saving is now much faster, the plugin
  keeps the CAD informed while it works and shows a "please wait…" message for
  large images; it also uses less memory.
- If the same image is inserted several times in the drawing (overlapping copies),
  `RT_TRANSP` relinks all copies of the same file to the transparent copy:
  previously a still-opaque copy could cover the one made transparent.

## [0.5.0] - 2026-11-08

### Added
- **Transparent colour** (`RT_TRANSP`, Raster › Transparency tab): choose the
  image, then click **directly on the image** the colour to make transparent (for example
  the white background of a scan). A window shows the chosen colour and asks for the
  **tolerance** (for similar colours; 10% proposed for
  JPG, 2% for other formats) and whether to act on the whole image or **only on the area
  connected to the clicked point**. The plugin saves a PNG copy with the transparency
  next to the original (`<name>_trasp-N.png`, the original is not touched),
  relinks the image to the copy and turns on transparency. The command can be
  repeated to remove other colours.
- `RT_TRANSPOFF` (Remove transparency): turns off transparency on images.

## [0.4.0] - 2026-11-05

### Added
- **English and Italian.** The plugin speaks English, or Italian when Open CAD
  Studio is in Italian (with the "System" setting, when Windows is in Italian).
  Messages, prompts, windows and toolbar labels follow the language; the new
  command `RT_LANG IT|EN|AUTO` forces it manually.
- README and command reference in English (`README.md`, `PLUGIN.md`) and in
  Italian (`README.it.md`, `PLUGIN.it.md`), with the "hobby project" disclaimer.

## [0.3.1] - 2026-11-05

### Fixed
- The build on GitHub no longer worked: the DWG library `acadrust`, which
  Open CAD Studio depends on, was moved to another repository. The plugin now
  downloads it from the new address (same version, no feature changes).
  This version also publishes the image insertion of 0.3.0.

## [0.3.0] - 2026-11-05

### Added
- **Image insertion** into the drawing (`RT_ATTACH`, *Insert image* button):
  choose the file (GIF, JPG, PNG, BMP, TIFF, WebP) and the size — indicated on
  screen with two points (proportional), by width, by DPI resolution or
  1 pixel = 1 unit — then click the insertion point. The image goes on the
  current layer and the file stays linked; the unit for DPI is proposed based on the
  drawing units.

## [0.2.0] - 2026-11-04

### Added
- Clipping, hole and split with a **polygon** drawn by clicking (`RT_CLIPPOLY`,
  `RT_HOLEPOLY`, `RT_CUTPOLY`): Enter or a click on the first vertex closes the polygon.
- Clipping, hole and split with a **closed polyline**, a circle or an ellipse
  already in the drawing (`RT_CLIPPL`, `RT_HOLEPL`, `RT_CUTPL`); arc segments
  of polylines are followed faithfully. Choose the polyline by clicking it or
  by clicking inside it; if the image and polyline are already selected, the clip
  is immediate.
- In the Raster tab the Clip, Hole and Split piece buttons become menus with
  three choices: Rectangle, Polygon, From polyline.

### Changed
- A new clip on an already clipped image narrows the clip also with
  polygons (if both outlines are concave, the new one replaces the old one).
- Outlines that intersect themselves are rejected with a clear message.

## [0.1.0] - 2026-11-04

First public version, in testing.

### Added
- Rectangular clip that keeps the inside (`RT_CLIP`); repeated, it narrows the clip.
- Rectangular hole that hides the inside (`RT_CLIPHOLE`).
- Splitting a piece off into a new movable image (`RT_CUT`).
- Removing the clip (`RT_UNCLIP`).
- Proportional resizing by dragging a corner (`RT_RESIZE`).
- Resizing by width, height, scale factor or DPI resolution (`RT_SIZE`).
- Changing the file on disk, keeping the width in the drawing (`RT_PATH`).
- Searching a folder for moved files (`RT_FIND`).
- Listing images with path, pixels, dimensions and file status (`RT_LIST`).
- Formats: GIF, JPG, PNG, BMP, TIFF, WebP.
- Compatible with Open CAD Studio v2026.44 (Plugin API 14).
