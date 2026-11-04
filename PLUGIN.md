# Raster Tools (`opencad.rastertools`)

Add-on esterno per [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
che gestisce le immagini raster collegate al disegno (entità IMAGE):
GIF, JPG, PNG, BMP, TIFF e WebP — gli stessi formati che il CAD visualizza.

Tutti i comandi hanno il prefisso `RT_`. La scheda **Raster** della barra
multifunzione li raccoglie in tre gruppi: *Ritaglio*, *Dimensioni*, *File*.
Nel gruppo *Ritaglio*, i pulsanti *Ritaglia*, *Foro* e *Separa pezzo* hanno un
menu (freccia ▾) per scegliere il contorno: **Rettangolo**, **Poligono** o
**Da polilinea**; il clic sul pulsante usa il rettangolo.

Il contorno va disegnato sull'immagine: le parti fuori dalla cornice vengono
tagliate via. Contorni con lati che si incrociano non sono accettati.
Ogni modifica è un solo passo di annullamento (`U` / Ctrl+Z).

## Comandi

| Comando | Barra | Cosa fa |
|---|---|---|
| `RT_CLIP` | Ritaglio › Ritaglia | Ritaglio rettangolare: si sceglie l'immagine (o la si seleziona prima), poi due angoli. Resta visibile **solo l'interno** del rettangolo. Ripetuto su un'immagine già ritagliata, **restringe** il ritaglio (intersezione). Se l'immagine è ruotata, il rettangolo disegnato a schermo diventa un contorno poligonale. |
| `RT_CLIPHOLE` | Ritaglio › Foro | Come sopra, ma **nasconde l'interno** del rettangolo (ritaglio invertito). |
| `RT_CUT` | Ritaglio › Separa pezzo | "Forbici": la parte nel rettangolo diventa una **nuova immagine** (stesso file, ritagliata sul rettangolo) e nell'originale resta un foro al suo posto. Il pezzo resta selezionato: con `SPOSTA`/`MOVE` lo si porta dove serve. Richiede un'immagine non ancora ritagliata. |
| `RT_CLIPPOLY` / `RT_HOLEPOLY` / `RT_CUTPOLY` | Ritaglia / Foro / Separa pezzo › Poligono | Come le tre righe sopra, ma con un **poligono** di qualsiasi forma, anche concava (a "L", a stella…): si sceglie l'immagine e si cliccano i vertici; **Invio** o un clic vicino al primo vertice chiudono il poligono (servono almeno 3 vertici). |
| `RT_CLIPPL` / `RT_HOLEPL` / `RT_CUTPL` | Ritaglia / Foro / Separa pezzo › Da polilinea | Come sopra, usando come contorno una **polilinea chiusa** (anche con lati ad arco), un **cerchio** o un'**ellisse** già disegnati: si sceglie l'immagine, poi si clicca la polilinea o un punto al suo interno. Se immagine e polilinea sono già selezionate, il ritaglio è immediato. Gli archi sono approssimati con segmenti di 3°. |
| `RT_UNCLIP` | Ritaglio › Togli ritaglio | Toglie il ritaglio alle immagini selezionate (o a quella indicata). |
| `RT_RESIZE` | Dimensioni › Trascina | Ridimensionamento grafico proporzionale: si sceglie l'immagine, si clicca vicino all'angolo che deve **restare fermo**, poi si porta l'angolo opposto alla nuova posizione (anteprima arancione). Gli angoli sono quelli della parte visibile. |
| `RT_SIZE` | Dimensioni › Dimensioni / DPI | Finestra con: larghezza o altezza (proporzionali), larghezza e altezza libere, fattore di scala, oppure **risoluzione di stampa** (DPI) con l'unità del disegno (mm, cm, m, pollici). Punto fisso: angolo in basso a sinistra o centro. Opzione per riferire le misure alla parte visibile. Agisce su tutte le immagini selezionate. |
| `RT_PATH` | File › Cambia percorso | Cambia il **file su disco** dell'immagine. Dalla barra si apre subito il selettore file e il percorso si applica alle immagini selezionate (se non ce ne sono, si indica l'immagine dopo). Digitato senza argomenti apre una finestra con il percorso attuale e il suo stato. Tutte le immagini che usano lo stesso file vengono ricollegate; la **larghezza nel disegno resta la stessa** anche se il nuovo file ha un numero diverso di pixel (l'eventuale ritaglio viene riproporzionato). |
| `RT_FIND` | File › Cerca in cartella | Per le immagini il cui file **manca**: si sceglie un file qualsiasi nella cartella dove ora si trovano e il plugin ricollega ogni immagine al file con lo stesso nome, anche nelle sottocartelle. |
| `RT_LIST` | File › Elenco | Elenca le immagini: handle, nome, pixel, dimensioni nel disegno, dimensione del pixel, ritaglio, stato del file (`trovato` / `MANCANTE`) e percorso completo. |
| `RT_HELP` | — | Riepilogo dei comandi. |

### Ricerca del file

Un'immagine risulta *trovata* se il percorso salvato esiste, oppure se esiste
relativo alla cartella del disegno, oppure se nella cartella del disegno c'è
un file con lo stesso nome.

### Percorso salvato nel DWG

Nel DWG il percorso sta nella *definizione* dell'immagine (IMAGEDEF), condivisa
da tutte le sue copie. `RT_PATH` e `RT_FIND` aggiornano subito le entità (la
vista si aggiorna) e poi spostano la definizione con il comando dell'host
`XREF Path <nome> <nuovo percorso>`, così il nuovo percorso viene salvato.
Se gli script sono disattivati (`SCRIPTCOMMANDS`) il plugin stampa la riga
esatta da eseguire a mano. Per percorsi relativi o assoluti usare
`XREF Pathtype`.

## Comandi di applicazione (script)

Gli strumenti interattivi finiscono passando all'host una riga di comando
che esegue la modifica. Le stesse righe si possono usare in uno script.
Gli handle sono esadecimali; più handle separati da virgola.

| Riga | Effetto |
|---|---|
| `RT_CLIPAPPLY <h> KEEP\|HOLE\|CUT <x1> <y1> <x2> <y2> [z]` | Ritaglio con il rettangolo (coordinate del disegno). |
| `RT_CLIPPOLYAPPLY <h> KEEP\|HOLE\|CUT <x1> <y1> <x2> <y2> <x3> <y3> ...` | Ritaglio con un poligono (coordinate del disegno, almeno 3 vertici). |
| `RT_UNCLIPAPPLY <h,...>` | Toglie il ritaglio. |
| `RT_SCALEAPPLY <h> <ancora_x_px> <ancora_y_px> <fattore_x> [fattore_y]` | Scala attorno a un punto espresso in pixel dell'immagine (0,0 = angolo di inserimento). |
| `RT_SIZEAPPLY <h,...> W\|H\|WH\|F\|DPI <larg> <alt> <fattore> <dpi> MM\|CM\|M\|IN BL\|CTR FULL\|VIS` | Come la finestra di `RT_SIZE`. Es.: `RT_SIZEAPPLY 2A DPI 0 0 0 300 MM BL FULL`. |
| `RT_SIZEDLG <h,...>` / `RT_PATHDLG <h,...>` | Apre la finestra per quelle immagini. |

`RT_PATHAPPLY` e `RT_FINDAPPLY` usano il percorso scelto nella finestra appena
chiusa (i percorsi non passano mai per la riga di comando, che può
convertirli in maiuscolo).

## Dati salvati

Il plugin non aggiunge XDATA né oggetti propri: modifica solo i campi standard
dell'entità IMAGE (punto di inserimento, vettori U/V, dimensione in pixel,
contorno e modalità di ritaglio, percorso), quindi il disegno resta leggibile
da qualunque programma DWG/DXF anche senza il plugin.

## Compilazione e installazione

```
cargo test --release     # test della geometria e dei lettori di intestazione
cargo build --release    # → target/release/{lib,}opencad_raster_plugin.{dll,so,dylib}
```

Installazione manuale: copiare la libreria e `plugin.toml` in

* Windows: `%APPDATA%\OpenCADStudio\plugins\opencad.rastertools\`
* macOS: `~/Library/Application Support/OpenCADStudio/plugins/opencad.rastertools/`
* Linux: `~/.config/OpenCADStudio/plugins/opencad.rastertools/`

e riavviare il CAD. Oppure pubblicare una release su GitHub (il workflow
`release.yml` lo fa al push di un tag `v*`) e installarla da
**Plugin Manager → Add repository**.

Compatibilità: Plugin API 14, Open CAD Studio v2026.44. Per una versione più
recente del CAD aggiornare `rev` in `Cargo.toml` al commit del suo tag e
`api_version` in `plugin.toml`.
