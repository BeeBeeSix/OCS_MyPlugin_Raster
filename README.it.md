# opencad-raster-plugin — Raster Tools per Open CAD Studio

[English](README.md) · **Italiano**

> **Progetto amatoriale, fatto solo per divertimento.** Non è pensato per uso
> professionale e non offre alcuna garanzia (vedi la licenza GPL-3.0): conserva una copia dei tuoi
> disegni prima di usarlo su lavori reali. Versione iniziale (0.x): le funzioni possono
> cambiare. Le novità sono nel [registro delle modifiche](CHANGELOG.md); idee e
> segnalazioni di errori sono benvenute nelle [Issues](https://github.com/Macchecaspita/opencad-raster-plugin/issues).

Un plugin per [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
che aggiunge una scheda **Raster** alla barra multifunzione per le immagini GIF, JPG, PNG, BMP, TIFF e WebP
nei disegni DWG/DXF:

- **inserire** un'immagine — o l'immagine contenuta in un estratto di mappa **PDF** — su un layer
  a scelta (RASTER di default), dietro al disegno, dimensionata a schermo, per
  larghezza, per DPI con **scala della mappa** oppure 1 pixel = 1 unità;
- **posizionarla dal suo file world** (`.pgw`, `.tfw`, `.jgw`… accanto all'immagine — senza clic),
  oppure **georeferenziarla** con **due punti** (sposta, ruota e scala insieme),
  **ruotarla**, portarla **dietro** al disegno;
- **ritagliare** un'immagine (tenere l'interno), fare un **foro** (nascondere l'interno) oppure
  **separare un pezzo** in una nuova immagine spostabile — con un **rettangolo**,
  un **poligono** disegnato a clic, oppure una **polilinea chiusa**, un cerchio o
  un'ellisse già esistenti;
- **ridimensionare** trascinando un angolo, o a una larghezza / altezza / fattore di scala /
  **DPI** esatti;
- rendere **un colore trasparente** (ad es. lo sfondo bianco di una scansione), scelto
  con un clic sull'immagine, e colorare il resto con il colore del layer;
- **cambiare il percorso del file** su disco e **cercare** le immagini il cui file è stato spostato;
- **elencare** ogni immagine con percorso, pixel, dimensioni e stato del file.

L'interfaccia è in **inglese**, oppure in **italiano** quando Open CAD Studio è
in italiano. `RT_LANG IT|EN|AUTO` forza una lingua.

Riferimento completo dei comandi: [PLUGIN.it.md](PLUGIN.it.md). Nel CAD, il pulsante **Guida**
della scheda Raster mostra una breve guida.

## Guida rapida

| Voglio… | Faccio… |
|---|---|
| inserire una foto o una scansione | **Inserisci immagine**, scelgo il file e la dimensione, clicco il punto |
| rifilare i bordi di una foto | **Ritaglia**, clicco l'immagine, due angoli dell'area da tenere |
| nascondere un'area | **Foro**, clicco l'immagine, due angoli |
| ritagliare lungo un contorno irregolare | **Ritaglia ▾ › Poligono**, clicco l'immagine, clicco i vertici, Invio |
| ritagliare lungo una polilinea già disegnata | seleziono immagine e polilinea → **Ritaglia ▾ › Da polilinea** |
| staccare un dettaglio e spostarlo | **Separa pezzo**, poi `MOVE` sul pezzo (resta selezionato) |
| portare una scansione in scala 1:1 | seleziono l'immagine → **Dimensioni / DPI** → modo *DPI*, 300, millimetri |
| ingrandire a occhio | **Trascina**: angolo fisso, poi angolo opposto |
| togliere lo sfondo bianco di una scansione | **Colore trasparente**, clicco l'immagine, clicco il bianco, *solo l'area collegata* |
| portare un PDF di mappa catastale alla dimensione reale | **Inserisci immagine**, scelgo il PDF, *DPI* 300, scala 1:2000, metri |
| adattare una scansione al rilievo | **Allinea 2 punti**: due punti sulla scansione, ciascuno seguito dalla sua posizione vera |
| una mappa nera su bianco leggibile sullo schermo scuro | **Colore trasparente**, clicco il bianco, *colora il resto con il colore del layer* (layer RASTER: bianco) |
| la foto è stata spostata su un altro disco | **Cerca in cartella** e scelgo un file della nuova cartella |
| sostituire la foto con un'altra | seleziono l'immagine → **Cambia percorso** → scelgo il file |

## Installazione

Nel **Plugin Manager** di Open CAD Studio (comando `PLUGINS`):
**Add repository →** `Macchecaspita/opencad-raster-plugin`, si sceglie l'ultima
versione e si preme *Install*. Le versioni pubblicate sono in
[Releases](https://github.com/Macchecaspita/opencad-raster-plugin/releases).

Oppure a mano: si copiano la libreria compilata **e** `plugin.toml` in

- Windows: `%APPDATA%\OpenCADStudio\plugins\opencad.rastertools\`
- macOS: `~/Library/Application Support/OpenCADStudio/plugins/opencad.rastertools/`
- Linux: `~/.config/OpenCADStudio/plugins/opencad.rastertools/`

e si riavvia Open CAD Studio.

## Compilazione

Serve Rust (https://rustup.rs). Dalla cartella del progetto:

```
cargo test --release
cargo build --release
```

Il risultato è `target/release/opencad_raster_plugin.dll` (Windows),
`libopencad_raster_plugin.so` (Linux) oppure `libopencad_raster_plugin.dylib` (macOS).

## Pubblicare una nuova versione

1. Scrivi le novità in [CHANGELOG.md](CHANGELOG.md), sotto **[Non rilasciato]**.
2. In PowerShell, nella cartella del progetto:

   ```
   powershell -ExecutionPolicy Bypass -File scripts\release.ps1 0.5.0
   ```

Lo script imposta la versione in `Cargo.toml`, `plugin.toml` e `CHANGELOG.md`,
compila e collauda il plugin, poi — dopo la tua conferma — fa commit, tag e
push. Il workflow `release.yml` compila per Windows, Linux e macOS e
pubblica la release con le note prese dal registro delle modifiche.

Numerazione: `0.4.1` solo per correzioni, `0.5.0` per nuove funzioni, `1.0.0` quando il
plugin sarà considerato stabile.

## Compatibilità

Compilato con `ocs_plugin_api` dal tag **v2026.45** (Plugin API 16). Ogni
build del plugin funziona solo con la release di Open CAD Studio per cui è stata compilata (il
CAD verifica che plugin e host scambino i dati del disegno nello stesso formato):
per la **v2026.44 usa il plugin 0.5.1**. Per una release successiva del CAD, si imposta `rev` in
`Cargo.toml` sul commit del tag di quella release e `api_version` in `plugin.toml`.

## Struttura

```
src/lib.rs          manifest, punto d'ingresso del plugin
src/i18n.rs         scelta inglese / italiano
src/ribbon.rs       scheda Raster della barra
src/commands.rs     comandi RT_* e modifiche al disegno
src/tools.rs        strumenti interattivi (scelta, rettangolo, poligono, trascinamento) e finestre
src/geom.rs         geometria pura: pixel ↔ disegno, ritaglio, scala (testata)
src/imgsize.rs      dimensione in pixel dall'intestazione del file (testata)
src/pdf.rs          immagine dentro una pagina PDF (testata)
src/pixels.rs       colore trasparente su pixel RGBA (testata)
scripts/release.ps1 pubblica una nuova versione
CHANGELOG.md        registro delle modifiche
icons/              icone SVG della barra
```

Licenza: GPL-3.0-only, come Open CAD Studio.
