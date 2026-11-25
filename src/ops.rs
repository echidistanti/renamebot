//! Esecuzione del piano (sposta o copia), file collegati e registro per l'annulla.
//!
//! Regole: non si sovrascrive mai un file esistente; le cartelle create vengono registrate
//! e tolte in annulla solo se rimaste vuote.

use crate::plan::{PlanItem, Status};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Estensioni dei file che seguono il video.
pub const SIDECAR_EXT: &[&str] = &["srt", "ass", "ssa", "sub", "idx", "nfo"];
const MAX_BATCHES: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Move,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub action: Action,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    pub id: u64,
    pub entries: Vec<Entry>,
    pub created_dirs: Vec<PathBuf>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub done: usize,
    pub errors: Vec<(PathBuf, String)>,
    pub batch: Batch,
}

/// File collegati a `video`: stesso nome base, estensione tra quelle note. Ritorna (percorso, suffisso dopo il nome base),
/// per esempio `Film.it.srt` dà il suffisso `.it.srt`.
pub fn sidecars(video: &Path) -> Vec<(PathBuf, String)> {
    let (Some(dir), Some(stem)) = (video.parent(), video.file_stem().and_then(|s| s.to_str()))
    else {
        return Vec::new();
    };
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if p == video || !p.is_file() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(rest) = name.strip_prefix(stem) else {
            continue;
        };
        let ext_ok = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SIDECAR_EXT.contains(&e.to_lowercase().as_str()));
        if rest.starts_with('.') && ext_ok {
            found.push((p.clone(), rest.to_string()));
        }
    }
    found.sort();
    found
}

/// Esegue gli elementi pronti (`Status::Ok`) del piano.
pub fn apply(items: &[PlanItem], action: Action, with_sidecars: bool) -> Report {
    let mut rep = Report {
        batch: Batch {
            id: now_secs(),
            ..Default::default()
        },
        ..Default::default()
    };
    for item in items.iter().filter(|i| i.status == Status::Ok) {
        let Some(target) = &item.target else { continue };
        match transfer(&item.source, target, action, &mut rep.batch) {
            Ok(()) => rep.done += 1,
            Err(e) => {
                rep.errors.push((item.source.clone(), e.to_string()));
                continue;
            }
        }
        if with_sidecars {
            let target_base = target.with_extension("");
            let target_name = target_base
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            for (src, suffix) in sidecars(&item.source) {
                let dst = target_base.with_file_name(format!("{target_name}{suffix}"));
                if dst.exists() {
                    rep.errors.push((src, "destinazione già esistente".into()));
                    continue;
                }
                if let Err(e) = transfer(&src, &dst, action, &mut rep.batch) {
                    rep.errors.push((src, e.to_string()));
                }
            }
        }
    }
    rep
}

fn transfer(from: &Path, to: &Path, action: Action, batch: &mut Batch) -> io::Result<()> {
    if to.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destinazione già esistente",
        ));
    }
    if let Some(parent) = to.parent() {
        create_dirs_tracked(parent, batch)?;
    }
    match action {
        Action::Copy => {
            fs::copy(from, to)?;
        }
        Action::Move => {
            if fs::rename(from, to).is_err() {
                // Volumi diversi: copia e poi rimozione dell'originale.
                fs::copy(from, to)?;
                fs::remove_file(from)?;
            }
        }
    }
    batch.entries.push(Entry {
        action,
        from: from.into(),
        to: to.into(),
    });
    Ok(())
}

fn create_dirs_tracked(dir: &Path, batch: &mut Batch) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.as_os_str().is_empty() || d.exists() {
            break;
        }
        missing.push(d.to_path_buf());
        cur = d.parent();
    }
    fs::create_dir_all(dir)?;
    // Dal più interno al più esterno: in annulla si rimuovono in quest'ordine.
    batch.created_dirs.extend(missing);
    Ok(())
}

/// Annulla un lotto. Ritorna gli errori per file; ciò che non si può ripristinare resta com'è.
pub fn undo(batch: &Batch) -> Vec<(PathBuf, String)> {
    let mut errors = Vec::new();
    for e in batch.entries.iter().rev() {
        let res = match e.action {
            Action::Copy => {
                if e.to.exists() {
                    fs::remove_file(&e.to)
                } else {
                    Ok(())
                }
            }
            Action::Move => {
                if e.from.exists() {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "l'originale esiste già",
                    ))
                } else if !e.to.exists() {
                    Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "il file spostato non c'è più",
                    ))
                } else if fs::rename(&e.to, &e.from).is_err() {
                    fs::copy(&e.to, &e.from).and_then(|_| fs::remove_file(&e.to))
                } else {
                    Ok(())
                }
            }
        };
        if let Err(err) = res {
            errors.push((e.to.clone(), err.to_string()));
        }
    }
    for d in &batch.created_dirs {
        let _ = fs::remove_dir(d); // riesce solo se la cartella è vuota
    }
    errors
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct History {
    pub batches: Vec<Batch>,
}

impl History {
    /// Percorso predefinito del registro nella cartella dati dell'utente.
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("Bobina").join("history.json"))
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
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        fs::write(path, json)
    }

    pub fn push(&mut self, batch: Batch) {
        if batch.entries.is_empty() {
            return;
        }
        self.batches.push(batch);
        let excess = self.batches.len().saturating_sub(MAX_BATCHES);
        self.batches.drain(..excess);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mask::Mask;
    use crate::plan::{Formats, build_plan};
    use crate::template::Template;

    fn touch(p: &Path, body: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn plan_for(files: &[PathBuf]) -> Vec<PlanItem> {
        let e = Template::parse("{n}/Season {s}/{n} - {s00e00}").unwrap();
        let m = Template::parse("{n}").unwrap();
        build_plan(
            files,
            None::<&Mask>,
            &Formats {
                episode: &e,
                movie: &m,
            },
            None,
        )
    }

    #[test]
    fn sposta_con_file_collegati_e_annulla() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        touch(&d.path().join("show.s01e02.it.srt"), "sub");
        touch(&d.path().join("show.s01e02.nfo"), "nfo");
        touch(&d.path().join("show.s01e03.srt"), "altro");

        let rep = apply(&plan_for(std::slice::from_ref(&v)), Action::Move, true);
        assert_eq!(rep.done, 1);
        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
        let base = d.path().join("show/Season 1");
        assert_eq!(
            fs::read_to_string(base.join("show - S01E02.mkv")).unwrap(),
            "video"
        );
        assert_eq!(
            fs::read_to_string(base.join("show - S01E02.it.srt")).unwrap(),
            "sub"
        );
        assert!(base.join("show - S01E02.nfo").exists());
        assert!(
            d.path().join("show.s01e03.srt").exists(),
            "il sottotitolo di un altro episodio resta"
        );
        assert!(!v.exists());

        let errs = undo(&rep.batch);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(fs::read_to_string(&v).unwrap(), "video");
        assert!(d.path().join("show.s01e02.it.srt").exists());
        assert!(
            !d.path().join("show").exists(),
            "le cartelle create sono rimosse"
        );
    }

    #[test]
    fn copia_e_annulla_cancella_solo_la_copia() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        let rep = apply(&plan_for(std::slice::from_ref(&v)), Action::Copy, false);
        assert_eq!(rep.done, 1);
        assert!(v.exists());
        assert!(undo(&rep.batch).is_empty());
        assert!(v.exists());
        assert!(!d.path().join("show").exists());
    }

    #[test]
    fn non_sovrascrive_mai() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        let items = plan_for(std::slice::from_ref(&v));
        // La destinazione compare dopo il calcolo del piano.
        touch(items[0].target.as_ref().unwrap(), "preesistente");
        let rep = apply(&items, Action::Move, false);
        assert_eq!(rep.done, 0);
        assert_eq!(rep.errors.len(), 1);
        assert_eq!(
            fs::read_to_string(items[0].target.as_ref().unwrap()).unwrap(),
            "preesistente"
        );
        assert!(v.exists());
    }

    #[test]
    fn annulla_non_clobbera_se_l_originale_e_tornato() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        let rep = apply(&plan_for(std::slice::from_ref(&v)), Action::Move, false);
        touch(&v, "nuovo file con lo stesso nome");
        let errs = undo(&rep.batch);
        assert_eq!(errs.len(), 1);
        assert_eq!(
            fs::read_to_string(&v).unwrap(),
            "nuovo file con lo stesso nome"
        );
    }

    #[test]
    fn registro_persistente() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("dati/history.json");
        let mut h = History::default();
        let b = Batch {
            id: 1,
            entries: vec![Entry {
                action: Action::Move,
                from: "a".into(),
                to: "b".into(),
            }],
            created_dirs: vec!["x".into()],
        };
        h.push(b.clone());
        h.push(Batch::default()); // lotto vuoto ignorato
        h.save(&path).unwrap();
        let loaded = History::load(&path);
        assert_eq!(loaded.batches, vec![b]);
        assert!(
            History::load(&d.path().join("non-esiste.json"))
                .batches
                .is_empty()
        );
    }
}
