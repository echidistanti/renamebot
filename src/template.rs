//! Linguaggio di template per i nomi di uscita: `{n} - {s00e00}{t|prefix: - }`.
//!
//! Segnaposto: n, y, s, e, t, s00, e00, s00e00, sxe.
//! Filtri (separati da `|`): upper, lower, title, default:X, prefix:X, suffix:X.
//! Prefix e suffix si applicano solo se il valore esiste e rendono il campo opzionale.
//! `{{` e `}}` producono le graffe letterali. Lo `/` nel template separa le cartelle.

use std::fmt;

/// Dati noti su un file multimediale.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub n: Option<String>,
    pub y: Option<u32>,
    pub s: Option<u32>,
    pub e: Option<u32>,
    pub t: Option<String>,
}

impl MediaInfo {
    pub fn is_episode(&self) -> bool {
        self.s.is_some() && self.e.is_some()
    }

    fn value(&self, name: &str) -> Option<String> {
        let text = |o: &Option<String>| o.clone().filter(|v| !v.trim().is_empty());
        match name {
            "n" => text(&self.n),
            "t" => text(&self.t),
            "y" => self.y.map(|v| v.to_string()),
            "s" => self.s.map(|v| v.to_string()),
            "e" => self.e.map(|v| v.to_string()),
            "s00" => self.s.map(|v| format!("{v:02}")),
            "e00" => self.e.map(|v| format!("{v:02}")),
            "s00e00" => Some(format!("S{:02}E{:02}", self.s?, self.e?)),
            "sxe" => Some(format!("{}x{:02}", self.s?, self.e?)),
            _ => None,
        }
    }
}

pub const PLACEHOLDERS: &[&str] = &["n", "y", "s", "e", "t", "s00", "e00", "s00e00", "sxe"];

#[derive(Debug, Clone, PartialEq)]
pub enum TemplateError {
    Unclosed,
    UnknownPlaceholder(String),
    UnknownFilter(String),
    EmptyExpression,
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unclosed => write!(f, "graffa non chiusa"),
            Self::UnknownPlaceholder(p) => write!(f, "segnaposto sconosciuto: {p}"),
            Self::UnknownFilter(p) => write!(f, "filtro sconosciuto: {p}"),
            Self::EmptyExpression => write!(f, "espressione vuota"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Filter {
    Upper,
    Lower,
    Title,
    Default(String),
    Prefix(String),
    Suffix(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Lit(String),
    Field { name: String, filters: Vec<Filter> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    tokens: Vec<Token>,
}

/// Risultato del rendering: percorso relativo e campi obbligatori mancanti.
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered {
    pub path: String,
    pub missing: Vec<String>,
}

impl Template {
    pub fn parse(src: &str) -> Result<Self, TemplateError> {
        let mut tokens = Vec::new();
        let mut lit = String::new();
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    lit.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    lit.push('}');
                }
                '{' => {
                    let mut expr = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some(ch) => expr.push(ch),
                            None => return Err(TemplateError::Unclosed),
                        }
                    }
                    if !lit.is_empty() {
                        tokens.push(Token::Lit(std::mem::take(&mut lit)));
                    }
                    tokens.push(parse_field(&expr)?);
                }
                other => lit.push(other),
            }
        }
        if !lit.is_empty() {
            tokens.push(Token::Lit(lit));
        }
        Ok(Self { tokens })
    }

    pub fn render(&self, info: &MediaInfo) -> Rendered {
        let mut out = String::new();
        let mut missing = Vec::new();
        for token in &self.tokens {
            match token {
                Token::Lit(s) => out.push_str(s),
                Token::Field { name, filters } => {
                    let optional = filters.iter().any(|f| {
                        matches!(
                            f,
                            Filter::Default(_) | Filter::Prefix(_) | Filter::Suffix(_)
                        )
                    });
                    let mut value = info.value(name).map(|v| sanitize(&v));
                    for f in filters {
                        value = apply(f, value);
                    }
                    match value {
                        Some(v) => out.push_str(&v),
                        None if optional => {}
                        None => {
                            if !missing.contains(name) {
                                missing.push(name.clone());
                            }
                        }
                    }
                }
            }
        }
        Rendered {
            path: finalize_path(&out),
            missing,
        }
    }
}

fn parse_field(expr: &str) -> Result<Token, TemplateError> {
    let mut parts = expr.split('|');
    let name = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return Err(TemplateError::EmptyExpression);
    }
    if !PLACEHOLDERS.contains(&name) {
        return Err(TemplateError::UnknownPlaceholder(name.to_string()));
    }
    let mut filters = Vec::new();
    for raw in parts {
        let (fname, arg) = match raw.split_once(':') {
            Some((a, b)) => (a.trim(), Some(b.to_string())),
            None => (raw.trim(), None),
        };
        let filter = match (fname, arg) {
            ("upper", None) => Filter::Upper,
            ("lower", None) => Filter::Lower,
            ("title", None) => Filter::Title,
            ("default", Some(a)) => Filter::Default(a),
            ("prefix", Some(a)) => Filter::Prefix(a),
            ("suffix", Some(a)) => Filter::Suffix(a),
            _ => return Err(TemplateError::UnknownFilter(raw.trim().to_string())),
        };
        filters.push(filter);
    }
    Ok(Token::Field {
        name: name.to_string(),
        filters,
    })
}

fn apply(filter: &Filter, value: Option<String>) -> Option<String> {
    match filter {
        Filter::Default(d) => value.or_else(|| Some(d.clone())),
        Filter::Prefix(p) => value.map(|v| format!("{p}{v}")),
        Filter::Suffix(s) => value.map(|v| format!("{v}{s}")),
        Filter::Upper => value.map(|v| v.to_uppercase()),
        Filter::Lower => value.map(|v| v.to_lowercase()),
        Filter::Title => value.map(|v| {
            v.split_whitespace()
                .map(|w| {
                    let mut cs = w.chars();
                    match cs.next() {
                        Some(f) => {
                            f.to_uppercase().collect::<String>() + &cs.as_str().to_lowercase()
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }),
    }
}

/// Toglie dai valori i caratteri non ammessi nei nomi di file.
pub fn sanitize(value: &str) -> String {
    let mut s = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '/' | '\\' => s.push(' '),
            ':' => s.push_str(" -"),
            '*' | '?' | '"' | '<' | '>' | '|' => {}
            c if c.is_control() => {}
            c => s.push(c),
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pulisce ogni componente del percorso: niente spazi o punti finali, niente segmenti vuoti.
fn finalize_path(raw: &str) -> String {
    raw.split('/')
        .map(|seg| seg.trim().trim_end_matches('.').trim())
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep() -> MediaInfo {
        MediaInfo {
            n: Some("Twin Peaks".into()),
            y: Some(1990),
            s: Some(2),
            e: Some(7),
            t: Some("Lonely Souls".into()),
        }
    }

    fn render(t: &str, i: &MediaInfo) -> Rendered {
        Template::parse(t).unwrap().render(i)
    }

    #[test]
    fn rende_formato_base() {
        let r = render("{n}/Season {s}/{n} - {s00e00} - {t}", &ep());
        assert_eq!(
            r.path,
            "Twin Peaks/Season 2/Twin Peaks - S02E07 - Lonely Souls"
        );
        assert!(r.missing.is_empty());
    }

    #[test]
    fn sxe_e_padding() {
        assert_eq!(render("{sxe}", &ep()).path, "2x07");
        assert_eq!(render("{s00}{e00}", &ep()).path, "0207");
    }

    #[test]
    fn campo_mancante_obbligatorio() {
        let mut i = ep();
        i.t = None;
        let r = render("{n} - {t}", &i);
        assert_eq!(r.missing, vec!["t"]);
    }

    #[test]
    fn prefix_rende_opzionale() {
        let mut i = ep();
        i.t = None;
        let r = render("{n} - {s00e00}{t|prefix: - }", &i);
        assert_eq!(r.path, "Twin Peaks - S02E07");
        assert!(r.missing.is_empty());
    }

    #[test]
    fn anno_tra_parentesi_solo_se_presente() {
        let t = "{n}{y|prefix: (|suffix:)}";
        assert_eq!(render(t, &ep()).path, "Twin Peaks (1990)");
        let mut i = ep();
        i.y = None;
        assert_eq!(render(t, &i).path, "Twin Peaks");
    }

    #[test]
    fn filtri_in_catena() {
        assert_eq!(render("{n|upper}", &ep()).path, "TWIN PEAKS");
        assert_eq!(render("{n|lower}", &ep()).path, "twin peaks");
        let i = MediaInfo {
            n: Some("the big LEBOWSKI".into()),
            ..Default::default()
        };
        assert_eq!(render("{n|title}", &i).path, "The Big Lebowski");
        assert_eq!(
            render("{t|default:Senza titolo}", &MediaInfo::default()).path,
            "Senza titolo"
        );
    }

    #[test]
    fn graffe_letterali() {
        assert_eq!(render("{{{n}}}", &ep()).path, "{Twin Peaks}");
    }

    #[test]
    fn sanitizza_valori_non_i_separatori() {
        let i = MediaInfo {
            n: Some("Fate/Zero: Part?".into()),
            ..Default::default()
        };
        assert_eq!(render("Anime/{n}", &i).path, "Anime/Fate Zero - Part");
    }

    #[test]
    fn errori_di_sintassi() {
        assert_eq!(Template::parse("{n").unwrap_err(), TemplateError::Unclosed);
        assert_eq!(
            Template::parse("{zz}").unwrap_err(),
            TemplateError::UnknownPlaceholder("zz".into())
        );
        assert_eq!(
            Template::parse("{n|boh}").unwrap_err(),
            TemplateError::UnknownFilter("boh".into())
        );
        assert_eq!(
            Template::parse("{}").unwrap_err(),
            TemplateError::EmptyExpression
        );
    }

    #[test]
    fn segmenti_vuoti_e_punti_finali() {
        let i = MediaInfo {
            n: Some("Show.".into()),
            ..Default::default()
        };
        assert_eq!(render("/{n}//x", &i).path, "Show/x");
    }
}
