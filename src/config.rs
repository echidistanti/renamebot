//! Impostazioni dell'utente, in `config.json` nella cartella dati.
//! Le chiavi API stanno in chiaro nel file, con permessi ristretti dove il sistema li supporta.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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
        }
    }
}

impl Config {
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("Bobina").join("config.json"))
    }

    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(
            path,
            serde_json::to_string_pretty(self).map_err(io::Error::other)?,
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
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

        fs::write(&p, r#"{"tmdb_key":"x"}"#).unwrap();
        let l = Config::load(&p);
        assert_eq!(l.tmdb_key, "x");
        assert_eq!(l.meta_lang, "it");
        assert_eq!(Config::load(&d.path().join("no.json")), Config::default());
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
