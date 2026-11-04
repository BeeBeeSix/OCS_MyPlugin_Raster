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

