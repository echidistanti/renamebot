//! Accesso alla rete. Un solo trait per tutti i servizi; la cache su disco avvolge le GET semplici.

use crate::error::Error;
use crate::fsx::write_atomic;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Trasporto HTTP; nei test si sostituisce con uno finto.
pub trait Http: Send + Sync {
    fn send(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<Vec<u8>, Error>;

    /// GET senza intestazioni, con il corpo come testo.
    fn get(&self, url: &str) -> Result<String, Error> {
        let bytes = self.send(Method::Get, url, &[], None)?;
        String::from_utf8(bytes).map_err(|e| Error::BadResponse(e.to_string()))
    }
}

/// Client HTTP reale.
pub struct Ureq {
    agent: ureq::Agent,
}

impl Default for Ureq {
    fn default() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("RenameBot/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }
}

impl Http for Ureq {
    fn send(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<Vec<u8>, Error> {
        let result = match method {
            Method::Get => {
                let mut r = self.agent.get(url);
                for (k, v) in headers {
                    r = r.header(*k, *v);
                }
                r.call()
            }
            Method::Post => {
                let mut r = self.agent.post(url);
                for (k, v) in headers {
                    r = r.header(*k, *v);
                }
                r.send(body.unwrap_or(""))
            }
        };
        let mut resp = result.map_err(|e| match e {
            ureq::Error::StatusCode(code) => Error::Http(code),
            other => Error::Network(other.to_string()),
        })?;
        resp.body_mut()
            .read_to_vec()
            .map_err(|e| Error::Network(e.to_string()))
    }
}

/// Cache su disco per le GET senza intestazioni. La chiave della cache non contiene `api_key`.
pub struct Cached<H: Http> {
    inner: H,
    dir: PathBuf,
    ttl: Duration,
}

impl<H: Http> Cached<H> {
    pub fn new(inner: H, dir: PathBuf, ttl: Duration) -> Self {
        Self { inner, dir, ttl }
    }

    /// Cartella di cache predefinita dell'utente.
    pub fn default_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("RenameBot").join("http"))
    }

    fn path_for(&self, url: &str) -> PathBuf {
        self.dir
            .join(format!("{:016x}.json", fnv1a(&cache_key(url))))
    }
}

impl<H: Http> Http for Cached<H> {
    fn send(
        &self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<Vec<u8>, Error> {
        if method != Method::Get || !headers.is_empty() {
            return self.inner.send(method, url, headers, body);
        }
        let path = self.path_for(url);
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < self.ttl);
        if fresh && let Ok(bytes) = fs::read(&path) {
            return Ok(bytes);
        }
        let bytes = self.inner.send(method, url, headers, body)?;
        // Un errore di scrittura nella cache non deve far fallire la richiesta.
        let _ = write_atomic(&path, &bytes, false);
        Ok(bytes)
    }
}

/// Toglie il parametro `api_key` dall'URL, così la chiave non finisce nei nomi né cambia la cache.
pub fn cache_key(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| !p.starts_with("api_key="))
        .collect();
    if kept.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", kept.join("&"))
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Codifica percentuale per i valori nei parametri di query.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake(AtomicUsize);
    impl Http for Fake {
        fn send(
            &self,
            _m: Method,
            url: &str,
            _h: &[(&str, &str)],
            _b: Option<&str>,
        ) -> Result<Vec<u8>, Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(format!("risposta per {url}").into_bytes())
        }
    }

    fn cached(ttl: Duration, dir: &std::path::Path) -> Cached<Fake> {
        Cached::new(Fake(AtomicUsize::new(0)), dir.into(), ttl)
    }

    #[test]
    fn la_cache_evita_la_seconda_richiesta() {
        let d = tempfile::tempdir().unwrap();
        let c = cached(Duration::from_secs(60), d.path());
        let a = c.get("https://x.test/search?api_key=SEGRETO&q=a").unwrap();
        let b = c.get("https://x.test/search?api_key=ALTRA&q=a").unwrap();
        assert_eq!(a, b, "la chiave API non entra nella cache");
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 1);
        c.get("https://x.test/search?q=b").unwrap();
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cache_scaduta_o_richieste_con_intestazioni_vanno_in_rete() {
        let d = tempfile::tempdir().unwrap();
        let c = cached(Duration::ZERO, d.path());
        c.get("https://x.test/a").unwrap();
        c.get("https://x.test/a").unwrap();
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 2);

        let c = cached(Duration::from_secs(60), d.path());
        for _ in 0..2 {
            c.send(Method::Get, "https://x.test/h", &[("Api-Key", "k")], None)
                .unwrap();
            c.send(Method::Post, "https://x.test/p", &[], Some("{}"))
                .unwrap();
        }
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn chiave_e_codifica() {
        assert_eq!(cache_key("https://x/a?api_key=k&q=1"), "https://x/a?q=1");
        assert_eq!(cache_key("https://x/a?api_key=k"), "https://x/a");
        assert_eq!(cache_key("https://x/a"), "https://x/a");
        assert_eq!(
            encode("Twin Peaks & Co/è"),
            "Twin%20Peaks%20%26%20Co%2F%C3%A8"
        );
    }
}
