# Raster Tools (`opencad.rastertools`)

[English](PLUGIN.md) · **Italiano**

Add-on esterno per [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
che gestisce le immagini raster collegate al disegno (entità IMAGE):
GIF, JPG, PNG, BMP, TIFF e WebP — gli stessi formati che il CAD visualizza.

Tutti i comandi hanno il prefisso `RT_`. La scheda **Raster** della barra
multifunzione li raccoglie in sette gruppi: *Inserisci*, *Posizione*, *Ritaglio*, *Trasparenza*, *Dimensioni*, *File*, *Aiuto*.
Il pulsante **Guida** (`RT_GUIDE`) apre una breve guida a tutti i comandi.
Nel gruppo *Ritaglio*, i pulsanti *Ritaglia*, *Foro* e *Separa pezzo* hanno un
menu (freccia ▾) per scegliere il contorno: **Rettangolo**, **Poligono** o
**Da polilinea**; il clic sul pulsante usa il rettangolo.

Il contorno va disegnato sull'immagine: le parti fuori dalla cornice vengono
tagliate via. Contorni con lati che si incrociano non sono accettati.
Ogni modifica è un solo passo di annullamento (`U` / Ctrl+Z).

## Lingua

Messaggi, richieste, finestre ed etichette della barra sono in **inglese**, oppure
in **italiano** quando la lingua dell'interfaccia di Open CAD Studio è l'italiano
(con l'impostazione *Sistema*, quando il sistema operativo è in italiano). Il
plugin legge la lingua dal file `settings.json` di Open CAD Studio prima di ogni
comando: i messaggi seguono subito un cambio, le etichette della barra dopo un
riavvio.

`RT_LANG IT` e `RT_LANG EN` forzano una lingua; `RT_LANG AUTO` torna a seguire
Open CAD Studio; `RT_LANG` da solo mostra l'impostazione attuale. La scelta è
salvata in `rastertools-lang.txt` nella cartella delle impostazioni di Open CAD
Studio.

## Comandi

| Comando | Barra | Cosa fa |
|---|---|---|
| `RT_ATTACH` | Inserisci › Inserisci immagine | **Inserisce una nuova immagine** nel disegno. Una finestra chiede il file e come dimensionarlo: *indicata sullo schermo* (punto di inserimento e angolo opposto, proporzionale; Invio al secondo punto = 1 pixel per unità), *larghezza nel disegno*, *risoluzione DPI* (con l'unità del disegno, proposta in base a `INSUNITS`) oppure *1 pixel = 1 unità*. Poi si clicca il punto di inserimento (angolo in basso a sinistra). La finestra chiede anche il **layer**: viene proposto per primo **RASTER** (creato, bianco, se nel disegno non c'è), oppure qualunque altro layer del disegno. Il file resta collegato, non viene copiato nel disegno. **PDF**: si può scegliere un PDF (per esempio un *estratto di mappa* catastale) e il plugin ne ricava l'immagine contenuta — la più grande del file, alla sua risoluzione — salvandola come `<nome>_pdf.png` accanto al PDF (un JPEG interno viene copiato come `<nome>_pdf.jpg`); il messaggio indica la risoluzione del PDF. Nel modo *DPI* la **scala della mappa 1:N** dà le dimensioni reali: un estratto di mappa da PDF a 300 DPI, scala 1:2000, unità metri entra nelle sue vere dimensioni in metri. I PDF solo vettoriali non si possono convertire. Le immagini nuove vanno **dietro** al disegno. |
| `RT_CLIP` | Ritaglio › Ritaglia | Ritaglio rettangolare: si sceglie l'immagine (o la si seleziona prima), poi due angoli. Resta visibile **solo l'interno** del rettangolo. Ripetuto su un'immagine già ritagliata, **restringe** il ritaglio (intersezione). Se l'immagine è ruotata, il rettangolo disegnato a schermo diventa un contorno poligonale. |
| `RT_CLIPHOLE` | Ritaglio › Foro | Come sopra, ma **nasconde l'interno** del rettangolo (ritaglio invertito). |
| `RT_CUT` | Ritaglio › Separa pezzo | "Forbici": la parte nel rettangolo diventa una **nuova immagine** (stesso file, ritagliata sul rettangolo) e nell'originale resta un foro al suo posto. Il pezzo resta selezionato: con `SPOSTA`/`MOVE` lo si porta dove serve. Richiede un'immagine non ancora ritagliata. |
| `RT_CLIPPOLY` / `RT_HOLEPOLY` / `RT_CUTPOLY` | Ritaglia / Foro / Separa pezzo › Poligono | Come le tre righe sopra, ma con un **poligono** di qualsiasi forma, anche concava (a "L", a stella…): si sceglie l'immagine e si cliccano i vertici; **Invio** o un clic vicino al primo vertice chiudono il poligono (servono almeno 3 vertici). |
| `RT_CLIPPL` / `RT_HOLEPL` / `RT_CUTPL` | Ritaglia / Foro / Separa pezzo › Da polilinea | Come sopra, usando come contorno una **polilinea chiusa** (anche con lati ad arco), un **cerchio** o un'**ellisse** già disegnati: si sceglie l'immagine, poi si clicca la polilinea o un punto al suo interno. Se immagine e polilinea sono già selezionate, il ritaglio è immediato. Gli archi sono approssimati con segmenti di 3°. |
| `RT_UNCLIP` | Ritaglio › Togli ritaglio | Toglie il ritaglio alle immagini selezionate (o a quella indicata). |
| `RT_TRANSP` | Trasparenza › Colore trasparente | Rende **trasparente un colore** dell'immagine. Si sceglie l'immagine (o la si seleziona prima), poi si **clicca sull'immagine** il colore da togliere (un mirino arancione segue il cursore). Una finestra mostra il colore (esadecimale e RGB) e chiede la **tolleranza** in % (0 = solo quel colore esatto; proposti 10% per JPG/WebP, che alterano leggermente i colori, e 2% per gli altri) e se agire su **tutta l'immagine** o **solo sull'area collegata al punto cliccato** (utile per togliere lo sfondo esterno lasciando lo stesso colore all'interno del disegno). Il plugin salva una **copia PNG** con la trasparenza nella cartella dell'originale, `<nome>_trasp-N.png` (l'originale non viene modificato), ricollega l'immagine alla copia e attiva la trasparenza. Ripetendo il comando si tolgono altri colori: ogni volta nasce una nuova copia. Opzione **Colora il resto con il colore del layer** (attiva di serie quando il colore scelto è chiaro, come la carta di una scansione): lo sfondo sparisce e tutto il resto prende il colore del layer dell'immagine, con i bordi sfumati — le linee nere di una mappa su carta bianca diventano per esempio linee bianche ben leggibili sullo sfondo scuro del CAD, senza l'alone chiaro che lascia la sola trasparenza. Vale per tutta l'immagine (l'opzione dell'area viene ignorata); per cambiare colore in seguito, cambia il colore del layer e ripeti il comando sull'originale. Annullabile con `U` / Ctrl+Z. |
| `RT_TRANSPOFF` | Trasparenza › Togli trasparenza | Spegne la trasparenza delle immagini selezionate (o di quella scelta): i colori tolti tornano visibili. Per tornare al file originale usare `RT_PATH`. |
| `RT_ALIGN` | Posizione › Allinea 2 punti | **Georeferenziazione**: si sceglie l'immagine, si clicca un punto riconoscibile sull'immagine e il punto dove deve andare, poi un secondo punto (lontano dal primo) e la sua destinazione. L'immagine si sposta, ruota e scala in modo che i due punti cadano esattamente al loro posto (anteprima arancione). **Invio** dopo la prima coppia la sposta soltanto. |
| `RT_ROTATE` | Posizione › Ruota | Si sceglie l'immagine, si clicca il **centro di rotazione**, poi un **punto di riferimento** sull'immagine e la **nuova direzione** del riferimento (allinea l'immagine a una linea). **Invio** al centro o al riferimento chiede un **angolo esatto** (positivo = antiorario; attorno al centro dell'immagine se il centro non è stato cliccato). |
| `RT_BACK` | Posizione › Porta dietro | Porta le immagini selezionate (o quella scelta) **dietro** al disegno, con il comando `DRAWORDER BACK` di Open CAD Studio. `RT_ATTACH` lo fa da solo per le immagini nuove. |
| `RT_RESIZE` | Dimensioni › Trascina | Ridimensionamento grafico proporzionale: si sceglie l'immagine, si clicca vicino all'angolo che deve **restare fermo**, poi si porta l'angolo opposto alla nuova posizione (anteprima arancione). Gli angoli sono quelli della parte visibile. |
| `RT_SIZE` | Dimensioni › Dimensioni / DPI | Finestra con: larghezza o altezza (proporzionali), larghezza e altezza libere, fattore di scala, oppure **risoluzione di stampa** (DPI) con l'unità del disegno (mm, cm, m, pollici). Punto fisso: angolo in basso a sinistra o centro. Opzione per riferire le misure alla parte visibile. Agisce su tutte le immagini selezionate. |
| `RT_PATH` | File › Cambia percorso | Cambia il **file su disco** dell'immagine. Dalla barra si apre subito il selettore file e il percorso si applica alle immagini selezionate (se non ce ne sono, si indica l'immagine dopo). Digitato senza argomenti apre una finestra con il percorso attuale e il suo stato. Tutte le immagini che usano lo stesso file vengono ricollegate; la **larghezza nel disegno resta la stessa** anche se il nuovo file ha un numero diverso di pixel (l'eventuale ritaglio viene riproporzionato). |
| `RT_FIND` | File › Cerca in cartella | Per le immagini il cui file **manca**: si sceglie un file qualsiasi nella cartella dove ora si trovano e il plugin ricollega ogni immagine al file con lo stesso nome, anche nelle sottocartelle. |
| `RT_LIST` | File › Elenco | Elenca le immagini: handle, nome, pixel, dimensioni nel disegno, dimensione del pixel, ritaglio, stato del file (`trovato` / `MANCANTE`) e percorso completo. |
| `RT_PURGE` | File › Pulisci copie | Cerca le copie `<nome>_trasp-N.png` create da `RT_TRANSP`, nelle cartelle delle immagini del disegno, che **nessuna immagine di questo disegno usa**, le elenca e — dopo la conferma — le sposta nella sottocartella `_trasp-non-usate` (non le cancella: un altro disegno potrebbe usarle). |
| `RT_GUIDE` | Aiuto › Guida | Breve guida a tutti i comandi, in una finestra. |
| `RT_LANG` | — | Lingua dell'interfaccia: `IT`, `EN` o `AUTO` (vedi sopra). |
| `RT_DIAG` | — | `ON` / `OFF`: registro diagnostico `rastertools-log.txt` nella cartella delle impostazioni di Open CAD Studio (spento di serie; utile se un comando si ferma senza messaggi). |
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
| `RT_ALIGNAPPLY <h> <p1x> <p1y> <q1x> <q1y> [<p2x> <p2y> <q2x> <q2y>]` | Sposta l'immagine perché p1 cada su q1 (e, con la seconda coppia, la ruota e scala perché p2 cada su q2). |
| `RT_ROTATEAPPLY <h> <gradi> <x> <y>` | Ruota in senso antiorario attorno a (x, y). |
| `RT_BACKAPPLY <h,...>` | Porta le immagini dietro al disegno. |
| `RT_TRANSPAPPLY <h> <x_px> <y_px> <tolleranza_%> ALL\|AREA [TINT]` | Rende trasparente il colore del pixel del file (`x`, `y` da in alto a sinistra), su tutta l'immagine o solo sull'area collegata; `TINT` colora il resto con il colore del layer. |
| `RT_TRANSPOFFAPPLY <h,...>` | Spegne la trasparenza. |

`RT_PATHAPPLY`, `RT_FINDAPPLY`, `RT_ATTACHPLACE` e `RT_ATTACHAPPLY` usano il percorso scelto nella finestra appena
chiusa (i percorsi non passano mai per la riga di comando, che può
convertirli in maiuscolo).

## Dati salvati

Il plugin non aggiunge XDATA né oggetti propri: modifica solo i campi standard
dell'entità IMAGE (punto di inserimento, vettori U/V, dimensione in pixel,
contorno e modalità di ritaglio, percorso), quindi il disegno resta leggibile
da qualunque programma DWG/DXF anche senza il plugin. `RT_TRANSP` in più scrive
la copia PNG `<nome>_trasp-N.png` accanto al file originale e accende il flag
standard "trasparenza" dell'immagine.

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

Compatibilità: Plugin API 16, Open CAD Studio v2026.45 (per la v2026.44 usare il plugin 0.5.1). Per una versione più
recente del CAD aggiornare `rev` in `Cargo.toml` al commit del suo tag e
`api_version` in `plugin.toml`.
