# opencad-raster-plugin — Raster Tools per Open CAD Studio

[English](README.md) · **Italiano**

> **Progetto amatoriale, realizzato solo per divertimento.** Non è pensato per un uso
> professionale e viene fornito senza alcuna garanzia (vedi la licenza GPL-3.0):
> prima di usarlo su lavori reali, fai una copia dei tuoi disegni.
> Versione iniziale (0.x): le funzioni potrebbero cambiare. Le novità sono nel
> [registro delle modifiche](CHANGELOG.md); idee e segnalazioni nelle
> [Issues](https://github.com/Macchecaspita/opencad-raster-plugin/issues).

Plugin per [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio) che
aggiunge la scheda **Raster** per lavorare sulle immagini GIF, JPG, PNG, BMP,
TIFF e WebP nei disegni DWG/DXF:

- **inserire** un'immagine — o l'immagine contenuta nel **PDF** di un estratto di
  mappa — sul layer scelto (di serie RASTER), dietro al disegno, dimensionandola
  a schermo, per larghezza, a DPI con la **scala della mappa** o 1 pixel = 1 unità;
- **georeferenziarla** con **due punti** (sposta, ruota e scala in un colpo),
  **ruotarla**, portarla **dietro** al disegno;
- **ritagliare** un'immagine (tenere l'interno), fare un **foro** (nascondere
  l'interno) o **separare un pezzo** in una nuova immagine da spostare, con un
  **rettangolo**, un **poligono** disegnato a clic o una **polilinea chiusa**,
  un cerchio o un'ellisse già nel disegno;
- **ridimensionare** trascinando un angolo, oppure a larghezza / altezza /
  fattore di scala / **DPI** esatti;
- rendere **trasparente un colore** (per esempio lo sfondo bianco di una
  scansione), scelto con un clic sull'immagine, e colorare il resto con il
  colore del layer;
- **cambiare il percorso su disco** del file dell'immagine e **ritrovare** in
  una cartella le immagini il cui file è stato spostato;
- **elencare** le immagini con percorso, pixel, dimensioni e stato del file.

L'interfaccia è in **inglese**, oppure in **italiano** quando Open CAD Studio è in
italiano. `RT_LANG IT|EN|AUTO` forza una lingua.

Il riferimento completo dei comandi è in [PLUGIN.it.md](PLUGIN.it.md). Nel CAD, il
pulsante **Guida** della scheda Raster mostra una breve guida.

## Uso rapido

| Voglio… | Faccio… |
|---|---|
| inserire una foto o una scansione | **Inserisci immagine**, scegli il file e la dimensione, clicca il punto |
| tagliare via i bordi di una foto | **Ritaglia**, clic sull'immagine, due angoli del rettangolo da tenere |
| nascondere una zona | **Foro**, clic sull'immagine, due angoli |
| ritagliare lungo un contorno irregolare | **Ritaglia ▾ › Poligono**, clic sull'immagine, clic sui vertici, Invio |
| ritagliare lungo una polilinea già disegnata | seleziona immagine e polilinea → **Ritaglia ▾ › Da polilinea** |
| ritagliare un dettaglio e spostarlo | **Separa pezzo**, poi `SPOSTA` sul pezzo (resta selezionato) |
| portare una scansione in scala 1:1 | seleziona l'immagine → **Dimensioni / DPI** → modo *DPI*, 300, millimetri |
| ingrandire a occhio | **Trascina**: angolo fisso, poi angolo opposto |
| togliere lo sfondo bianco di una scansione | **Colore trasparente**, clic sull'immagine, clic sul bianco, *solo l'area collegata* |
| portare un estratto di mappa PDF in dimensioni reali | **Inserisci immagine**, scegli il PDF, *DPI* 300, scala 1:2000, metri |
| sovrapporre una scansione al rilievo | **Allinea 2 punti**: due punti sulla scansione, ognuno seguito dalla sua posizione vera |
| rendere leggibile sullo schermo scuro una mappa nera su bianco | **Colore trasparente**, clic sul bianco, *colora il resto con il colore del layer* (layer RASTER: bianco) |
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

Oppure, senza compilare nulla, dal **Plugin Manager** di Open CAD Studio
(comando `PLUGINS`): **Add repository →** `Macchecaspita/opencad-raster-plugin`,
scegli l'ultima versione e premi *Install*. Le versioni pubblicate sono in
[Releases](https://github.com/Macchecaspita/opencad-raster-plugin/releases).

## Pubblicare una nuova versione

1. Scrivi le novità in [CHANGELOG.md](CHANGELOG.md), sotto **[Non rilasciato]**.
2. In PowerShell, nella cartella del progetto:

   ```
   powershell -ExecutionPolicy Bypass -File scripts\release.ps1 0.5.0
   ```

Lo script aggiorna la versione in `Cargo.toml`, `plugin.toml` e `CHANGELOG.md`,
compila e prova il plugin, poi, dopo la tua conferma, crea il commit e il tag
e li carica su GitHub. Il workflow `release.yml` compila per Windows, Linux e
macOS e pubblica la release con le note prese dal CHANGELOG.

Numerazione: `0.4.1` per le sole correzioni, `0.5.0` per nuove funzioni,
`1.0.0` quando il plugin sarà considerato stabile.

## Compatibilità

Compilato contro `ocs_plugin_api` del tag **v2026.45** (Plugin API 16). Ogni
compilazione del plugin funziona solo con la versione di Open CAD Studio per cui
è stata fatta (il CAD controlla che plugin e programma si scambino i dati del
disegno nello stesso formato): per la **v2026.44 usa il plugin 0.5.1**. Per una
versione successiva del CAD, aggiorna `rev` in `Cargo.toml` con il commit del
nuovo tag e `api_version` in `plugin.toml`.

## Struttura

```
src/lib.rs       manifest, punto d'ingresso del plugin
src/i18n.rs      scelta inglese / italiano
src/ribbon.rs    scheda Raster della barra multifunzione
src/commands.rs  comandi RT_* e modifiche al disegno
src/tools.rs     strumenti interattivi (selezione, rettangolo, trascinamento) e finestre
src/geom.rs      geometria pura: pixel ↔ disegno, ritaglio, scala (con test)
src/imgsize.rs   dimensioni in pixel dall'intestazione del file (con test)
src/pdf.rs       immagine contenuta in una pagina PDF (con test)
src/pixels.rs    colore trasparente sui pixel RGBA (con test)
scripts/release.ps1  pubblicazione di una nuova versione
CHANGELOG.md     registro delle modifiche
icons/           icone SVG della scheda
```

Licenza GPL-3.0-only, come Open CAD Studio.
