# opencad-raster-plugin — Raster Tools per Open CAD Studio

Plugin per [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio) che
aggiunge la scheda **Raster** per lavorare sulle immagini GIF, JPG, PNG, BMP,
TIFF e WebP inserite nei disegni DWG/DXF:

- **ritagliare** un'immagine con un rettangolo (tenere l'interno), fare un
  **foro** (nascondere l'interno) o **separare un pezzo** in una nuova immagine
  da spostare;
- **ridimensionare** trascinando un angolo, oppure a larghezza / altezza /
  fattore di scala / **DPI** esatti;
- **cambiare il percorso su disco** del file dell'immagine e **ritrovare** in
  una cartella le immagini il cui file è stato spostato;
- **elencare** le immagini con percorso, pixel, dimensioni e stato del file.

Il riferimento completo dei comandi è in [PLUGIN.md](PLUGIN.md).

## Uso rapido

| Voglio… | Faccio… |
|---|---|
| tagliare via i bordi di una foto | **Ritaglia**, clic sull'immagine, due angoli del rettangolo da tenere |
| nascondere una zona | **Foro**, clic sull'immagine, due angoli |
| ritagliare un dettaglio e spostarlo | **Separa pezzo**, poi `SPOSTA` sul pezzo (resta selezionato) |
| portare una scansione in scala 1:1 | seleziona l'immagine → **Dimensioni / DPI** → modo *DPI*, 300, millimetri |
| ingrandire a occhio | **Trascina**: angolo fisso, poi angolo opposto |
| la foto è stata spostata su un altro disco | **Cerca in cartella** e scegli un file nella nuova cartella |
| sostituire la foto con un'altra | seleziona l'immagine → **Cambia percorso** → scegli il file |

## Compilare

Serve Rust (https://rustup.rs). Dalla cartella del progetto:

```
cargo test --release
cargo build --release
```

Il risultato è `target/release/opencad_raster_plugin.dll` (Windows),
`libopencad_raster_plugin.so` (Linux) o `libopencad_raster_plugin.dylib` (macOS).

## Installare

Copia la libreria compilata **e** `plugin.toml` nella cartella

- Windows: `%APPDATA%\OpenCADStudio\plugins\opencad.rastertools\`
- macOS: `~/Library/Application Support/OpenCADStudio/plugins/opencad.rastertools/`
- Linux: `~/.config/OpenCADStudio/plugins/opencad.rastertools/`

e riavvia Open CAD Studio: compare la scheda **Raster**.

In alternativa: carica il progetto su un tuo repository GitHub, crea un tag
(`git tag v0.1.0 && git push --tags`): il workflow `release.yml` compila per
Windows, Linux e macOS e pubblica la release. Poi in Open CAD Studio:
**Plugin Manager → Add repository →** `tuo-account/opencad-raster-plugin` → *Install*.

## Compatibilità

Compilato contro `ocs_plugin_api` del tag **v2026.44** (Plugin API 14), come il
plugin di esempio Land Survey. Per una versione successiva del CAD, aggiorna
`rev` in `Cargo.toml` con il commit del nuovo tag e `api_version` in
`plugin.toml`.

## Struttura

```
src/lib.rs       manifest, punto d'ingresso del plugin
src/ribbon.rs    scheda Raster della barra multifunzione
src/commands.rs  comandi RT_* e modifiche al disegno
src/tools.rs     strumenti interattivi (selezione, rettangolo, trascinamento) e finestre
src/geom.rs      geometria pura: pixel ↔ disegno, ritaglio, scala (con test)
src/imgsize.rs   dimensioni in pixel dall'intestazione del file (con test)
icons/           icone SVG della scheda
```

Licenza GPL-3.0-only, come Open CAD Studio.
