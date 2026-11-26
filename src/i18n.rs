//! Traduzioni dell'interfaccia. Per aggiungere una lingua: una variante in `Lang`,
//! una voce in `Lang::ALL`/`name` e un ramo in `tr`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
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
        // Barra del titolo e barra in alto
        "minimize" => ("Riduci a icona", "Minimize"),
        "maximize" => ("Ingrandisci", "Maximize"),
        "restore" => ("Ripristina", "Restore"),
        "add_files" => ("Aggiungi file", "Add files"),
        "add_folder" => ("Aggiungi cartella", "Add folder"),
        "tip_add_files" => (
            "Scegli uno o più video (Ctrl+O)",
            "Pick one or more videos (Ctrl+O)",
        ),
        "tip_add_folder" => (
            "Aggiunge tutti i video della cartella, sottocartelle comprese",
            "Adds every video in the folder, subfolders included",
        ),
        "options" => ("Opzioni", "Options"),
        "tab_rename" => ("Rinomina", "Rename"),
        "tab_subs" => ("Sottotitoli", "Subtitles"),
        "clear" => ("Svuota elenco", "Clear list"),
        "files_count" => ("file", "files"),
        // Elenco vuoto
        "drop_title" => ("Trascina qui film e serie", "Drop films and series here"),
        "drop_release" => ("Rilascia per aggiungere", "Release to add"),
        "drop_body" => (
            "File singoli o cartelle intere: i video li cerco anche nelle sottocartelle.",
            "Single files or whole folders: videos in subfolders are found too.",
        ),
        "open_title" => ("Aggiungi film e serie", "Add films and series"),
        "open_body" => (
            "Scegli singoli file o cartelle intere: i video li cerco anche nelle sottocartelle.",
            "Pick single files or whole folders: videos in subfolders are found too.",
        ),
        // Lettura dei nomi
        "card_reading" => ("Lettura dei nomi", "Reading the names"),
        "mask" => ("Maschera (facoltativa)", "Mask (optional)"),
        "mask_hint" => ("es. {n} - S{s}E{e} - *", "e.g. {n} - S{s}E{e} - *"),
        "mask_help" => (
            "Vuota: titolo, anno, stagione ed episodio li riconosco da solo. Con una maschera indichi dove stanno; * salta il resto.",
            "Empty: title, year, season and episode are recognised automatically. A mask says where they are; * skips the rest.",
        ),
        "online" => ("Completa con i dati online", "Fill in from online data"),
        "online_help" => (
            "Titoli ufficiali e nomi degli episodi da TMDB per i film e dal database scelto nelle opzioni per le serie.",
            "Official titles and episode names from TMDB for films and from the database chosen in the options for series.",
        ),
        "search_again" => ("Cerca di nuovo", "Search again"),
        "searching" => ("ricerca in corso…", "searching…"),
        // Formato
        "card_format" => ("Formato dei nomi", "Name format"),
        "preset" => ("Preset", "Preset"),
        "preset_custom" => ("Personalizzato", "Custom"),
        "preset_save_as" => ("Salva come…", "Save as…"),
        "preset_name" => ("Nome del preset", "Preset name"),
        "preset_delete" => ("Elimina", "Delete"),
        "save" => ("Salva", "Save"),
        "fmt_episode" => ("Serie", "Series"),
        "fmt_movie" => ("Film", "Films"),
        "errors" => ("Errore nel formato", "Format error"),
        "placeholders" => ("Segnaposto e filtri", "Placeholders and filters"),
        "ph_n" => (
            "titolo del film o della serie",
            "title of the film or series",
        ),
        "ph_y" => ("anno", "year"),
        "ph_se" => ("stagione ed episodio (2, 7)", "season and episode (2, 7)"),
        "ph_se00" => (
            "stagione ed episodio a due cifre (02, 07)",
            "two-digit season and episode (02, 07)",
        ),
        "ph_s00e00" => ("S02E07", "S02E07"),
        "ph_sxe" => ("2x07", "2x07"),
        "ph_t" => ("titolo dell'episodio", "episode title"),
        "ph_slash" => ("separa le cartelle", "separates folders"),
        "ph_case" => (
            "maiuscole, minuscole, iniziali maiuscole",
            "upper case, lower case, title case",
        ),
        "ph_default" => (
            "testo da usare se il dato manca",
            "text to use when the value is missing",
        ),
        "ph_affix" => (
            "testo prima o dopo, solo se il dato c'è",
            "text before or after, only when the value exists",
        ),
        // Anteprima
        "card_preview" => ("Anteprima", "Preview"),
        "sum_ready" => ("pronti", "ready"),
        "sum_review" => ("da rivedere", "to review"),
        "sum_same" => ("invariati", "unchanged"),
        "st_ok" => ("Pronto", "Ready"),
        "st_same" => ("Invariato", "Unchanged"),
        "st_exists" => ("Esiste già", "Already exists"),
        "st_dup" => ("Nome doppio", "Duplicate name"),
        "st_missing" => ("Manca", "Missing"),
        "st_nomatch" => ("Non riconosciuto", "Not recognised"),
        "tip_ok" => ("Verrà rinominato così.", "It will be renamed like this."),
        "tip_same" => ("Il nome è già quello giusto.", "The name is already right."),
        "tip_exists" => (
            "C'è già un file con questo nome: cambia il formato o la destinazione.",
            "A file with this name already exists: change the format or the destination.",
        ),
        "tip_dup" => (
            "Un altro file avrebbe lo stesso nome: togline uno dall'elenco, correggi i dati con la matita o rendi il formato più preciso (anno, titolo dell'episodio).",
            "Another file would get the same name: remove one from the list, fix the data with the pencil or make the format more specific (year, episode title).",
        ),
        "tip_missing" => (
            "Il formato usa un dato che per questo file manca: aggiungilo con la matita, attiva i dati online oppure rendi il campo facoltativo con |prefix: o |default:.",
            "The format uses a value this file lacks: add it with the pencil, turn on online data or make the field optional with |prefix: or |default:.",
        ),
        "tip_nomatch" => (
            "La maschera non corrisponde al nome di questo file: correggila, oppure inserisci i dati con la matita.",
            "The mask does not match this file name: fix it, or enter the data with the pencil.",
        ),
        "fix_exists" => (
            "cambia formato o destinazione",
            "change the format or the destination",
        ),
        "fix_dup" => (
            "togline uno o rendi il formato più preciso",
            "remove one or make the format more specific",
        ),
        "fix_missing" => (
            "aggiungilo con la matita o attiva i dati online",
            "add it with the pencil or turn on online data",
        ),
        "fix_nomatch" => (
            "correggi la maschera o i dati con la matita",
            "fix the mask, or the data with the pencil",
        ),
        "manual" => ("dati corretti a mano", "data fixed by hand"),
        "edit" => (
            "Correggi i dati di questo file",
            "Fix the data of this file",
        ),
        "edit_title" => ("Correggi i dati", "Fix the data"),
        "edit_help" => (
            "Valgono solo per questo file e prendono il posto di quelli riconosciuti. Lascia vuoto ciò che manca.",
            "They apply to this file only and replace the recognised ones. Leave empty what is missing.",
        ),
        "f_n" => ("Titolo", "Title"),
        "f_y" => ("Anno", "Year"),
        "f_s" => ("Stagione", "Season"),
        "f_e" => ("Episodio", "Episode"),
        "f_t" => ("Titolo dell'episodio", "Episode title"),
        "f_invalid" => ("deve essere un numero", "must be a number"),
        "f_result" => ("Diventerà", "It will become"),
        "reset" => ("Ripristina", "Reset"),
        "cancel" => ("Annulla", "Cancel"),
        "apply_edit" => ("Applica", "Apply"),
        "from" => ("da", "from"),
        "remove" => ("Togli dall'elenco", "Remove from the list"),
        // Barra in basso
        "dest" => ("Destinazione", "Destination"),
        "dest_next" => ("Accanto ai file", "Next to the files"),
        "dest_pick" => ("Scegli la cartella", "Choose the folder"),
        "act_move" => ("Sposta", "Move"),
        "act_copy" => ("Copia", "Copy"),
        "sidecars" => ("Con sottotitoli e .nfo", "With subtitles and .nfo"),
        "tip_sidecars" => (
            "Porta con il video i sottotitoli e i .nfo che hanno il suo stesso nome",
            "Takes along the subtitles and .nfo files that share the video's name",
        ),
        "do_rename" => ("Rinomina", "Rename"),
        "do_copy" => ("Copia", "Copy"),
        "undo" => ("Annulla l'ultima", "Undo last"),
        "tip_undo" => (
            "Rimette i file com'erano prima dell'ultima operazione, anche dopo un riavvio",
            "Puts the files back as they were before the last operation, even after a restart",
        ),
        // Avanzamento ed esito
        "working" => ("Rinomina in corso", "Renaming"),
        "undoing" => ("Annullamento in corso", "Undoing"),
        "done" => ("Fatto", "Done"),
        "attention" => ("Attenzione", "Warning"),
        "msg_done" => ("file elaborati", "files processed"),
        "msg_errors" => ("errori", "errors"),
        "msg_undone" => ("operazione annullata", "operation undone"),
        "dismiss" => ("Chiudi", "Dismiss"),
        // Sottotitoli
        "sub_lang" => ("Lingua", "Language"),
        "sub_lang_pick" => ("scegli una lingua…", "choose a language…"),
        "sub_search" => ("Cerca sottotitoli", "Search subtitles"),
        "sub_download" => ("Scarica", "Download"),
        "subs_word" => ("sottotitoli", "subtitles"),
        "card_subs" => ("Sottotitoli", "Subtitles"),
        "sub_found" => ("Trovato", "Found"),
        "sum_found" => ("trovati", "found"),
        "sum_none" => ("senza sottotitoli", "without subtitles"),
        "sub_error" => ("Errore", "Error"),
        "sub_none" => ("Nessuno", "None"),
        "sub_saved" => ("Salvato", "Saved"),
        "sub_downloading" => ("Download…", "Downloading…"),
        "subs_searching" => ("Ricerca dei sottotitoli", "Searching subtitles"),
        "subs_downloading" => ("Download dei sottotitoli", "Downloading subtitles"),
        "subs_hint_title" => ("Sottotitoli per i tuoi file", "Subtitles for your files"),
        "subs_hint" => (
            "Scegli la lingua qui sotto e premi «Cerca sottotitoli». Prima di scaricare potrai scegliere quale versione prendere.",
            "Choose the language below and press “Search subtitles”. Before downloading you can pick which version to take.",
        ),
        // Opzioni
        "ui_lang" => ("Lingua dell'interfaccia", "Interface language"),
        "provider" => ("Database delle serie", "Series database"),
        "meta_lang" => ("Lingua dei titoli", "Title language"),
        "tmdb_key" => ("Chiave API TMDB", "TMDB API key"),
        "tmdb_help" => (
            "Gratuita, da themoviedb.org: Impostazioni → API.",
            "Free, from themoviedb.org: Settings → API.",
        ),
        "os_title" => ("OpenSubtitles", "OpenSubtitles"),
        "os_key" => ("Chiave API", "API key"),
        "os_user" => ("Utente", "Username"),
        "os_pass" => ("Password", "Password"),
        "os_help" => (
            "La chiave basta per cercare; utente e password servono per scaricare.",
            "The key is enough to search; username and password are needed to download.",
        ),
        "close" => ("Chiudi", "Close"),
        "msg_history_not_saved" => ("registro non salvato", "history not saved"),
        "msg_settings_not_saved" => ("impostazioni non salvate", "settings not saved"),
        "err_tmdb_key_missing" => (
            "chiave TMDB mancante: inseriscila nelle Opzioni",
            "TMDB key missing: add it in Options",
        ),
        "err_tmdb_key_invalid" => ("chiave TMDB non valida", "invalid TMDB key"),
        "err_os_key_missing" => (
            "chiave OpenSubtitles mancante: inseriscila nelle Opzioni",
            "OpenSubtitles key missing: add it in Options",
        ),
        "err_os_key_invalid" => (
            "chiave API OpenSubtitles non valida",
            "invalid OpenSubtitles API key",
        ),
        "err_os_account_missing" => (
            "account OpenSubtitles mancante: serve per scaricare",
            "OpenSubtitles account missing: needed to download",
        ),
        "err_os_login" => (
            "credenziali OpenSubtitles non valide",
            "invalid OpenSubtitles credentials",
        ),
        "err_os_daily_limit" => (
            "limite di download giornaliero raggiunto",
            "daily download limit reached",
        ),
        "err_too_many_requests" => (
            "troppe richieste, riprova tra poco",
            "too many requests, try again shortly",
        ),
        "err_no_results" => ("nessun risultato", "no results"),
        "err_nothing_to_search" => (
            "niente da cercare: manca il titolo",
            "nothing to search: title missing",
        ),
        "err_name_not_recognized" => ("nome non riconosciuto", "name not recognized"),
        "err_target_exists" => ("la destinazione esiste già", "destination already exists"),
        "err_original_exists" => ("l'originale esiste già", "the original already exists"),
        "err_moved_file_gone" => ("il file spostato non c'è più", "the moved file is gone"),
        "err_file_too_small" => (
            "file troppo piccolo per l'hash",
            "file too small for hashing",
        ),
        "err_http" => ("errore HTTP", "HTTP error"),
        "err_network" => ("errore di rete", "network error"),
        "err_bad_response" => ("risposta non valida", "invalid response"),
        "err_io" => ("errore sul disco", "disk error"),
        "err_unclosed" => ("graffa non chiusa", "unclosed brace"),
        "err_unknown_placeholder" => ("segnaposto sconosciuto", "unknown placeholder"),
        "err_unknown_filter" => ("filtro sconosciuto", "unknown filter"),
        "err_empty_expression" => ("espressione vuota", "empty expression"),
        "err_mask_placeholder" => (
            "segnaposto non valido nella maschera",
            "invalid placeholder in mask",
        ),
        "err_duplicate_placeholder" => ("segnaposto ripetuto", "repeated placeholder"),
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
        assert_eq!(tr(Lang::It, "clear"), "Svuota elenco");
        assert_eq!(tr(Lang::En, "clear"), "Clear list");
        assert_eq!(tr(Lang::It, "chiave_inesistente"), "chiave_inesistente");
    }
}
