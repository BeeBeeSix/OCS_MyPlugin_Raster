# Raster Tools (`opencad.rastertools`)

**English** · [Italiano](PLUGIN.it.md)

External add-on for [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
that handles the raster images linked to a drawing (IMAGE entities):
GIF, JPG, PNG, BMP, TIFF and WebP — the same formats the CAD displays.

Every command starts with `RT_`. The **Raster** ribbon tab groups them in five
panels: *Insert*, *Clip*, *Transparency*, *Size*, *File*. In the *Clip* panel the *Clip*,
*Hole* and *Split piece* buttons have a menu (▾ arrow) to choose the outline:
**Rectangle**, **Polygon** or **From polyline**; clicking the button itself uses
the rectangle.

The outline is drawn over the image: parts outside the frame are trimmed away.
Outlines whose sides cross are refused. Every change is a single undo step
(`U` / Ctrl+Z).

## Language

Messages, prompts, dialogs and ribbon labels are in **English**, or in
**Italian** when Open CAD Studio's interface language is Italian (with the
*System* setting, when the operating system is in Italian). The plugin reads the
language from Open CAD Studio's `settings.json` before every command, so
messages follow a change at once; ribbon labels follow it after a restart.

`RT_LANG IT`, `RT_LANG EN` force a language; `RT_LANG AUTO` goes back to
following Open CAD Studio; `RT_LANG` alone shows the current setting. The
choice is kept in `rastertools-lang.txt` in Open CAD Studio's settings folder.

## Commands

| Command | Ribbon | What it does |
|---|---|---|
| `RT_ATTACH` | Insert › Insert image | **Inserts a new image** into the drawing. A dialog asks for the file and how to size it: *on screen* (insertion point and opposite corner, proportional; Enter at the second point = 1 pixel per unit), *width in the drawing*, *DPI resolution* (with the drawing unit, suggested from `INSUNITS`) or *1 pixel = 1 unit*. Then click the insertion point (bottom-left corner). The dialog also asks for the **layer**: **RASTER** is proposed first (created, white, if the drawing has none), or any other layer of the drawing. The file stays linked, it is not copied into the drawing. |
| `RT_CLIP` | Clip › Clip | Rectangular clip: pick the image (or select it first), then two corners. **Only the inside** of the rectangle stays visible. Repeated on an image already clipped, it **narrows** the clip (intersection). If the image is rotated, the rectangle drawn on screen becomes a polygonal outline. |
| `RT_CLIPHOLE` | Clip › Hole | As above, but **hides the inside** of the rectangle (inverted clip). |
| `RT_CUT` | Clip › Split piece | "Scissors": the part inside the rectangle becomes a **new image** (same file, clipped to the rectangle) and the original keeps a hole in its place. The piece stays selected: move it with `MOVE`. Needs an image that is not clipped yet. |
| `RT_CLIPPOLY` / `RT_HOLEPOLY` / `RT_CUTPOLY` | Clip / Hole / Split piece › Polygon | Like the three rows above, but with a **polygon** of any shape, concave too ("L", star…): pick the image and click the vertices; **Enter** or a click near the first vertex closes the polygon (at least 3 vertices). |
| `RT_CLIPPL` / `RT_HOLEPL` / `RT_CUTPL` | Clip / Hole / Split piece › From polyline | As above, using as outline a **closed polyline** (arc segments too), a **circle** or an **ellipse** already drawn: pick the image, then click the polyline or a point inside it. If image and polyline are already selected, the clip is immediate. Arcs are approximated with 3° segments. |
| `RT_UNCLIP` | Clip › Remove clip | Removes the clip from the selected images (or the one you pick). |
| `RT_TRANSP` | Transparency › Transparent colour | Makes **one colour of the image transparent**. Pick the image (or select it first), then **click on the image** the colour to drop (an orange crosshair follows the cursor). A dialog shows the colour (hex and RGB) and asks for the **tolerance** in % (0 = that exact colour only; 10% is suggested for JPG/WebP, whose colours drift slightly, 2% for the rest) and whether to act on the **whole image** or **only the area connected to the clicked point** (handy to drop the outer background while the same colour inside the drawing stays). The plugin saves a **PNG copy** with the transparency in the original's folder, `<name>_trasp-N.png` (the original is not changed), relinks the image to the copy and turns transparency on. Run it again to drop more colours: each run makes a new copy. Option **Colour the rest with the layer's colour** (on by default when the picked colour is light, e.g. a scan's paper): the background disappears and everything else takes the colour of the image's layer, with soft edges — black lines on a white map become, say, white lines that stand out on the CAD's dark background, without the pale fringe left by plain transparency. It applies to the whole image (the area option is ignored); to change the colour later, change the layer's colour and run it again on the original. Undo with `U` / Ctrl+Z. |
| `RT_TRANSPOFF` | Transparency › Transparency off | Turns transparency off on the selected images (or the one you pick): the dropped colours show again. To go back to the original file use `RT_PATH`. |
| `RT_RESIZE` | Size › Drag | Proportional on-screen resize: pick the image, click near the corner that must **stay fixed**, then move the opposite corner to its new position (orange preview). Corners are those of the visible part. |
| `RT_SIZE` | Size › Size / DPI | Dialog with: width or height (proportional), free width and height, scale factor, or **print resolution** (DPI) with the drawing unit (mm, cm, m, inches). Fixed point: bottom-left corner or centre. Option to measure the visible part. Works on every selected image. |
| `RT_PATH` | File › Change path | Changes the image's **file on disk**. From the ribbon the file picker opens at once and the path applies to the selected images (if none, you pick the image afterwards). Typed without arguments it opens a dialog with the current path and its status. Every image using the same file is relinked; the **width in the drawing stays the same** even if the new file has a different pixel count (any clip is rescaled). |
| `RT_FIND` | File › Find in folder | For images whose file is **missing**: pick any file in the folder where they are now and the plugin relinks each image to the file with the same name, subfolders included. |
| `RT_LIST` | File › List | Lists the images: handle, name, pixels, drawing size, pixel size, clip, file status (`found` / `MISSING`) and full path. |
| `RT_LANG` | — | Interface language: `IT`, `EN` or `AUTO` (see above). |
| `RT_HELP` | — | Command summary. |

### Finding the file

An image counts as *found* if the stored path exists, or exists relative to the
drawing's folder, or the drawing's folder holds a file with the same name.

### Path saved in the DWG

In a DWG the path lives on the image *definition* (IMAGEDEF), shared by all its
copies. `RT_PATH` and `RT_FIND` update the entities at once (the view refreshes)
and then move the definition with the host command
`XREF Path <name> <new path>`, so the new path is saved. If scripted commands
are turned off (`SCRIPTCOMMANDS`) the plugin prints the exact line to run by
hand. For relative or absolute paths use `XREF Pathtype`.

## Apply commands (scripts)

Interactive tools end by handing the host a command line that performs the
edit. The same lines work in a script. Handles are hexadecimal; several
handles are separated by commas.

| Line | Effect |
|---|---|
| `RT_CLIPAPPLY <h> KEEP\|HOLE\|CUT <x1> <y1> <x2> <y2> [z]` | Clip with the rectangle (drawing coordinates). |
| `RT_CLIPPOLYAPPLY <h> KEEP\|HOLE\|CUT <x1> <y1> <x2> <y2> <x3> <y3> ...` | Clip with a polygon (drawing coordinates, at least 3 vertices). |
| `RT_UNCLIPAPPLY <h,...>` | Removes the clip. |
| `RT_SCALEAPPLY <h> <anchor_x_px> <anchor_y_px> <factor_x> [factor_y]` | Scales around a point given in image pixels (0,0 = insertion corner). |
| `RT_SIZEAPPLY <h,...> W\|H\|WH\|F\|DPI <width> <height> <factor> <dpi> MM\|CM\|M\|IN BL\|CTR FULL\|VIS` | Like the `RT_SIZE` dialog. E.g.: `RT_SIZEAPPLY 2A DPI 0 0 0 300 MM BL FULL`. |
| `RT_SIZEDLG <h,...>` / `RT_PATHDLG <h,...>` | Opens the dialog for those images. |
| `RT_TRANSPAPPLY <h> <x_px> <y_px> <tolerance_%> ALL\|AREA [TINT]` | Makes the colour of that file pixel (`x`, `y` from the top left) transparent, on the whole image or the connected area only; `TINT` colours the rest with the layer's colour. |
| `RT_TRANSPOFFAPPLY <h,...>` | Turns transparency off. |

`RT_PATHAPPLY`, `RT_FINDAPPLY`, `RT_ATTACHPLACE` and `RT_ATTACHAPPLY` use the
path chosen in the dialog just closed (paths never travel through the command
line, which may upper-case them).

## Saved data

The plugin adds no XDATA or objects of its own: it only changes standard IMAGE
fields (insertion point, U/V vectors, pixel size, clip outline and mode, path),
so the drawing stays readable by any DWG/DXF program without the plugin.
`RT_TRANSP` also writes the PNG copy `<name>_trasp-N.png` next to the original
file and sets the image's standard "transparency" flag.

## Build and install

```
cargo test --release     # geometry, header readers and language tests
cargo build --release    # → target/release/{lib,}opencad_raster_plugin.{dll,so,dylib}
```

Manual install: copy the library and `plugin.toml` into

* Windows: `%APPDATA%\OpenCADStudio\plugins\opencad.rastertools\`
* macOS: `~/Library/Application Support/OpenCADStudio/plugins/opencad.rastertools/`
* Linux: `~/.config/OpenCADStudio/plugins/opencad.rastertools/`

and restart the CAD. Or publish a GitHub release (the `release.yml` workflow
does it when a `v*` tag is pushed) and install it from
**Plugin Manager → Add repository**.

Compatibility: Plugin API 16, Open CAD Studio v2026.45 (for v2026.44 use plugin 0.5.1). For a later CAD release
set `rev` in `Cargo.toml` to its tag's commit and `api_version` in
`plugin.toml`.
