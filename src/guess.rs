//! Riconoscimento automatico di titolo, anno, stagione ed episodio dal nome del file.

use crate::template::MediaInfo;
use regex::Regex;
use std::sync::OnceLock;

fn re_sxe() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\bS(\d{1,2})[ ._-]?E(\d{1,3})\b").unwrap())
}
fn re_nxn() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?i)\b(\d{1,2})x(\d{2,3})\b").unwrap())
}
fn re_year() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"\b(19\d{2}|20\d{2})\b").unwrap())
}

fn re_tag() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)\b(480p|576p|720p|1080p|2160p|4k|uhd|bluray|bdrip|brrip|dvdrip|web-?dl|webrip|hdtv|x264|x265|h264|h265|hevc|remux)\b",
        )
        .unwrap()
    })
}

/// Ricava i dati dal nome del file senza estensione.
pub fn guess(stem: &str) -> MediaInfo {
    // Il trattino basso è un carattere di parola per \b: lo si sostituisce (stessa lunghezza in byte).
    let norm = stem.replace('_', " ");
    let stem = norm.as_str();
    let mut info = MediaInfo::default();
    let mut cut = stem.len();

    let ep = re_sxe().captures(stem).or_else(|| re_nxn().captures(stem));
    if let Some(c) = &ep {
        info.s = c[1].parse().ok();
        info.e = c[2].parse().ok();
        cut = cut.min(c.get(0).unwrap().start());
    }

    // L'anno in testa al nome fa parte del titolo ("2001 Odissea nello spazio"): si cerca oltre.
    if let Some(m) = re_year().find_iter(stem).filter(|m| m.start() > 0).last() {
        info.y = m.as_str().parse().ok();
        cut = cut.min(m.start());
    }

    if let Some(m) = re_tag().find(stem).filter(|m| m.start() > 0) {
        cut = cut.min(m.start());
    }

    let title = clean_title(&stem[..cut]);
    if !title.is_empty() {
        info.n = Some(title);
    }
    info
}

fn clean_title(raw: &str) -> String {
    let spaced: String = raw
        .chars()
        .map(|c| if c == '.' || c == '_' { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '-' || c == '(' || c == '[' || c.is_whitespace())
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serie_con_sxxexx() {
        let i = guess("Twin.Peaks.S02E07.1080p.WEB-DL");
        assert_eq!(i.n.as_deref(), Some("Twin Peaks"));
        assert_eq!((i.s, i.e), (Some(2), Some(7)));
    }

    #[test]
    fn serie_con_nxnn() {
        let i = guess("the_wire_3x05_hamsterdam");
        assert_eq!(i.n.as_deref(), Some("the wire"));
        assert_eq!((i.s, i.e), (Some(3), Some(5)));
    }

    #[test]
    fn film_con_anno() {
        let i = guess("Blade.Runner.1982.Final.Cut.1080p");
        assert_eq!(i.n.as_deref(), Some("Blade Runner"));
        assert_eq!(i.y, Some(1982));
        assert!(!i.is_episode());
    }

    #[test]
    fn anno_in_testa_resta_nel_titolo() {
        let i = guess("2001.A.Space.Odyssey.1968");
        assert_eq!(i.n.as_deref(), Some("2001 A Space Odyssey"));
        assert_eq!(i.y, Some(1968));
    }

    #[test]
    fn serie_con_anno_e_episodio() {
        let i = guess("Twin Peaks (1990) S01E01");
        assert_eq!(i.n.as_deref(), Some("Twin Peaks"));
        assert_eq!(i.y, Some(1990));
        assert_eq!((i.s, i.e), (Some(1), Some(1)));
    }

    #[test]
    fn risoluzione_non_e_anno() {
        let i = guess("Solaris.1080p");
        assert_eq!(i.y, None);
        assert_eq!(i.n.as_deref(), Some("Solaris"));
    }
}
