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

