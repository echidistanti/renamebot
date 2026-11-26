//! Ricerca di serie, film ed episodi su TheMovieDB e TVmaze.

use crate::config::{Config, Provider};
use crate::error::Error;
use crate::http::{Http, encode};
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

fn tmdb_key(cfg: &Config) -> Result<&str, Error> {
    let k = cfg.tmdb_key.trim();
    if k.is_empty() {
        Err(Error::TmdbKeyMissing)
    } else {
        Ok(k)
    }
}

/// GET verso TMDB: un 401 vuol dire chiave sbagliata.
fn tmdb_get(f: &dyn Http, url: &str) -> Result<String, Error> {
    f.get(url).map_err(|e| {
        if e == Error::Http(401) {
            Error::TmdbKeyInvalid
        } else {
            e
        }
    })
}

fn json(body: &str) -> Result<Value, Error> {
    serde_json::from_str(body).map_err(|e| Error::BadResponse(e.to_string()))
}

pub fn parse_tmdb_search(body: &str, tv: bool) -> Result<Vec<Candidate>, Error> {
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

pub fn parse_tmdb_season(body: &str) -> Result<Vec<Episode>, Error> {
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

pub fn parse_tvmaze_search(body: &str) -> Result<Vec<Candidate>, Error> {
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
pub fn parse_tvmaze_episodes(body: &str) -> Result<Vec<Episode>, Error> {
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
/// Database usato davvero per le serie: senza chiave TMDB si passa a TVmaze, che non la chiede.
pub fn series_provider(cfg: &Config) -> Provider {
    match cfg.provider {
        Provider::Tmdb if cfg.tmdb_key.trim().is_empty() => Provider::Tvmaze,
        p => p,
    }
}

pub fn search_series(f: &dyn Http, cfg: &Config, query: &str) -> Result<Vec<Candidate>, Error> {
    match series_provider(cfg) {
        Provider::Tmdb => {
            let url = format!(
                "https://api.themoviedb.org/3/search/tv?api_key={}&language={}&query={}",
                tmdb_key(cfg)?,
                cfg.tmdb_lang(),
                encode(query)
            );
            parse_tmdb_search(&tmdb_get(f, &url)?, true)
        }
        Provider::Tvmaze => {
            let url = format!("https://api.tvmaze.com/search/shows?q={}", encode(query));
            parse_tvmaze_search(&f.get(&url)?)
        }
    }
}

/// I film si cercano solo su TMDB, anche se il provider scelto è TVmaze.
pub fn search_movies(f: &dyn Http, cfg: &Config, query: &str) -> Result<Vec<Candidate>, Error> {
    let url = format!(
        "https://api.themoviedb.org/3/search/movie?api_key={}&language={}&query={}",
        tmdb_key(cfg)?,
        cfg.tmdb_lang(),
        encode(query)
    );
    parse_tmdb_search(&tmdb_get(f, &url)?, false)
}

/// Episodi delle stagioni richieste (TVmaze li restituisce tutti insieme e si filtra dopo).
/// Una stagione che TMDB non conosce viene saltata; l'errore torna solo se falliscono tutte.
pub fn episodes(
    f: &dyn Http,
    cfg: &Config,
    cand: &Candidate,
    seasons: &[u32],
) -> Result<Vec<Episode>, Error> {
    match cand.provider {
        Provider::Tmdb => {
            let key = tmdb_key(cfg)?;
            let mut all = Vec::new();
            let mut first_err = None;
            let mut any_ok = false;
            for s in seasons {
                let url = format!(
                    "https://api.themoviedb.org/3/tv/{}/season/{s}?api_key={key}&language={}",
                    cand.id,
                    cfg.tmdb_lang()
                );
                match tmdb_get(f, &url).and_then(|b| parse_tmdb_season(&b)) {
                    Ok(eps) => {
                        any_ok = true;
                        all.extend(eps);
                    }
                    Err(e) => first_err = first_err.or(Some(e)),
                }
            }
            match first_err {
                Some(e) if !any_ok => Err(e),
                _ => Ok(all),
            }
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

/// Chiave del gruppo: file con lo stesso titolo (e lo stesso tipo) si cercano online una volta sola.
pub fn group_key(info: &MediaInfo) -> Option<String> {
    Some(format!(
        "{}|{}",
        info.n.as_deref()?.to_lowercase(),
        info.is_episode()
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupSpec {
    pub key: String,
    pub query: String,
    pub is_episode: bool,
    pub year: Option<u32>,
    pub seasons: Vec<u32>,
}

/// Raggruppa i file per titolo, raccogliendo le stagioni presenti. I file senza titolo restano fuori.
pub fn group_files<'a>(infos: impl IntoIterator<Item = &'a MediaInfo>) -> Vec<GroupSpec> {
    let mut groups: Vec<GroupSpec> = Vec::new();
    for info in infos {
        let (Some(key), Some(name)) = (group_key(info), info.n.clone()) else {
            continue;
        };
        let idx = match groups.iter().position(|g| g.key == key) {
            Some(i) => i,
            None => {
                groups.push(GroupSpec {
                    key,
                    query: name,
                    is_episode: info.is_episode(),
                    year: info.y,
                    seasons: Vec::new(),
                });
                groups.len() - 1
            }
        };
        if let Some(s) = info.s
            && !groups[idx].seasons.contains(&s)
        {
            groups[idx].seasons.push(s);
        }
    }
    groups
}

/// Ricerca completa di un gruppo: candidati, indice del migliore ed episodi del migliore.
pub fn find(
    f: &dyn Http,
    cfg: &Config,
    g: &GroupSpec,
) -> Result<(Vec<Candidate>, usize, Vec<Episode>), Error> {
    let cands = if g.is_episode {
        search_series(f, cfg, &g.query)?
    } else {
        search_movies(f, cfg, &g.query)?
    };
    if cands.is_empty() {
        return Err(Error::NoResults);
    }
    let best = pick_best(&cands, g.year);
    let eps = if g.is_episode {
        episodes(f, cfg, &cands[best], &g.seasons)?
    } else {
        Vec::new()
    };
    Ok((cands, best, eps))
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
    impl Http for Canned {
        fn send(
            &self,
            _m: crate::http::Method,
            url: &str,
            _h: &[(&str, &str)],
            _b: Option<&str>,
        ) -> Result<Vec<u8>, Error> {
            self.1.lock().unwrap().push(url.to_string());
            self.0
                .lock()
                .unwrap()
                .get(url)
                .map(|s| s.as_bytes().to_vec())
                .ok_or(Error::Http(404))
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
    fn senza_chiave_tmdb_le_serie_passano_a_tvmaze() {
        let f = Canned::new(&[(
            "https://api.tvmaze.com/search/shows?q=Twin%20Peaks",
            MAZE_SEARCH,
        )]);
        let cfg = Config::default();
        assert_eq!(cfg.provider, Provider::Tmdb);
        assert_eq!(series_provider(&cfg), Provider::Tvmaze);
        let c = search_series(&f, &cfg, "Twin Peaks").unwrap();
        assert_eq!((c.len(), c[0].provider), (2, Provider::Tvmaze));
        // I film invece si cercano solo su TMDB: senza chiave l'errore resta e dice cosa fare.
        assert_eq!(
            search_movies(&f, &cfg, "x").unwrap_err(),
            Error::TmdbKeyMissing
        );
        let con_chiave = Config {
            tmdb_key: "K".into(),
            ..Default::default()
        };
        assert_eq!(series_provider(&con_chiave), Provider::Tmdb);
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
    fn una_stagione_sconosciuta_non_blocca_le_altre() {
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
        assert_eq!(
            episodes(&f, &cfg, &tm, &[0, 2]).unwrap().len(),
            2,
            "la stagione 0 manca: si salta"
        );
        assert_eq!(
            episodes(&f, &cfg, &tm, &[0, 9]).unwrap_err(),
            Error::Http(404)
        );
        assert!(episodes(&f, &cfg, &tm, &[]).unwrap().is_empty());
    }

    #[test]
    fn chiave_tmdb_rifiutata() {
        struct No;
        impl Http for No {
            fn send(
                &self,
                _m: crate::http::Method,
                _u: &str,
                _h: &[(&str, &str)],
                _b: Option<&str>,
            ) -> Result<Vec<u8>, Error> {
                Err(Error::Http(401))
            }
        }
        let cfg = Config {
            tmdb_key: "K".into(),
            ..Default::default()
        };
        assert_eq!(
            search_movies(&No, &cfg, "x").unwrap_err(),
            Error::TmdbKeyInvalid
        );
    }

    #[test]
    fn raggruppa_per_titolo_e_raccoglie_le_stagioni() {
        let ep = |n: &str, s, e| MediaInfo {
            n: Some(n.into()),
            s: Some(s),
            e: Some(e),
            ..Default::default()
        };
        let infos = vec![
            ep("Twin Peaks", 1, 1),
            ep("twin peaks", 2, 1),
            ep("Twin Peaks", 1, 2),
            MediaInfo {
                n: Some("Twin Peaks".into()),
                y: Some(1992),
                ..Default::default()
            },
            MediaInfo::default(),
        ];
        let g = group_files(&infos);
        assert_eq!(
            g.len(),
            2,
            "serie e film con lo stesso titolo sono gruppi diversi; senza titolo fuori"
        );
        assert_eq!(
            (g[0].query.as_str(), g[0].seasons.clone()),
            ("Twin Peaks", vec![1, 2])
        );
        assert!(!g[1].is_episode && g[1].year == Some(1992));
    }

    #[test]
    fn ricerca_completa_di_un_gruppo() {
        let cfg = Config {
            provider: Provider::Tvmaze,
            ..Default::default()
        };
        let f = Canned::new(&[
            (
                "https://api.tvmaze.com/search/shows?q=Twin%20Peaks",
                MAZE_SEARCH,
            ),
            ("https://api.tvmaze.com/shows/123/episodes", MAZE_EPS),
        ]);
        let g = GroupSpec {
            key: "k".into(),
            query: "Twin Peaks".into(),
            is_episode: true,
            year: Some(1990),
            seasons: vec![2],
        };
        let (c, best, eps) = find(&f, &cfg, &g).unwrap();
        assert_eq!((c.len(), best, eps.len()), (2, 0, 1));
        let vuoto = Canned::new(&[("https://api.tvmaze.com/search/shows?q=Twin%20Peaks", "[]")]);
        assert_eq!(find(&vuoto, &cfg, &g).unwrap_err(), Error::NoResults);
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
