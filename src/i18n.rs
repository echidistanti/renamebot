//! Traduzioni dell'interfaccia. Per aggiungere una lingua: una variante in `Lang`,
//! una voce in `Lang::ALL`/`name` e un ramo in `tr`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    It,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::It, Lang::En];

    pub fn name(self) -> &'static str {
        match self {
            Lang::It => "Italiano",
            Lang::En => "English",
        }
    }
}

/// Restituisce il testo per `key` nella lingua scelta. Una chiave sconosciuta torna com'è.
pub fn tr(lang: Lang, key: &'static str) -> &'static str {
    let (it, en) = match key {
        "title" => ("Bobina", "Bobina"),
        "add_files" => ("Aggiungi file…", "Add files…"),
        "add_folder" => ("Aggiungi cartella…", "Add folder…"),
        "clear" => ("Svuota", "Clear"),
        "drop_hint" => ("Trascina qui file o cartelle", "Drop files or folders here"),
        "mask" => ("Maschera sul nome attuale", "Mask on current name"),
        "mask_hint" => (
            "vuota = riconoscimento automatico, es. {n} - S{s}E{e} - *",
            "empty = automatic detection, e.g. {n} - S{s}E{e} - *",
        ),
        "preset" => ("Preset", "Preset"),
        "fmt_episode" => ("Formato serie", "Series format"),
        "fmt_movie" => ("Formato film", "Movie format"),
        "dest" => ("Cartella di destinazione", "Destination folder"),
        "dest_hint" => ("vuota = accanto al file", "empty = next to the file"),
        "col_current" => ("Nome attuale", "Current name"),
        "col_new" => ("Nuovo nome", "New name"),
        "col_status" => ("Stato", "Status"),
        "st_ok" => ("pronto", "ready"),
        "st_same" => ("invariato", "unchanged"),
        "st_exists" => ("esiste già", "already exists"),
        "st_dup" => ("duplicato", "duplicate"),
        "st_missing" => ("manca", "missing"),
        "st_nomatch" => ("maschera non corrisponde", "mask does not match"),
        "language" => ("Lingua", "Language"),
        "errors" => ("Errore nel formato", "Format error"),
        "action" => ("Azione", "Action"),
        "act_move" => ("Sposta / rinomina", "Move / rename"),
        "act_copy" => ("Copia", "Copy"),
        "sidecars" => (
            "Sposta anche sottotitoli e .nfo collegati",
            "Also move linked subtitles and .nfo files",
        ),
        "apply" => ("Applica", "Apply"),
        "undo" => ("Annulla ultima operazione", "Undo last operation"),
        "msg_done" => ("file elaborati", "files processed"),
        "msg_errors" => ("errori", "errors"),
        "msg_undone" => ("operazione annullata", "operation undone"),
        "settings" => ("Impostazioni", "Settings"),
        "tmdb_key" => ("Chiave API TMDB", "TMDB API key"),
        "provider" => ("Database serie", "Series database"),
        "meta_lang" => ("Lingua dei metadati", "Metadata language"),
        "save" => ("Salva", "Save"),
        "online" => ("Riconoscimento online", "Online matching"),
        "search" => ("Cerca online", "Search online"),
        "searching" => ("ricerca in corso…", "searching…"),
        "tab_rename" => ("Rinomina", "Rename"),
        "tab_subs" => ("Sottotitoli", "Subtitles"),
        "sub_lang" => ("Lingua dei sottotitoli", "Subtitle language"),
        "sub_lang_pick" => ("scegli una lingua…", "choose a language…"),
        "sub_search" => ("Cerca sottotitoli", "Search subtitles"),
        "sub_download" => ("Scarica selezionati", "Download selected"),
        "sub_found" => ("trovato", "found"),
        "sub_none" => ("nessun sottotitolo", "no subtitles"),
        "sub_saved" => ("salvato", "saved"),
        "sub_downloading" => ("download in corso…", "downloading…"),
        "os_key" => ("Chiave API OpenSubtitles", "OpenSubtitles API key"),
        "os_user" => ("Account OpenSubtitles", "OpenSubtitles username"),
        "os_pass" => ("Password OpenSubtitles", "OpenSubtitles password"),
        "subs_hint" => (
            "Scegli la lingua e premi «Cerca sottotitoli»",
            "Choose a language and press “Search subtitles”",
        ),
        "files_count" => ("file", "files"),
        _ => return key,
    };
    match lang {
        Lang::It => it,
        Lang::En => en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduce_e_ripiega_sulla_chiave() {
        assert_eq!(tr(Lang::It, "clear"), "Svuota");
        assert_eq!(tr(Lang::En, "clear"), "Clear");
        assert_eq!(tr(Lang::It, "chiave_inesistente"), "chiave_inesistente");
    }
}
