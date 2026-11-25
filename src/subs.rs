//! Sottotitoli da OpenSubtitles.com (API REST v1): ricerca per hash e per nome, download.
//!
//! La ricerca funziona con la sola chiave API. Il download richiede anche account e password.

use crate::config::Config;
use crate::error::Error;
use crate::http::{Http, Method, encode};
use crate::template::MediaInfo;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const BASE: &str = "https://api.opensubtitles.com/api/v1";

#[derive(Debug, Clone, PartialEq)]
pub struct SubCandidate {
    pub file_id: u64,
    pub file_name: String,
    pub release: String,
    pub language: String,
    pub downloads: u64,
    pub hash_match: bool,
    pub hearing_impaired: bool,
}

impl SubCandidate {
    /// Riga di anteprima per la scelta: release, lingua, scaricamenti e se coincide con l'hash del file.
    pub fn label(&self) -> String {
        let mut s = format!("{} · {} · {}↓", self.release, self.language, self.downloads);
        if self.hash_match {
            s.push_str(" · ✓ hash");
        }
        if self.hearing_impaired {
            s.push_str(" · HI");
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub token: String,
    pub base: String,
}

/// Hash di OpenSubtitles: dimensione più somma delle parole a 64 bit dei primi e ultimi 64 KiB.
pub fn moviehash(path: &Path) -> Result<String, Error> {
    const CH: u64 = 65536;
    let mut f = File::open(path)?;
    let size = f.metadata()?.len();
    if size < CH * 2 {
        return Err(Error::FileTooSmall);
    }
    let mut hash = size;
    let mut buf = vec![0u8; CH as usize];
    for off in [0, size - CH] {
        f.seek(SeekFrom::Start(off))?;
        f.read_exact(&mut buf)?;
        for c in buf.chunks_exact(8) {
            hash = hash.wrapping_add(u64::from_le_bytes(c.try_into().expect("blocchi da 8 byte")));
        }
    }
    Ok(format!("{hash:016x}"))
}

fn json(body: &[u8]) -> Result<Value, Error> {
    serde_json::from_slice(body).map_err(|e| Error::BadResponse(e.to_string()))
}

pub fn parse_search(body: &[u8]) -> Result<Vec<SubCandidate>, Error> {
    let v = json(body)?;
    Ok(v["data"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|d| {
            let a = &d["attributes"];
            let file = a["files"].as_array()?.first()?;
            Some(SubCandidate {
                file_id: file["file_id"].as_u64()?,
                file_name: file["file_name"].as_str().unwrap_or("sub.srt").to_string(),
                release: a["release"].as_str().unwrap_or("?").to_string(),
                language: a["language"].as_str().unwrap_or("?").to_string(),
                downloads: a["download_count"].as_u64().unwrap_or(0),
                hash_match: a["moviehash_match"].as_bool().unwrap_or(false),
                hearing_impaired: a["hearing_impaired"].as_bool().unwrap_or(false),
            })
        })
        .collect())
}

pub fn parse_login(body: &[u8]) -> Result<Session, Error> {
    let v = json(body)?;
    let token = v["token"]
        .as_str()
        .ok_or(Error::BadResponse("token".into()))?
        .to_string();
    let base = match v["base_url"].as_str().filter(|b| !b.is_empty()) {
        Some(b) => format!(
            "https://{}/api/v1",
            b.trim_start_matches("https://").trim_end_matches('/')
        ),
        None => BASE.to_string(),
    };
    Ok(Session { token, base })
}

pub fn parse_download(body: &[u8]) -> Result<(String, String), Error> {
    let v = json(body)?;
    let link = v["link"]
        .as_str()
        .ok_or(Error::BadResponse("link".into()))?;
    let name = v["file_name"].as_str().unwrap_or("sub.srt");
    Ok((link.to_string(), name.to_string()))
}

/// Percorso del sottotitolo accanto al video: `NomeVideo.<lingua>.<estensione>`.
pub fn subtitle_path(video: &Path, lang: &str, file_name: &str) -> PathBuf {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_lowercase)
        .unwrap_or_else(|| "srt".into());
    let lang: String = lang
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let stem = video.file_stem().and_then(|s| s.to_str()).unwrap_or("sub");
    video.with_file_name(format!("{stem}.{lang}.{ext}"))
}

pub struct Client {
    http: Arc<dyn Http>,
    api_key: String,
    username: String,
    password: String,
}

impl Client {
    pub fn new(http: Arc<dyn Http>, cfg: &Config) -> Self {
        Self {
            http,
            api_key: cfg.os_api_key.trim().to_string(),
            username: cfg.os_username.trim().to_string(),
            password: cfg.os_password.clone(),
        }
    }

    fn api_key(&self) -> Result<&str, Error> {
        if self.api_key.is_empty() {
            Err(Error::OsKeyMissing)
        } else {
            Ok(&self.api_key)
        }
    }

    /// Richiesta all'API con i codici d'errore di OpenSubtitles tradotti.
    fn call(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<Vec<u8>, Error> {
        self.http
            .send(method, url, headers, body)
            .map_err(|e| match e {
                Error::Http(401) => Error::OsLoginFailed,
                Error::Http(403) => Error::OsKeyInvalid,
                Error::Http(406) => Error::OsDailyLimit,
                other => other,
            })
    }

    fn user_agent() -> String {
        format!("Bobina v{}", env!("CARGO_PKG_VERSION"))
    }

    /// Cerca per hash (se noto) e per nome; i risultati con hash coincidente vengono per primi.
    pub fn search(
        &self,
        info: &MediaInfo,
        hash: Option<&str>,
        lang: &str,
    ) -> Result<Vec<SubCandidate>, Error> {
        let key = self.api_key()?;
        let ua = Self::user_agent();
        let headers = [
            ("Api-Key", key),
            ("User-Agent", ua.as_str()),
            ("Accept", "application/json"),
        ];
        let lang = encode(lang);

        let mut urls = Vec::new();
        if let Some(h) = hash {
            urls.push(format!("{BASE}/subtitles?languages={lang}&moviehash={h}"));
        }
        if let Some(n) = &info.n {
            // L'API vuole i parametri in ordine alfabetico e la ricerca in minuscolo.
            let mut params: Vec<(&str, String)> = vec![
                ("languages", lang.clone()),
                ("query", encode(&n.to_lowercase())),
            ];
            if info.is_episode() {
                params.push(("episode_number", info.e.unwrap_or(0).to_string()));
                params.push(("season_number", info.s.unwrap_or(0).to_string()));
            } else if let Some(y) = info.y {
                params.push(("year", y.to_string()));
            }
            params.sort_by_key(|p| p.0);
            let q: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            urls.push(format!("{BASE}/subtitles?{}", q.join("&")));
        }
        if urls.is_empty() {
            return Err(Error::NothingToSearch);
        }

        let mut all: Vec<SubCandidate> = Vec::new();
        let mut first_err = None;
        let mut any_ok = false;
        for url in &urls {
            match self
                .call(Method::Get, url, &headers, None)
                .and_then(|b| parse_search(&b))
            {
                Ok(found) => {
                    any_ok = true;
                    for c in found {
                        match all.iter_mut().find(|x| x.file_id == c.file_id) {
                            Some(x) => x.hash_match |= c.hash_match,
                            None => all.push(c),
                        }
                    }
                }
                Err(e) => first_err = first_err.or(Some(e)),
            }
        }
        if !any_ok {
            return Err(first_err.unwrap_or(Error::NoResults));
        }
        all.sort_by(|a, b| {
            b.hash_match
                .cmp(&a.hash_match)
                .then(b.downloads.cmp(&a.downloads))
        });
        Ok(all)
    }

    pub fn login(&self) -> Result<Session, Error> {
        let key = self.api_key()?;
        if self.username.is_empty() || self.password.is_empty() {
            return Err(Error::OsAccountMissing);
        }
        let ua = Self::user_agent();
        let headers = [
            ("Api-Key", key),
            ("User-Agent", ua.as_str()),
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
        ];
        let body =
            serde_json::json!({"username": self.username, "password": self.password}).to_string();
        let resp = self.call(
            Method::Post,
            &format!("{BASE}/login"),
            &headers,
            Some(&body),
        )?;
        parse_login(&resp)
    }

    /// Scarica un sottotitolo. Ritorna i byte così come sono, senza ricodificarli, e il nome del file.
    pub fn download(&self, session: &Session, file_id: u64) -> Result<(Vec<u8>, String), Error> {
        let key = self.api_key()?;
        let ua = Self::user_agent();
        let auth = format!("Bearer {}", session.token);
        let headers = [
            ("Api-Key", key),
            ("User-Agent", ua.as_str()),
            ("Authorization", auth.as_str()),
            ("Content-Type", "application/json"),
            ("Accept", "application/json"),
        ];
        let body = serde_json::json!({"file_id": file_id}).to_string();
        let resp = self.call(
            Method::Post,
            &format!("{}/download", session.base),
            &headers,
            Some(&body),
        )?;
        let (link, name) = parse_download(&resp)?;
        let bytes = self
            .http
            .send(Method::Get, &link, &[("User-Agent", ua.as_str())], None)?;
        Ok((bytes, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    type Call = (Method, String, Vec<(String, String)>, Option<String>);

    struct Fake {
        replies: HashMap<String, Result<&'static str, Error>>,
        calls: Mutex<Vec<Call>>,
    }
    impl Fake {
        fn new(pairs: Vec<(&str, Result<&'static str, Error>)>) -> Arc<Self> {
            Arc::new(Self {
                replies: pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                calls: Mutex::new(Vec::new()),
            })
        }
    }
    impl Http for Fake {
        fn send(
            &self,
            method: Method,
            url: &str,
            headers: &[(&str, &str)],
            body: Option<&str>,
        ) -> Result<Vec<u8>, Error> {
            self.calls.lock().unwrap().push((
                method,
                url.into(),
                headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body.map(String::from),
            ));
            match self.replies.get(url) {
                Some(Ok(b)) => Ok(b.as_bytes().to_vec()),
                Some(Err(e)) => Err(e.clone()),
                None => Err(Error::Http(404)),
            }
        }
    }

    fn cfg() -> Config {
        Config {
            os_api_key: "KEY".into(),
            os_username: "mario".into(),
            os_password: "pw".into(),
            ..Default::default()
        }
    }

    const SEARCH_HASH: &str = r#"{"data":[
        {"id":"1","attributes":{"language":"it","download_count":10,"release":"Show.S02E07.WEB","moviehash_match":true,
         "files":[{"file_id":111,"file_name":"show.s02e07.srt"}]}}]}"#;
    const SEARCH_NAME: &str = r#"{"data":[
        {"id":"1","attributes":{"language":"it","download_count":10,"release":"Show.S02E07.WEB",
         "files":[{"file_id":111,"file_name":"show.s02e07.srt"}]}},
        {"id":"2","attributes":{"language":"it","download_count":500,"release":"Show.S02E07.BluRay","hearing_impaired":true,
         "files":[{"file_id":222,"file_name":"b.srt"}]}},
        {"id":"3","attributes":{"files":[]}}]}"#;

    #[test]
    fn hash_di_riferimento() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("v.mkv");
        let data: Vec<u8> = (0..300000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&p, data).unwrap();
        assert_eq!(moviehash(&p).unwrap(), "eeaa601ad68fc085");
        std::fs::write(d.path().join("piccolo"), b"x").unwrap();
        assert!(moviehash(&d.path().join("piccolo")).is_err());
    }

    #[test]
    fn parser_ricerca_login_download() {
        let c = parse_search(SEARCH_NAME.as_bytes()).unwrap();
        assert_eq!(c.len(), 2, "senza file viene scartato");
        assert!(c[1].hearing_impaired && !c[1].hash_match);
        assert!(c[1].label().contains("500↓") && c[1].label().contains("HI"));
        assert!(parse_search(b"boh").is_err());

        let s = parse_login(br#"{"token":"T","base_url":"vip-api.opensubtitles.com"}"#).unwrap();
        assert_eq!(
            s,
            Session {
                token: "T".into(),
                base: "https://vip-api.opensubtitles.com/api/v1".into()
            }
        );
        assert_eq!(parse_login(br#"{"token":"T"}"#).unwrap().base, BASE);
        assert!(parse_login(br#"{}"#).is_err());

        let (l, n) = parse_download(br#"{"link":"https://dl/x","file_name":"a.srt"}"#).unwrap();
        assert_eq!((l.as_str(), n.as_str()), ("https://dl/x", "a.srt"));
    }

    #[test]
    fn ricerca_unisce_hash_e_nome_con_parametri_ordinati() {
        let hash_url = format!("{BASE}/subtitles?languages=it&moviehash=abcd");
        let name_url = format!(
            "{BASE}/subtitles?episode_number=7&languages=it&query=twin%20peaks&season_number=2"
        );
        let http = Fake::new(vec![
            (&hash_url, Ok(SEARCH_HASH)),
            (&name_url, Ok(SEARCH_NAME)),
        ]);
        let client = Client::new(http.clone(), &cfg());
        let info = MediaInfo {
            n: Some("Twin Peaks".into()),
            s: Some(2),
            e: Some(7),
            ..Default::default()
        };
        let r = client.search(&info, Some("abcd"), "it").unwrap();
        assert_eq!(
            r.iter().map(|c| c.file_id).collect::<Vec<_>>(),
            vec![111, 222]
        );
        assert!(
            r[0].hash_match,
            "il match per hash viene per primo anche con meno scaricamenti"
        );
        let calls = http.calls.lock().unwrap();
        assert!(calls[0].2.contains(&("Api-Key".into(), "KEY".into())));
        assert!(
            calls[0]
                .2
                .iter()
                .any(|(k, v)| k == "User-Agent" && v.starts_with("Bobina v"))
        );
    }

    #[test]
    fn film_usa_anno_e_un_errore_non_blocca_l_altra_ricerca() {
        let name_url = format!("{BASE}/subtitles?languages=en&query=blade%20runner&year=1982");
        let hash_url = format!("{BASE}/subtitles?languages=en&moviehash=ff");
        let http = Fake::new(vec![
            (&hash_url, Err(Error::Http(500))),
            (&name_url, Ok(SEARCH_NAME)),
        ]);
        let client = Client::new(http, &cfg());
        let info = MediaInfo {
            n: Some("Blade Runner".into()),
            y: Some(1982),
            ..Default::default()
        };
        assert_eq!(client.search(&info, Some("ff"), "en").unwrap().len(), 2);

        let solo_err = Fake::new(vec![
            (&hash_url, Err(Error::Http(500))),
            (&name_url, Err(Error::Http(500))),
        ]);
        let client = Client::new(solo_err, &cfg());
        assert_eq!(
            client.search(&info, Some("ff"), "en").unwrap_err(),
            Error::Http(500)
        );
    }

    #[test]
    fn senza_chiave_o_account_errori_chiari() {
        let http = Fake::new(vec![]);
        let client = Client::new(http.clone(), &Config::default());
        let info = MediaInfo {
            n: Some("x".into()),
            ..Default::default()
        };
        assert_eq!(
            client.search(&info, None, "it").unwrap_err(),
            Error::OsKeyMissing
        );
        let no_user = Config {
            os_api_key: "K".into(),
            ..Default::default()
        };
        assert_eq!(
            Client::new(http, &no_user).login().unwrap_err(),
            Error::OsAccountMissing
        );
        assert!(
            Client::new(Fake::new(vec![]), &cfg())
                .search(&MediaInfo::default(), None, "it")
                .is_err()
        );
    }

    #[test]
    fn codici_di_errore_di_opensubtitles() {
        let http = Fake::new(vec![
            (&format!("{BASE}/login"), Err(Error::Http(401))),
            (&format!("{BASE}/download"), Err(Error::Http(406))),
        ]);
        let client = Client::new(http, &cfg());
        assert_eq!(client.login().unwrap_err(), Error::OsLoginFailed);
        let s = Session {
            token: "T".into(),
            base: BASE.into(),
        };
        assert_eq!(client.download(&s, 1).unwrap_err(), Error::OsDailyLimit);
    }

    #[test]
    fn login_e_download_in_due_passi() {
        let http = Fake::new(vec![
            (
                &format!("{BASE}/login"),
                Ok(r#"{"token":"TOK","base_url":"api.opensubtitles.com"}"#),
            ),
            (
                &format!("{BASE}/download"),
                Ok(r#"{"link":"https://dl.test/f.srt","file_name":"Show.it.SRT"}"#),
            ),
            (
                "https://dl.test/f.srt",
                Ok("1\n00:00:01,000 --> 00:00:02,000\nCiao\n"),
            ),
        ]);
        let client = Client::new(http.clone(), &cfg());
        let session = client.login().unwrap();
        let (bytes, name) = client.download(&session, 222).unwrap();
        assert!(String::from_utf8(bytes).unwrap().contains("Ciao"));
        assert_eq!(name, "Show.it.SRT");
        let calls = http.calls.lock().unwrap();
        assert_eq!(
            calls[0].3.as_deref(),
            Some(r#"{"password":"pw","username":"mario"}"#)
        );
        assert!(
            calls[1]
                .2
                .contains(&("Authorization".into(), "Bearer TOK".into()))
        );
        assert_eq!(calls[1].3.as_deref(), Some(r#"{"file_id":222}"#));
        assert!(
            !calls[2].2.iter().any(|(k, _)| k == "Authorization"),
            "il link di download non riceve il token"
        );
    }

    #[test]
    fn percorso_del_sottotitolo() {
        let v = Path::new("/m/Show/Show - S02E07.mkv");
        assert_eq!(
            subtitle_path(v, "it", "x.SRT"),
            Path::new("/m/Show/Show - S02E07.it.srt")
        );
        assert_eq!(
            subtitle_path(v, "pt-BR", "x.ass"),
            Path::new("/m/Show/Show - S02E07.pt-BR.ass")
        );
        assert_eq!(
            subtitle_path(v, "it", "senzaext"),
            Path::new("/m/Show/Show - S02E07.it.srt")
        );
        assert_eq!(
            subtitle_path(v, "i/t", "x.sub"),
            Path::new("/m/Show/Show - S02E07.it.sub")
        );
    }
}
