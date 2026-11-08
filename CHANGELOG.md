# Registro delle modifiche

Tutte le modifiche rilevanti di Raster Tools sono annotate qui.
Formato: [Keep a Changelog](https://keepachangelog.com/it/1.1.0/).
Numerazione: [Semantic Versioning](https://semver.org/lang/it/) —
`0.x.y` indica una versione iniziale, ancora in fase di test.

Le note di ogni release su GitHub vengono prese automaticamente dalla sezione
della versione corrispondente. Scrivi le novità sotto **[Non rilasciato]**:
lo script `scripts/release.ps1` le sposta nella nuova versione al momento
del rilascio.

## [Non rilasciato]

## [0.6.1] - 2026-11-08

### Corretto
- **Il pulsante "Rendi trasparente" non faceva nulla.** La finestra si apriva, ma
  il CAD la chiudeva subito "dietro le quinte" e il pulsante restava senza effetto.
  Lo stesso difetto poteva bloccare anche altri passaggi aperti dopo aver scelto
  l'immagine: "Inserisci immagine" (scelta del punto) e "Dimensioni / DPI" o
  "Cambia percorso" senza immagine selezionata prima.
  *Fixed: the "Make transparent" button did nothing; the same flaw could also
  stop "Insert image" placement and the Size / Change path dialogs opened after
  picking an image.*

## [0.6.0] - 2026-11-08

### Modificato
- **Compatibile con Open CAD Studio 2026.45** (Plugin API 16). La 2026.45 rifiuta
  i plugin compilati per la versione precedente (all'avvio dava errore sui
  plugin). Questa versione funziona **solo con la 2026.45**: chi usa ancora la
  2026.44 resti alla 0.5.1.
  *Built for Open CAD Studio 2026.45 (API 16); for 2026.44 stay on 0.5.1.*

## [0.5.1] - 2026-11-08

### Corretto
- **Colore trasparente su scansioni grandi.** Con immagini molto grandi (per esempio
  una mappa catastale da 7000 × 10000 pixel) il comando si interrompeva senza
  messaggi: salvare la copia PNG richiedeva più dei 30 secondi che Open CAD Studio
  concede a un comando del plugin. Ora il salvataggio è molto più veloce, il plugin
  tiene informato il CAD mentre lavora e mostra un messaggio "attendere…" per le
  immagini grandi; usa anche meno memoria.
- Se la stessa immagine è inserita più volte nel disegno (copie sovrapposte),
  `RT_TRANSP` ricollega alla copia trasparente tutte le copie dello stesso file:
  prima una copia ancora opaca poteva coprire quella resa trasparente.
  *Transparent colour now works on large scans (fast PNG saving, no 30-second
  timeout) and on images inserted several times.*

## [0.5.0] - 2026-11-08

### Aggiunto
- **Colore trasparente** (`RT_TRANSP`, scheda Raster › Trasparenza): si sceglie
  l'immagine, poi si clicca **direttamente sull'immagine** il colore da rendere
  trasparente (per esempio lo sfondo bianco di una scansione). Una finestra mostra
  il colore scelto e chiede la **tolleranza** (per i colori simili; 10% proposto per
  JPG, 2% per gli altri formati) e se agire su tutta l'immagine o **solo sull'area
  collegata al punto cliccato**. Il plugin salva una copia PNG con la trasparenza
  accanto all'originale (`<nome>_trasp-N.png`, l'originale non viene toccato),
  ricollega l'immagine alla copia e attiva la trasparenza. Il comando si può
  ripetere per togliere altri colori.
- `RT_TRANSPOFF` (Togli trasparenza): spegne la trasparenza delle immagini.
  *Transparent colour: `RT_TRANSP` picks an image, then the colour to drop with a
  click on the image (tolerance, whole image or connected area only); a PNG copy
  is saved next to the original. `RT_TRANSPOFF` turns transparency off.*

## [0.4.0] - 2026-11-05

### Aggiunto
- **Inglese e italiano.** Il plugin parla inglese, oppure italiano quando Open CAD
  Studio è in italiano (con l'impostazione "Sistema", quando Windows è in italiano).
  Messaggi, richieste, finestre ed etichette della barra seguono la lingua; il nuovo
  comando `RT_LANG IT|EN|AUTO` la forza a mano.
  *English and Italian: the plugin follows Open CAD Studio's language (English by
  default); `RT_LANG IT|EN|AUTO` forces one.*
- README e riferimento dei comandi in inglese (`README.md`, `PLUGIN.md`) e in
  italiano (`README.it.md`, `PLUGIN.it.md`), con l'avvertenza "progetto amatoriale".

## [0.3.1] - 2026-11-05

### Corretto
- La compilazione su GitHub non riusciva più: la libreria DWG `acadrust`, da cui
  dipende Open CAD Studio, è stata spostata in un altro repository. Il plugin ora la
  scarica dal nuovo indirizzo (stessa versione, nessun cambiamento di funzioni).
  Questa versione pubblica anche l'inserimento di immagini della 0.3.0.

## [0.3.0] - 2026-11-05

### Aggiunto
- **Inserimento di immagini** nel disegno (`RT_ATTACH`, pulsante *Inserisci immagine*):
  si sceglie il file (GIF, JPG, PNG, BMP, TIFF, WebP) e la dimensione — indicata a
  schermo con due punti (proporzionale), per larghezza, per risoluzione DPI oppure
  1 pixel = 1 unità — poi si clicca il punto di inserimento. L'immagine va sul layer
  corrente e il file resta collegato; l'unità per i DPI è proposta in base alle
  unità del disegno.

## [0.2.0] - 2026-11-04

### Aggiunto
- Ritaglio, foro e separazione con un **poligono** disegnato a clic (`RT_CLIPPOLY`,
  `RT_HOLEPOLY`, `RT_CUTPOLY`): Invio o un clic sul primo vertice chiudono il poligono.
- Ritaglio, foro e separazione con una **polilinea chiusa**, un cerchio o un'ellisse
  già presenti nel disegno (`RT_CLIPPL`, `RT_HOLEPL`, `RT_CUTPL`); i lati ad arco
  delle polilinee sono seguiti fedelmente. Si sceglie la polilinea cliccandola o
  cliccando al suo interno; se immagine e polilinea sono già selezionate il ritaglio
  è immediato.
- Nella scheda Raster i pulsanti Ritaglia, Foro e Separa pezzo diventano menu con
  tre scelte: Rettangolo, Poligono, Da polilinea.

### Modificato
- Un nuovo ritaglio su un'immagine già ritagliata restringe il ritaglio anche con i
  poligoni (se entrambi i contorni sono concavi, il nuovo sostituisce il vecchio).
- I contorni che si intersecano con se stessi vengono rifiutati con un messaggio chiaro.

## [0.1.0] - 2026-11-04

Prima versione pubblica, in fase di test.

### Aggiunto
- Ritaglio rettangolare che tiene l'interno (`RT_CLIP`); ripetuto, restringe il ritaglio.
- Foro rettangolare che nasconde l'interno (`RT_CLIPHOLE`).
- Separazione di un pezzo in una nuova immagine spostabile (`RT_CUT`).
- Rimozione del ritaglio (`RT_UNCLIP`).
- Ridimensionamento proporzionale trascinando un angolo (`RT_RESIZE`).
- Ridimensionamento per larghezza, altezza, fattore di scala o risoluzione DPI (`RT_SIZE`).
- Cambio del file su disco, mantenendo la larghezza nel disegno (`RT_PATH`).
- Ricerca in una cartella dei file spostati (`RT_FIND`).
- Elenco delle immagini con percorso, pixel, dimensioni e stato del file (`RT_LIST`).
- Formati: GIF, JPG, PNG, BMP, TIFF, WebP.
- Compatibile con Open CAD Studio v2026.44 (Plugin API 14).

