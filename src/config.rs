//! Impostazioni dell'utente, in `config.json` nella cartella dati.
//! Le chiavi API stanno in chiaro nel file, con permessi ristretti dove il sistema li supporta.

use crate::fsx::write_atomic;
use crate::i18n::Lang;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Cartella dati dell'utente (impostazioni e registro).
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|base| data_dir_in(&base))
}

/// Fino alla 0.1.0 il programma si chiamava Bobina: se c'è la sua cartella, con i nostri file,
/// e quella nuova non esiste ancora, la si rinomina.
fn data_dir_in(base: &Path) -> PathBuf {
    let dir = base.join("RenameBot");
    let old = base.join("Bobina");
    let ours = old.join("config.json").exists() || old.join("history.json").exists();
    if !dir.exists() && ours {
        let _ = fs::rename(&old, &dir);
    }
    dir
}

/// Coppia di formati con un nome: quelli integrati più quelli salvati dall'utente.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub episode: String,
    pub movie: String,
}

const MOVIE: &str = "Movies/{n}{y|prefix: (|suffix:)}/{n}{y|prefix: (|suffix:)}";

pub fn builtin_presets() -> Vec<Preset> {
    let p = |name: &str, episode: &str| Preset {
        name: name.into(),
        episode: episode.into(),
        movie: MOVIE.into(),
    };
    vec![
        p(
            "Plex",
            "TV Shows/{n}{y|prefix: (|suffix:)}/Season {s00}/{n} - {s00e00}{t|prefix: - }",
        ),
        p(
            "Jellyfin",
            "Shows/{n}{y|prefix: (|suffix:)}/Season {s}/{n} S{s00}E{e00}{t|prefix: - }",
        ),
        p(
            "Kodi",
            "TV Shows/{n}/Season {s}/{n} - {s00e00}{t|prefix: - }",
        ),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Provider {
    #[default]
    Tmdb,
    Tvmaze,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Tmdb, Provider::Tvmaze];

    pub fn name(self) -> &'static str {
        match self {
            Provider::Tmdb => "TheMovieDB",
            Provider::Tvmaze => "TVmaze",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub tmdb_key: String,
    pub provider: Provider,
    /// Lingua dei metadati: codice a due lettere ("it", "en").
    pub meta_lang: String,
    /// Chiave API di OpenSubtitles.com, account e password per scaricare.
    pub os_api_key: String,
    pub os_username: String,
    pub os_password: String,
    /// Stato dell'interfaccia, ripreso al riavvio.
    pub ui_lang: Lang,
    pub mask: String,
    pub fmt_episode: String,
    pub fmt_movie: String,
    pub dest: String,
    pub presets: Vec<Preset>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tmdb_key: String::new(),
            provider: Provider::Tmdb,
            meta_lang: "it".into(),
            os_api_key: String::new(),
            os_username: String::new(),
            os_password: String::new(),
            ui_lang: Lang::It,
            mask: String::new(),
            fmt_episode: builtin_presets()[0].episode.clone(),
            fmt_movie: builtin_presets()[0].movie.clone(),
            dest: String::new(),
            presets: Vec::new(),
        }
    }
}

impl Config {
    pub fn default_path() -> Option<PathBuf> {
        data_dir().map(|d| d.join("config.json"))
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Scrittura atomica; il file nasce leggibile solo dall'utente perché contiene le chiavi.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        write_atomic(path, &json, true)
    }

    /// Salva un preset personale; uno con lo stesso nome viene sostituito.
    pub fn upsert_preset(&mut self, preset: Preset) {
        match self.presets.iter_mut().find(|p| p.name == preset.name) {
            Some(p) => *p = preset,
            None => self.presets.push(preset),
        }
    }

    /// Codice lingua nel formato richiesto da TMDB ("it-IT").
    pub fn tmdb_lang(&self) -> String {
        match self.meta_lang.as_str() {
            "it" => "it-IT".into(),
            "en" => "en-US".into(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salva_carica_e_default_per_campi_mancanti() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c/config.json");
        let c = Config {
            tmdb_key: "abc".into(),
            provider: Provider::Tvmaze,
            meta_lang: "en".into(),
            os_username: "mario".into(),
            ..Default::default()
        };
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p), c);
        assert_eq!(c.tmdb_lang(), "en-US");

        fs::write(&p, r#"{"tmdb_key":"x","ui_lang":"en"}"#).unwrap();
        let l = Config::load(&p);
        assert_eq!(l.tmdb_key, "x");
        assert_eq!(l.meta_lang, "it");
        assert_eq!(l.ui_lang, Lang::En);
        assert_eq!(
            l.fmt_episode,
            builtin_presets()[0].episode,
            "formato mancante: quello di Plex"
        );
        assert_eq!(Config::load(&d.path().join("no.json")), Config::default());
    }

    #[test]
    fn la_cartella_di_bobina_diventa_quella_di_renamebot() {
        let d = tempfile::tempdir().unwrap();
        let old = d.path().join("Bobina");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("history.json"), "{}").unwrap();
        let dir = data_dir_in(d.path());
        assert!(dir.join("history.json").exists() && !old.exists());

        // Una cartella Bobina senza i nostri file non è nostra: resta dov'è.
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("Bobina/altro")).unwrap();
        data_dir_in(d.path());
        assert!(d.path().join("Bobina/altro").exists());
    }

    #[test]
    fn preset_personali() {
        let mut c = Config::default();
        let p = |e: &str| Preset {
            name: "Mio".into(),
            episode: e.into(),
            movie: "{n}".into(),
        };
        c.upsert_preset(p("{n} {s00e00}"));
        c.upsert_preset(p("{n} {sxe}"));
        assert_eq!(c.presets, vec![p("{n} {sxe}")]);
    }

    #[cfg(unix)]
    #[test]
    fn permessi_ristretti() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        Config::default().save(&p).unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
