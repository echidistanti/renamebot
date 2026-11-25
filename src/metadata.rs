//! Ricerca di serie, film ed episodi su TheMovieDB e TVmaze.

use crate::config::{Config, Provider};
use crate::http::{Fetch, encode};
use crate::template::MediaInfo;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub provider: Provider,
    pub id: u64,
    pub name: String,
    pub year: Option<u32>,
}

impl Candidate {
    pub fn label(&self) -> String {
        match self.year {
            Some(y) => format!("{} ({y}) · {}", self.name, self.provider.name()),
            None => format!("{} · {}", self.name, self.provider.name()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Episode {
    pub season: u32,
    pub number: u32,
    pub title: String,
}

fn year_of(date: Option<&str>) -> Option<u32> {
    date?.get(..4)?.parse().ok()
}

fn tmdb_key(cfg: &Config) -> Result<&str, String> {
    let k = cfg.tmdb_key.trim();
    if k.is_empty() {
        Err("chiave TMDB mancante: inseriscila nelle impostazioni".into())
    } else {
        Ok(k)
    }
}

fn json(body: &str) -> Result<Value, String> {
    serde_json::from_str(body).map_err(|e| format!("risposta non valida: {e}"))
}

pub fn parse_tmdb_search(body: &str, tv: bool) -> Result<Vec<Candidate>, String> {
    let v = json(body)?;
    let (name_k, date_k) = if tv {
        ("name", "first_air_date")
    } else {
        ("title", "release_date")
    };
    Ok(v["results"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            Some(Candidate {
                provider: Provider::Tmdb,
                id: r["id"].as_u64()?,
                name: r[name_k].as_str()?.to_string(),
                year: year_of(r[date_k].as_str()),
            })
        })
        .collect())
}

pub fn parse_tmdb_season(body: &str) -> Result<Vec<Episode>, String> {
    let v = json(body)?;
    Ok(v["episodes"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| {
            Some(Episode {
                season: u32::try_from(e["season_number"].as_u64()?).ok()?,
                number: u32::try_from(e["episode_number"].as_u64()?).ok()?,
                title: e["name"].as_str()?.to_string(),
            })
        })
        .collect())
}

pub fn parse_tvmaze_search(body: &str) -> Result<Vec<Candidate>, String> {
    let v = json(body)?;
    Ok(v.as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            let s = &r["show"];
            Some(Candidate {
                provider: Provider::Tvmaze,
                id: s["id"].as_u64()?,
                name: s["name"].as_str()?.to_string(),
                year: year_of(s["premiered"].as_str()),
            })
        })
        .collect())
}

/// Gli speciali di TVmaze non hanno numero di episodio e vengono saltati.
pub fn parse_tvmaze_episodes(body: &str) -> Result<Vec<Episode>, String> {
    let v = json(body)?;
    Ok(v.as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| {
            Some(Episode {
                season: u32::try_from(e["season"].as_u64()?).ok()?,
                number: u32::try_from(e["number"].as_u64()?).ok()?,
                title: e["name"].as_str()?.to_string(),
            })
        })
        .collect())
}

/// Cerca serie con il provider scelto.
pub fn search_series(f: &dyn Fetch, cfg: &Config, query: &str) -> Result<Vec<Candidate>, String> {
    match cfg.provider {
        Provider::Tmdb => {
            let url = format!(
                "https://api.themoviedb.org/3/search/tv?api_key={}&language={}&query={}",
                tmdb_key(cfg)?,
                cfg.tmdb_lang(),
                encode(query)
            );
            parse_tmdb_search(&f.get(&url)?, true)
        }
        Provider::Tvmaze => {
            let url = format!("https://api.tvmaze.com/search/shows?q={}", encode(query));
            parse_tvmaze_search(&f.get(&url)?)
        }
    }
}

/// I film si cercano solo su TMDB, anche se il provider scelto è TVmaze.
pub fn search_movies(f: &dyn Fetch, cfg: &Config, query: &str) -> Result<Vec<Candidate>, String> {
    let url = format!(
        "https://api.themoviedb.org/3/search/movie?api_key={}&language={}&query={}",
        tmdb_key(cfg)?,
        cfg.tmdb_lang(),
        encode(query)
    );
    parse_tmdb_search(&f.get(&url)?, false)
}

/// Episodi delle stagioni richieste (TVmaze li restituisce tutti insieme e si filtra dopo).
pub fn episodes(
    f: &dyn Fetch,
    cfg: &Config,
    cand: &Candidate,
    seasons: &[u32],
) -> Result<Vec<Episode>, String> {
    match cand.provider {
        Provider::Tmdb => {
            let mut all = Vec::new();
            for s in seasons {
                let url = format!(
                    "https://api.themoviedb.org/3/tv/{}/season/{s}?api_key={}&language={}",
                    cand.id,
                    tmdb_key(cfg)?,
                    cfg.tmdb_lang()
                );
                all.extend(parse_tmdb_season(&f.get(&url)?)?);
            }
            Ok(all)
        }
        Provider::Tvmaze => {
            let url = format!("https://api.tvmaze.com/shows/{}/episodes", cand.id);
            let mut all = parse_tvmaze_episodes(&f.get(&url)?)?;
            all.retain(|e| seasons.contains(&e.season));
            Ok(all)
        }
    }
}

/// Sceglie il candidato: a parità di nome conta l'anno ricavato dal file, altrimenti il primo.
pub fn pick_best(cands: &[Candidate], year: Option<u32>) -> usize {
    year.and_then(|y| cands.iter().position(|c| c.year == Some(y)))
        .unwrap_or(0)
}

/// Completa i dati del file con quelli ufficiali del candidato e dell'episodio.
pub fn enrich(info: &MediaInfo, cand: &Candidate, eps: &[Episode]) -> MediaInfo {
    let mut out = info.clone();
    out.n = Some(cand.name.clone());
    out.y = cand.year.or(info.y);
    if let (Some(s), Some(e)) = (info.s, info.e)
        && let Some(ep) = eps.iter().find(|x| x.season == s && x.number == e)
    {
        out.t = Some(ep.title.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    const TMDB_TV: &str = r#"{"page":1,"results":[
        {"id":1920,"name":"I segreti di Twin Peaks","first_air_date":"1990-04-08"},
        {"id":2,"name":"Senza data","first_air_date":""},
        {"name":"Senza id"}],"total_results":3}"#;
    const TMDB_MOVIE: &str =
        r#"{"results":[{"id":78,"title":"Blade Runner","release_date":"1982-06-25"}]}"#;
    const TMDB_SEASON: &str = r#"{"episodes":[
        {"episode_number":1,"season_number":2,"name":"Il giorno dopo"},
        {"episode_number":2,"season_number":2,"name":"Il sogno"}]}"#;
    const MAZE_SEARCH: &str = r#"[{"score":0.9,"show":{"id":123,"name":"Twin Peaks","premiered":"1990-04-08"}},
        {"score":0.5,"show":{"id":124,"name":"Twin Peaks 2017","premiered":null}}]"#;
    const MAZE_EPS: &str = r#"[
        {"season":1,"number":1,"name":"Pilot"},
        {"season":1,"number":null,"name":"Speciale"},
        {"season":2,"number":7,"name":"Lonely Souls"}]"#;

    struct Canned(Mutex<HashMap<String, &'static str>>, Mutex<Vec<String>>);
    impl Canned {
        fn new(pairs: &[(&str, &'static str)]) -> Self {
            Self(
                Mutex::new(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()),
                Mutex::new(Vec::new()),
            )
        }
    }
    impl Fetch for Canned {
        fn get(&self, url: &str) -> Result<String, String> {
            self.1.lock().unwrap().push(url.to_string());
            self.0
                .lock()
                .unwrap()
                .get(url)
                .map(|s| s.to_string())
                .ok_or_else(|| format!("HTTP 404 per {url}"))
        }
    }

    #[test]
    fn parser_tmdb() {
        let c = parse_tmdb_search(TMDB_TV, true).unwrap();
        assert_eq!(c.len(), 2, "senza id viene scartato");
        assert_eq!((c[0].id, c[0].year), (1920, Some(1990)));
        assert_eq!(c[1].year, None);
        let m = parse_tmdb_search(TMDB_MOVIE, false).unwrap();
        assert_eq!(
            (m[0].name.as_str(), m[0].year),
            ("Blade Runner", Some(1982))
        );
        let e = parse_tmdb_season(TMDB_SEASON).unwrap();
        assert_eq!(
            e[1],
            Episode {
                season: 2,
                number: 2,
                title: "Il sogno".into()
            }
        );
        assert!(parse_tmdb_search("non json", true).is_err());
    }

    #[test]
    fn parser_tvmaze() {
        let c = parse_tvmaze_search(MAZE_SEARCH).unwrap();
        assert_eq!((c[0].id, c[0].year, c[1].year), (123, Some(1990), None));
        let e = parse_tvmaze_episodes(MAZE_EPS).unwrap();
        assert_eq!(e.len(), 2, "lo speciale senza numero è saltato");
    }

    #[test]
    fn ricerca_tmdb_usa_chiave_lingua_e_codifica() {
        let cfg = Config {
            tmdb_key: "K".into(),
            ..Default::default()
        };
        let url =
            "https://api.themoviedb.org/3/search/tv?api_key=K&language=it-IT&query=Twin%20Peaks";
        let f = Canned::new(&[(url, TMDB_TV)]);
        assert_eq!(search_series(&f, &cfg, "Twin Peaks").unwrap().len(), 2);
    }

    #[test]
    fn senza_chiave_tmdb_errore_chiaro_ma_tvmaze_funziona() {
        let f = Canned::new(&[(
            "https://api.tvmaze.com/search/shows?q=Twin%20Peaks",
            MAZE_SEARCH,
        )]);
        let mut cfg = Config::default();
        assert!(
            search_series(&f, &cfg, "Twin Peaks")
                .unwrap_err()
                .contains("chiave")
        );
        assert!(search_movies(&f, &cfg, "x").is_err());
        cfg.provider = Provider::Tvmaze;
        assert_eq!(search_series(&f, &cfg, "Twin Peaks").unwrap().len(), 2);
    }

    #[test]
    fn episodi_tmdb_per_stagione_e_tvmaze_filtrati() {
        let cfg = Config {
            tmdb_key: "K".into(),
            ..Default::default()
        };
        let tm = Candidate {
            provider: Provider::Tmdb,
            id: 1920,
            name: "x".into(),
            year: None,
        };
        let f = Canned::new(&[(
            "https://api.themoviedb.org/3/tv/1920/season/2?api_key=K&language=it-IT",
            TMDB_SEASON,
        )]);
        assert_eq!(episodes(&f, &cfg, &tm, &[2]).unwrap().len(), 2);

        let mz = Candidate {
            provider: Provider::Tvmaze,
            id: 123,
            name: "x".into(),
            year: None,
        };
        let f = Canned::new(&[("https://api.tvmaze.com/shows/123/episodes", MAZE_EPS)]);
        let e = episodes(&f, &cfg, &mz, &[2]).unwrap();
        assert_eq!(
            e,
            vec![Episode {
                season: 2,
                number: 7,
                title: "Lonely Souls".into()
            }]
        );
    }

    #[test]
    fn scelta_per_anno_e_arricchimento() {
        let c = parse_tmdb_search(
            r#"{"results":[{"id":1,"name":"Dune","first_air_date":"1984-01-01"},
                           {"id":2,"name":"Dune","first_air_date":"2021-01-01"}]}"#,
            true,
        )
        .unwrap();
        assert_eq!(pick_best(&c, Some(2021)), 1);
        assert_eq!(pick_best(&c, Some(1999)), 0);
        assert_eq!(pick_best(&c, None), 0);

        let info = MediaInfo {
            n: Some("dune".into()),
            s: Some(1),
            e: Some(2),
            ..Default::default()
        };
        let eps = [Episode {
            season: 1,
            number: 2,
            title: "Titolo".into(),
        }];
        let out = enrich(&info, &c[1], &eps);
        assert_eq!(out.n.as_deref(), Some("Dune"));
        assert_eq!((out.y, out.t.as_deref()), (Some(2021), Some("Titolo")));
        let senza = enrich(&info, &c[1], &[]);
        assert_eq!(senza.t, None);
    }
}
