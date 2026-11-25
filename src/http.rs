//! Accesso alla rete con cache su disco. Il trait `Fetch` permette di sostituire la rete nei test.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub trait Fetch: Send + Sync {
    fn get(&self, url: &str) -> Result<String, String>;
}

/// Client HTTP reale.
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .user_agent(concat!("Bobina/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent }
    }
}

impl Fetch for Http {
    fn get(&self, url: &str) -> Result<String, String> {
        let mut resp = self.agent.get(url).call().map_err(|e| match e {
            ureq::Error::StatusCode(code) => format!("HTTP {code}"),
            other => other.to_string(),
        })?;
        resp.body_mut().read_to_string().map_err(|e| e.to_string())
    }
}

/// Aggiunge una cache su disco a un altro `Fetch`. La chiave della cache non contiene `api_key`.
pub struct Cached<F: Fetch> {
    inner: F,
    dir: PathBuf,
    ttl: Duration,
}

impl<F: Fetch> Cached<F> {
    pub fn new(inner: F, dir: PathBuf, ttl: Duration) -> Self {
        Self { inner, dir, ttl }
    }

    /// Cartella di cache predefinita dell'utente.
    pub fn default_dir() -> Option<PathBuf> {
        dirs::cache_dir().map(|d| d.join("Bobina").join("http"))
    }

    fn path_for(&self, url: &str) -> PathBuf {
        self.dir
            .join(format!("{:016x}.json", fnv1a(&cache_key(url))))
    }
}

impl<F: Fetch> Fetch for Cached<F> {
    fn get(&self, url: &str) -> Result<String, String> {
        let path = self.path_for(url);
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|age| age < self.ttl);
        if fresh && let Ok(body) = fs::read_to_string(&path) {
            return Ok(body);
        }
        let body = self.inner.get(url)?;
        // Un errore di scrittura nella cache non deve far fallire la richiesta.
        let _ = fs::create_dir_all(&self.dir).and_then(|_| fs::write(&path, &body));
        Ok(body)
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
    impl Fetch for Fake {
        fn get(&self, url: &str) -> Result<String, String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(format!("risposta per {url}"))
        }
    }

    #[test]
    fn la_cache_evita_la_seconda_richiesta() {
        let d = tempfile::tempdir().unwrap();
        let c = Cached::new(
            Fake(AtomicUsize::new(0)),
            d.path().into(),
            Duration::from_secs(60),
        );
        let a = c.get("https://x.test/search?api_key=SEGRETO&q=a").unwrap();
        let b = c.get("https://x.test/search?api_key=ALTRA&q=a").unwrap();
        assert_eq!(a, b, "la chiave API non entra nella cache");
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 1);
        c.get("https://x.test/search?q=b").unwrap();
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn cache_scaduta_richiede_di_nuovo() {
        let d = tempfile::tempdir().unwrap();
        let c = Cached::new(Fake(AtomicUsize::new(0)), d.path().into(), Duration::ZERO);
        c.get("https://x.test/a").unwrap();
        c.get("https://x.test/a").unwrap();
        assert_eq!(c.inner.0.load(Ordering::SeqCst), 2);
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
