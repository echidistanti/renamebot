# Bobina

Rinominatore di film e serie TV con anteprima, scritto in Rust con GUI nativa (egui).
Windows e Linux subito, macOS in seguito. Riscrittura indipendente, ispirata al comportamento di FileBot Pro.

## Stato

Scheletro funzionante:

- Template dei nomi con segnaposto e filtri: `{n}{y|prefix: (|suffix:)}`
- Maschere a segnaposto sul nome attuale: `{n} - S{s}E{e} - *`
- Riconoscimento automatico di titolo, anno, stagione ed episodio dal nome file
- Anteprima con rilevamento di conflitti (esiste già, duplicato, campi mancanti)
- Applica (sposta/rinomina o copia), file collegati (`.srt`, `.ass`, `.nfo`…) che seguono il video, mai sovrascritture
- Annulla persistente (registro in `history.json` nella cartella dati utente), ultimi 50 lotti
- Riconoscimento online (TMDB per film e serie, TVmaze per serie): ricerca per gruppo di file, miglior candidato scelto anche per anno, elenco per cambiare scelta, titoli degli episodi; cache su disco di 7 giorni
- Impostazioni (chiave TMDB, database, lingua dei metadati) in `config.json` nella cartella dati utente
- Sottotitoli da OpenSubtitles.com: ricerca per hash del file e per nome nella lingua scelta, anteprima dei candidati (release, scaricamenti, corrispondenza hash), download accanto al video come `Nome.<lingua>.srt`, mai sovrascrivere. Servono chiave API, account e password nelle impostazioni
- Preset Plex, Jellyfin, Kodi; interfaccia in italiano e inglese; drag and drop

I client TMDB, TVmaze e OpenSubtitles sono testati su risposte registrate e vanno ancora provati dal vivo con le chiavi.

## Linux

I dialoghi «Aggiungi file/cartella» usano xdg-desktop-portal: su desktop senza portale resta il trascinamento.

## Release

La CI (`.github/workflows/ci.yml`) compila e lancia i test su Linux e Windows a ogni push su `main`.
Un tag `vX.Y.Z` avvia `release.yml`, che crea una release in bozza con installer e zip per Windows e AppImage per Linux.

## Sviluppo

    cargo test
    cargo run

## Licenza

MIT
