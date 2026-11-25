//! Maschere a segnaposto per estrarre dati dal nome attuale del file.
//!
//! Esempio: `{n} - S{s}E{e} - *` su `Twin Peaks - S02E07 - Lonely Souls`.
//! Segnaposto: n, t (testo), y (anno a 4 cifre), s, e (numeri). `*` ignora qualsiasi testo.
//! La maschera confronta il nome senza estensione, ignorando le maiuscole.

use crate::template::MediaInfo;
use regex::Regex;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum MaskError {
    Unclosed,
    UnknownPlaceholder(String),
    Duplicate(String),
}

impl fmt::Display for MaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclosed => write!(f, "graffa non chiusa"),
            Self::UnknownPlaceholder(p) => write!(f, "segnaposto non valido nella maschera: {p}"),
            Self::Duplicate(p) => write!(f, "segnaposto ripetuto: {p}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mask {
    re: Regex,
}

impl Mask {
    pub fn compile(src: &str) -> Result<Self, MaskError> {
        let mut pat = String::from("(?i)^");
        let mut seen: Vec<String> = Vec::new();
        let mut chars = src.chars();
        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    let mut name = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(ch) => name.push(ch),
                            None => return Err(MaskError::Unclosed),
                        }
                    }
                    let name = name.trim().to_string();
                    let group = match name.as_str() {
                        "n" | "t" => ".+?",
                        "y" => r"\d{4}",
                        "s" | "e" => r"\d{1,3}",
                        _ => return Err(MaskError::UnknownPlaceholder(name)),
                    };
                    if seen.contains(&name) {
                        return Err(MaskError::Duplicate(name));
                    }
                    pat.push_str(&format!("(?P<{name}>{group})"));
                    seen.push(name);
                }
                '*' => pat.push_str(".*"),
                other => pat.push_str(&regex::escape(&other.to_string())),
            }
        }
        pat.push('$');
        // Il pattern è costruito qui con soli blocchi validi, quindi non può fallire.
        let re = Regex::new(&pat).expect("pattern della maschera valido");
        Ok(Self { re })
    }

    /// Estrae i campi dal nome (senza estensione). `None` se la maschera non corrisponde.
    pub fn extract(&self, stem: &str) -> Option<MediaInfo> {
        let caps = self.re.captures(stem)?;
        let text = |k: &str| caps.name(k).map(|m| m.as_str().trim().to_string());
        let num = |k: &str| caps.name(k).and_then(|m| m.as_str().parse::<u32>().ok());
        Some(MediaInfo {
            n: text("n"),
            t: text("t"),
            y: num("y"),
            s: num("s"),
            e: num("e"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estrae_serie_episodio_titolo() {
        let m = Mask::compile("{n} - S{s}E{e} - {t}").unwrap();
        let i = m.extract("Twin Peaks - S02E07 - Lonely Souls").unwrap();
        assert_eq!(i.n.as_deref(), Some("Twin Peaks"));
        assert_eq!((i.s, i.e), (Some(2), Some(7)));
        assert_eq!(i.t.as_deref(), Some("Lonely Souls"));
    }

    #[test]
    fn asterisco_ignora_il_resto() {
        let m = Mask::compile("{n}.{y}.*").unwrap();
        let i = m.extract("Blade.Runner.1982.1080p.BluRay.x264").unwrap();
        assert_eq!(i.n.as_deref(), Some("Blade.Runner"));
        assert_eq!(i.y, Some(1982));
    }

    #[test]
    fn nessuna_corrispondenza() {
        let m = Mask::compile("{n} - S{s}E{e}").unwrap();
        assert!(m.extract("file a caso").is_none());
    }

    #[test]
    fn maiuscole_ignorate_e_punti_letterali() {
        let m = Mask::compile("{n}.s{s}e{e}").unwrap();
        assert!(m.extract("show.S01E02").is_some());
        assert!(m.extract("showXS01E02").is_none());
    }

    #[test]
    fn errori() {
        assert_eq!(Mask::compile("{n").unwrap_err(), MaskError::Unclosed);
        assert_eq!(
            Mask::compile("{q}").unwrap_err(),
            MaskError::UnknownPlaceholder("q".into())
        );
        assert_eq!(
            Mask::compile("{n}{n}").unwrap_err(),
            MaskError::Duplicate("n".into())
        );
    }
}
