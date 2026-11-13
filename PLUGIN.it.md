# Raster Tools (`opencad.rastertools`)

[English](PLUGIN.md) · **Italiano**

Componente aggiuntivo esterno per [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio)
che gestisce le immagini raster collegate a un disegno (entità IMAGE):
GIF, JPG, PNG, BMP, TIFF e WebP — gli stessi formati che il CAD visualizza.

Ogni comando inizia con `RT_`. La scheda **Raster** della barra multifunzione li raggruppa in sette
pannelli: *Inserisci*, *Posizione*, *Ritaglio*, *Trasparenza*, *Dimensioni*, *File*, *Guida*. Il pulsante **Guida**
(`RT_GUIDE`) apre una breve guida a tutti i comandi. Nel pannello *Ritaglio* i pulsanti *Ritaglia*,
*Foro* e *Separa pezzo* hanno un menu (freccia ▾) per scegliere il contorno:
**Rettangolo**, **Poligono** o **Da polilinea**; cliccando il pulsante stesso si usa
il rettangolo.

Il contorno viene disegnato sopra l'immagine: le parti fuori dalla cornice vengono tagliate via.
I contorni con lati che si incrociano vengono rifiutati. Ogni modifica è un unico passo di annullamento
(`U` / Ctrl+Z).

## Lingua

Messaggi, richieste, finestre ed etichette della barra sono in **inglese**, oppure in
**italiano** quando la lingua dell'interfaccia di Open CAD Studio è l'italiano (con
l'impostazione *Sistema*, quando il sistema operativo è in italiano). Il plugin legge la
lingua dal file `settings.json` di Open CAD Studio prima di ogni comando, quindi i
messaggi seguono subito un cambiamento; le etichette della barra lo seguono dopo un riavvio.

`RT_LANG IT`, `RT_LANG EN` forzano una lingua; `RT_LANG AUTO` torna a
seguire Open CAD Studio; `RT_LANG` da solo mostra l'impostazione attuale. La
scelta è salvata in `rastertools-lang.txt` nella cartella delle impostazioni di Open CAD Studio.

## Comandi

| Comando | Barra | Cosa fa |
|---|---|---|
| `RT_ATTACH` | Inserisci › Inserisci immagine | **Inserisce una nuova immagine** nel disegno. Una finestra chiede il file e come dimensionarla: *a schermo* (punto di inserimento e angolo opposto, proporzionale; Invio al secondo punto = 1 pixel per unità), *larghezza nel disegno*, *risoluzione DPI* (con l'unità del disegno, proposta in base a `INSUNITS`) oppure *1 pixel = 1 unità*. Poi si clicca il punto di inserimento (angolo in basso a sinistra). La finestra chiede anche il **layer**: viene proposto per primo **RASTER** (creato, bianco, se il disegno non ce l'ha), oppure un altro layer qualsiasi del disegno. Il file resta collegato, non viene copiato nel disegno. **PDF**: si sceglie un PDF (ad es. un *estratto di mappa* catastale) e il plugin ne estrae l'immagine contenuta — la più grande del file, alla sua risoluzione — in `<nome>_pdf.png` accanto al PDF (un JPEG al suo interno viene copiato come `<nome>_pdf.jpg`); il messaggio indica la risoluzione del PDF. Nel modo *DPI* la **scala della mappa 1:N** dà la dimensione reale: un PDF di mappa catastale a 300 DPI, scala 1:2000, unità metri entra nelle sue dimensioni reali in metri. I PDF solo vettoriali non si possono convertire. Le nuove immagini vanno **dietro** al disegno. |
| `RT_CLIP` | Ritaglio › Ritaglia | Ritaglio rettangolare: si sceglie l'immagine (o la si seleziona prima), poi due angoli. Resta visibile **solo l'interno** del rettangolo. Ripetuto su un'immagine già ritagliata, **restringe** il ritaglio (intersezione). Se l'immagine è ruotata, il rettangolo disegnato a schermo diventa un contorno poligonale. |
| `RT_CLIPHOLE` | Ritaglio › Foro | Come sopra, ma **nasconde l'interno** del rettangolo (ritaglio invertito). |
| `RT_CUT` | Ritaglio › Separa pezzo | «Forbici»: la parte dentro il rettangolo diventa una **nuova immagine** (stesso file, ritagliata al rettangolo) e l'originale conserva un foro al suo posto. Il pezzo resta selezionato: si sposta con `MOVE`. Serve un'immagine non ancora ritagliata. |
| `RT_CLIPPOLY` / `RT_HOLEPOLY` / `RT_CUTPOLY` | Ritaglia / Foro / Separa pezzo › Poligono | Come le tre righe precedenti, ma con un **poligono** di forma qualsiasi, anche concavo («L», stella…): si sceglie l'immagine e si cliccano i vertici; **Invio** o un clic vicino al primo vertice chiude il poligono (almeno 3 vertici). |
| `RT_CLIPPL` / `RT_HOLEPL` / `RT_CUTPL` | Ritaglia / Foro / Separa pezzo › Da polilinea | Come sopra, usando come contorno una **polilinea chiusa** (anche con lati ad arco), un **cerchio** o un'**ellisse** già disegnati: si sceglie l'immagine, poi si clicca la polilinea o un punto al suo interno. Se immagine e polilinea sono già selezionate, il ritaglio è immediato. Gli archi sono approssimati con segmenti di 3°. |
| `RT_UNCLIP` | Ritaglio › Togli ritaglio | Toglie il ritaglio dalle immagini selezionate (o da quella che si sceglie). |
| `RT_TRANSP` | Trasparenza › Colore trasparente | Rende **trasparente un colore dell'immagine**. Si sceglie l'immagine (o la si seleziona prima), poi si **clicca sull'immagine** il colore da togliere (un mirino arancione segue il cursore). Una finestra mostra il colore (esadecimale e RGB) e chiede la **tolleranza** in % (0 = solo quel colore esatto; 10% è proposto per JPG/WebP, i cui colori variano leggermente, 2% per gli altri) e se agire su **tutta l'immagine** oppure **solo sull'area collegata al punto cliccato** (utile per togliere lo sfondo esterno mentre lo stesso colore dentro il disegno resta). Il plugin salva una **copia PNG** con la trasparenza nella cartella dell'originale, `<nome>_trasp-N.png` (l'originale non viene modificato), ricollega l'immagine alla copia e attiva la trasparenza. Si può ripetere per togliere altri colori: ogni esecuzione crea una nuova copia. Opzione **Colora il resto con il colore del layer** (attiva di serie quando il colore scelto è chiaro, ad es. la carta di una scansione): lo sfondo sparisce e tutto il resto prende il colore del layer dell'immagine, con i bordi sfumati — le linee nere su una mappa bianca diventano, per esempio, linee bianche che risaltano sullo sfondo scuro del CAD, senza l'alone chiaro lasciato dalla sola trasparenza. Vale per tutta l'immagine (l'opzione area viene ignorata); per cambiare il colore in seguito, si cambia il colore del layer e si riesegue il comando sull'originale. Annulla con `U` / Ctrl+Z. |
| `RT_ALPHAON` | Trasparenza › Attiva alfa | Usa il **canale alfa** del file dell'immagine (PNG/TIFF a 32 bit) sulle immagini selezionate — oppure su **tutte** le immagini del disegno se non c'è selezione. Cambia solo il flag di visualizzazione: nessun file viene scritto o copiato. `RT_TRANSPOFF` lo disattiva di nuovo. |
| `RT_TRANSPOFF` | Trasparenza › Togli trasparenza | Disattiva la trasparenza sulle immagini selezionate (o su quella che si sceglie): i colori tolti tornano visibili. Per tornare al file originale si usa `RT_PATH`. |
| `RT_ALIGN` | Posizione › Allinea 2 punti | **Georeferenziazione**: si sceglie l'immagine, si clicca un punto riconoscibile su di essa e il punto dove deve andare, poi un secondo punto (lontano dal primo) e la sua destinazione. L'immagine si sposta, ruota e scala in modo che i due punti coincidano esattamente (anteprima arancione). **Invio** dopo la prima coppia = solo spostamento. |
| `RT_WORLD` | Posizione › Da file world | Rimette le immagini selezionate (o quella cliccata) dove dice il loro **file world** — `.pgw` `.tfw` `.jgw` `.gfw` `.bpw` `.wld` accanto all'immagine, con lo stesso nome. Posizione, dimensione del pixel e rotazione vengono dalle sei righe; il ritaglio resta. Gli stessi dati sono usati da **Inserisci immagine › File world**, che posiziona una nuova immagine senza clic. **GeoTIFF**: per i `.tif`/`.tiff` la posizione viene letta dai tag nell'intestazione del file (scala pixel + punto di aggancio, oppure matrice di trasformazione; TIFF classico e BigTIFF); il `.tfw` è usato solo quando il TIFF non ha tag geografici. |
| `RT_OSM [<EPSG> [pixel] [max-tessere]]` | Inserisci › Mappa base | Si seleziona un **rettangolo (polilinea chiusa)**, si esegue il comando e si indica il **codice EPSG** del disegno (UTM 32610 = zona 10N, 2193 = NZTM, …). Il plugin scarica le tessere OpenStreetMap per quell'area, le riproietta (Web Mercator → il tuo sistema) in un raster orientato a nord, scrive `osm_epsg…png` + `.pgw` accanto al disegno e lo inserisce sul layer `OSM`, dietro al disegno, con il credito «© OpenStreetMap contributors». Dimensione del pixel: automatica (la più fine entro il tetto di tessere, 64 di default) oppure il valore indicato. L'URL delle tessere è configurabile (`{z}/{x}/{y}`); rispetta le condizioni d'uso del server delle tessere. Le tessere restano in cache per 7 giorni. |
| `RT_FOLDER <file o cartella>` | Inserisci › Inserisci cartella | Si sceglie un'immagine qualsiasi di una cartella: **tutte** le immagini raster lì presenti vengono inserite nella posizione del loro file world, sul layer RASTER (creato se manca), dietro al disegno, poi zoom estensione. Le immagini senza file world, o già nel disegno, vengono saltate ed elencate. Un solo passo di annullamento. |
| `RT_ROTATE` | Posizione › Ruota | Si sceglie l'immagine, si clicca il **centro di rotazione**, poi un **punto di riferimento** sull'immagine e la **nuova direzione** di quel riferimento (allinea l'immagine a una linea). **Invio** al passo del centro o del riferimento chiede un **angolo esatto** (positivo = antiorario; attorno al centro dell'immagine se non è stato cliccato un centro). |
| `RT_BACK` | Posizione › Porta dietro | Porta le immagini selezionate (o quella che si sceglie) **dietro** al disegno, con il comando host `DRAWORDER BACK`. `RT_ATTACH` lo fa da sola per le nuove immagini. |
| `RT_RESIZE` | Dimensioni › Trascina | Ridimensionamento proporzionale a schermo: si sceglie l'immagine, si clicca vicino all'angolo che deve **restare fermo**, poi si sposta l'angolo opposto nella nuova posizione (anteprima arancione). Gli angoli sono quelli della parte visibile. |
| `RT_SIZE` | Dimensioni › Dimensioni / DPI | Finestra con: larghezza o altezza (proporzionale), larghezza e altezza libere, fattore di scala, oppure **risoluzione di stampa** (DPI) con l'unità del disegno (mm, cm, m, pollici). Punto fisso: angolo in basso a sinistra o centro. Opzione per misurare la parte visibile. Funziona su ogni immagine selezionata. |
| `RT_PATH` | File › Cambia percorso | Cambia il **file su disco** dell'immagine. Dalla barra si apre subito la selezione del file e il percorso si applica alle immagini selezionate (se non ce ne sono, si sceglie l'immagine dopo). Digitato senza argomenti apre una finestra con il percorso attuale e il suo stato. Tutte le immagini che usano lo stesso file vengono ricollegate; la **larghezza nel disegno resta la stessa** anche se il nuovo file ha un numero di pixel diverso (l'eventuale ritaglio viene riscalato). |
| `RT_FIND` | File › Cerca in cartella | Per le immagini il cui file è **mancante**: si sceglie un file qualsiasi della cartella dove si trovano ora e il plugin ricollega ogni immagine al file con lo stesso nome, sottocartelle comprese. |
| `RT_LIST` | File › Elenco | Elenca le immagini: handle, nome, pixel, dimensioni nel disegno, dimensione del pixel, ritaglio, stato del file (`found` / `MISSING`) e percorso completo. |
| `RT_PURGE` | File › Pulisci copie | Trova le copie `<nome>_trasp-N.png` create da `RT_TRANSP`, nelle cartelle delle immagini di questo disegno, che **nessuna immagine di questo disegno usa**, le elenca e — dopo la conferma — le sposta in una sottocartella `_trasp-non-usate` (non vengono cancellate: un altro disegno potrebbe usarle). |
| `RT_GUIDE` | Guida › Guida | Breve guida a tutti i comandi, in una finestra. |
| `RT_LANG` | — | Lingua dell'interfaccia: `IT`, `EN` o `AUTO` (vedi sopra). |
| `RT_DIAG` | — | `ON` / `OFF`: registro diagnostico `rastertools-log.txt` nella cartella delle impostazioni di Open CAD Studio (spento di serie; utile quando un comando si ferma senza messaggi). |
| `RT_HELP` | — | Riepilogo dei comandi. |

### Come si trova il file

Un'immagine conta come *trovata* se il percorso memorizzato esiste, oppure esiste relativamente
alla cartella del disegno, oppure la cartella del disegno contiene un file con lo stesso nome.

### Percorso salvato nel DWG

In un DWG il percorso sta nella *definizione* dell'immagine (IMAGEDEF), condivisa da tutte le sue
copie. `RT_PATH` e `RT_FIND` aggiornano subito le entità (la vista si aggiorna)
e poi spostano la definizione con il comando host
`XREF Path <nome> <nuovo percorso>`, così il nuovo percorso viene salvato. Se i comandi da script
sono disattivati (`SCRIPTCOMMANDS`) il plugin stampa la riga esatta da eseguire
a mano. Per percorsi relativi o assoluti si usa `XREF Pathtype`.

## Comandi di applicazione (script)

Gli strumenti interattivi terminano passando all'host una riga di comando che esegue la
modifica. Le stesse righe funzionano in uno script. Gli handle sono esadecimali; più
handle si separano con virgole.

| Riga | Effetto |
|---|---|
| `RT_CLIPAPPLY <h> KEEP\|HOLE\|CUT <x1> <y1> <x2> <y2> [z]` | Ritaglia con il rettangolo (coordinate del disegno). |
| `RT_CLIPPOLYAPPLY <h> KEEP\|HOLE\|CUT <x1> <y1> <x2> <y2> <x3> <y3> ...` | Ritaglia con un poligono (coordinate del disegno, almeno 3 vertici). |
| `RT_UNCLIPAPPLY <h,...>` | Toglie il ritaglio. |
| `RT_SCALEAPPLY <h> <anchor_x_px> <anchor_y_px> <factor_x> [factor_y]` | Scala attorno a un punto dato in pixel dell'immagine (0,0 = angolo di inserimento). |
| `RT_SIZEAPPLY <h,...> W\|H\|WH\|F\|DPI <width> <height> <factor> <dpi> MM\|CM\|M\|IN BL\|CTR FULL\|VIS` | Come la finestra di `RT_SIZE`. Es.: `RT_SIZEAPPLY 2A DPI 0 0 0 300 MM BL FULL`. |
| `RT_SIZEDLG <h,...>` / `RT_PATHDLG <h,...>` | Apre la finestra per quelle immagini. |
| `RT_ALIGNAPPLY <h> <p1x> <p1y> <q1x> <q1y> [<p2x> <p2y> <q2x> <q2y>]` | Sposta l'immagine in modo che p1 coincida con q1 (e, con la seconda coppia, la ruota e scala in modo che p2 coincida con q2). |
| `RT_WORLDAPPLY <h,...>` | Riposiziona le immagini dai loro file world. |
| `RT_ROTATEAPPLY <h> <degrees> <x> <y>` | Ruota in senso antiorario attorno a (x, y). |
| `RT_BACKAPPLY <h,...>` | Porta le immagini dietro al disegno. |
| `RT_TRANSPAPPLY <h> <x_px> <y_px> <tolerance_%> ALL\|AREA [TINT]` | Rende trasparente il colore di quel pixel del file (`x`, `y` dall'alto a sinistra), su tutta l'immagine o solo sull'area collegata; `TINT` colora il resto con il colore del layer. |
| `RT_TRANSPOFFAPPLY <h,...>` | Disattiva la trasparenza. |

`RT_PATHAPPLY`, `RT_FINDAPPLY`, `RT_ATTACHPLACE` e `RT_ATTACHAPPLY` usano il
percorso scelto nella finestra appena chiusa (i percorsi non passano mai dalla riga
di comando, che potrebbe metterli in maiuscolo).

## Dati salvati

Il plugin non aggiunge XDATA né oggetti propri: modifica solo i campi standard di IMAGE
(punto di inserimento, vettori U/V, dimensione del pixel, contorno e modo del ritaglio, percorso),
quindi il disegno resta leggibile da qualsiasi programma DWG/DXF senza il plugin.
`RT_TRANSP` scrive anche la copia PNG `<nome>_trasp-N.png` accanto al file
originale e imposta il flag standard di «trasparenza» dell'immagine.

## Compilazione e installazione

```
cargo test --release     # test di geometria, lettori di intestazioni e lingua
cargo build --release    # → target/release/{lib,}opencad_raster_plugin.{dll,so,dylib}
```

Installazione manuale: si copiano la libreria e `plugin.toml` in

* Windows: `%APPDATA%\OpenCADStudio\plugins\opencad.rastertools\`
* macOS: `~/Library/Application Support/OpenCADStudio/plugins/opencad.rastertools/`
* Linux: `~/.config/OpenCADStudio/plugins/opencad.rastertools/`

e si riavvia il CAD. Oppure si pubblica una release su GitHub (il workflow `release.yml`
lo fa quando si invia un tag `v*`) e la si installa da
**Plugin Manager → Add repository**.

Compatibilità: Plugin API 16, Open CAD Studio v2026.45 (per la v2026.44 usa il plugin 0.5.1). Per una release successiva del CAD
si imposta `rev` in `Cargo.toml` sul commit del suo tag e `api_version` in
`plugin.toml`.

## Altre sorgenti di tessere per RT_OSM (aeree)

Si incolla un qualsiasi modello Web-Mercator `{z}/{x}/{y}` nel campo URL della finestra. Verifica tu stesso ogni licenza.

| Sorgente | Modello | Note |
|---|---|---|
| USGS Imagery (USA, NAIP ~1 m) | `https://basemap.nationalmap.gov/arcgis/rest/services/USGSImageryOnly/MapServer/tile/{z}/{y}/{x}` | Servizio pubblico del governo USA, 256 px, EPSG:3857, livelli 0-23 (dati fino a ~z16). L'ordine ArcGIS è z/y/x - il plugin accetta qualsiasi ordine dei token. Credito: «USDA, USGS The National Map: Orthoimagery». |
| LINZ Basemaps aerial (NZ) | `https://basemaps.linz.govt.nz/v1/tiles/aerial/3857/{z}/{x}/{y}.png?api=YOUR_KEY` | CC BY 4.0, serve una chiave API gratuita; attribuzione a LINZ obbligatoria. |
| EOX Sentinel-2 cloudless | vedi eox.at / cloudless.eox.at | ~10 m, solo uso non commerciale, attribuzione obbligatoria. |
| OpenAerialMap | tessere per singola immagine | CC BY 4.0, copertura a macchia. |
| Bing | non supportato | Le condizioni vietano gli URL diretti delle tessere. |
