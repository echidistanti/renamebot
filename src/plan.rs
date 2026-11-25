//! Piano di rinomina: per ogni file calcola il percorso di destinazione e segnala i problemi.

use crate::guess::guess;
use crate::mask::Mask;
use crate::ops::is_case_rename;
use crate::template::{MediaInfo, Template};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ok,
    Unchanged,
    /// Il file di destinazione esiste già sul disco.
    Exists,
    /// Un altro file del piano punta alla stessa destinazione.
    Duplicate,
    /// Mancano campi obbligatori del formato.
    Missing(Vec<String>),
    /// La maschera non corrisponde al nome.
    NoMatch,
}

impl Status {
    pub fn is_blocking(&self) -> bool {
        !matches!(self, Status::Ok | Status::Unchanged)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanItem {
    pub source: PathBuf,
    pub target: Option<PathBuf>,
    pub status: Status,
}

pub struct Formats<'a> {
    pub episode: &'a Template,
    pub movie: &'a Template,
}

/// Dati di un file dal suo nome: con `mask` dalla maschera, senza dal riconoscimento automatico.
/// `None` se la maschera non corrisponde.
pub fn info_from_name(mask: Option<&Mask>, path: &Path) -> Option<MediaInfo> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    match mask {
        Some(m) => m.extract(stem),
        None => Some(guess(stem)),
    }
}

/// Calcola il piano. I dati di ogni file arrivano da `info_for` (`None` = non riconosciuto).
pub fn build_plan(
    files: &[PathBuf],
    formats: &Formats,
    dest_root: Option<&Path>,
    info_for: &dyn Fn(&Path) -> Option<MediaInfo>,
) -> Vec<PlanItem> {
    let mut items: Vec<PlanItem> = files
        .iter()
        .map(|f| plan_one(f, info_for(f), formats, dest_root))
        .collect();

    let mut counts: HashMap<PathBuf, usize> = HashMap::new();
    for it in &items {
        if let Some(t) = &it.target {
            *counts.entry(t.clone()).or_default() += 1;
        }
    }
    for it in &mut items {
        if it.status == Status::Ok
            && let Some(t) = &it.target
            && counts.get(t).copied().unwrap_or(0) > 1
        {
            it.status = Status::Duplicate;
        }
    }
    items
}

fn plan_one(
    source: &Path,
    info: Option<MediaInfo>,
    formats: &Formats,
    dest_root: Option<&Path>,
) -> PlanItem {
    let Some(info) = info else {
        return PlanItem {
            source: source.into(),
            target: None,
            status: Status::NoMatch,
        };
    };

    let template = if info.is_episode() {
        formats.episode
    } else {
        formats.movie
    };
    let rendered = template.render(&info);
    if !rendered.missing.is_empty() || rendered.path.is_empty() {
        let missing = if rendered.missing.is_empty() {
            vec!["?".into()]
        } else {
            rendered.missing
        };
        return PlanItem {
            source: source.into(),
            target: None,
            status: Status::Missing(missing),
        };
    }

    let root = dest_root
        .map(Path::to_path_buf)
        .or_else(|| source.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let mut name = rendered.path;
    if let Some(ext) = source.extension().and_then(|e| e.to_str()) {
        name.push('.');
        name.push_str(ext);
    }
    let target = root.join(name);

    let status = if target == source {
        Status::Unchanged
    } else if target.exists() && !is_case_rename(source, &target) {
        Status::Exists
    } else {
        Status::Ok
    };
    PlanItem {
        source: source.into(),
        target: Some(target),
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fmts() -> (Template, Template) {
        (
            Template::parse("{n}/Season {s}/{n} - {s00e00}{t|prefix: - }").unwrap(),
            Template::parse("{n}{y|prefix: (|suffix:)}").unwrap(),
        )
    }

    fn bp(
        files: &[PathBuf],
        mask: Option<&Mask>,
        f: &Formats,
        dest: Option<&Path>,
    ) -> Vec<PlanItem> {
        build_plan(files, f, dest, &|p| info_from_name(mask, p))
    }

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"").unwrap();
        p
    }

    #[test]
    fn piano_automatico_serie_e_film() {
        let d = tempfile::tempdir().unwrap();
        let a = touch(d.path(), "twin.peaks.s01e02.mkv");
        let b = touch(d.path(), "Blade.Runner.1982.1080p.mp4");
        let (e, m) = fmts();
        let plan = bp(
            &[a, b],
            None,
            &Formats {
                episode: &e,
                movie: &m,
            },
            None,
        );
        assert_eq!(
            plan[0].target.as_ref().unwrap(),
            &d.path().join("twin peaks/Season 1/twin peaks - S01E02.mkv")
        );
        assert_eq!(
            plan[1].target.as_ref().unwrap(),
            &d.path().join("Blade Runner (1982).mp4")
        );
        assert!(plan.iter().all(|p| p.status == Status::Ok));
    }

    #[test]
    fn destinazione_esistente_e_duplicati() {
        let d = tempfile::tempdir().unwrap();
        let a = touch(d.path(), "Solaris.1972.mkv");
        let _gia = touch(d.path(), "Solaris (1972).mkv");
        let b = touch(d.path(), "Stalker.1979.mkv");
        let c = touch(d.path(), "Stalker.1979.1080p.mkv");
        let (e, m) = fmts();
        let plan = bp(
            &[a, b, c],
            None,
            &Formats {
                episode: &e,
                movie: &m,
            },
            None,
        );
        assert_eq!(plan[0].status, Status::Exists);
        assert_eq!(plan[1].status, Status::Duplicate);
        assert_eq!(plan[2].status, Status::Duplicate);
    }

    #[test]
    fn maschera_non_corrispondente_e_campi_mancanti() {
        let d = tempfile::tempdir().unwrap();
        let a = touch(d.path(), "qualcosa.mkv");
        let (e, m) = fmts();
        let mask = Mask::compile("{n} - S{s}E{e}").unwrap();
        let plan = bp(
            std::slice::from_ref(&a),
            Some(&mask),
            &Formats {
                episode: &e,
                movie: &m,
            },
            None,
        );
        assert_eq!(plan[0].status, Status::NoMatch);

        let solo_e = Template::parse("{t}").unwrap();
        let plan = bp(
            &[a],
            None,
            &Formats {
                episode: &solo_e,
                movie: &solo_e,
            },
            None,
        );
        assert_eq!(plan[0].status, Status::Missing(vec!["t".into()]));
    }

    #[test]
    fn invariato_e_cartella_di_destinazione() {
        let d = tempfile::tempdir().unwrap();
        let a = touch(d.path(), "Solaris (1972).mkv");
        let (e, m) = fmts();
        let f = Formats {
            episode: &e,
            movie: &m,
        };
        let plan = bp(std::slice::from_ref(&a), None, &f, None);
        assert_eq!(plan[0].status, Status::Unchanged);

        let out = d.path().join("out");
        let plan = bp(&[a], None, &f, Some(&out));
        assert_eq!(
            plan[0].target.as_ref().unwrap(),
            &out.join("Solaris (1972).mkv")
        );
        assert_eq!(plan[0].status, Status::Ok);
    }

    #[test]
    fn cambiare_solo_le_maiuscole_e_permesso() {
        // Su Windows e macOS il nome nuovo "esiste" perché è lo stesso file: non è un conflitto.
        let d = tempfile::tempdir().unwrap();
        let a = touch(d.path(), "solaris (1972).mkv");
        let (e, _) = fmts();
        let m = Template::parse("Solaris (1972)").unwrap();
        let f = Formats {
            episode: &e,
            movie: &m,
        };
        assert_eq!(
            bp(std::slice::from_ref(&a), None, &f, None)[0].status,
            Status::Ok
        );

        // Dove le maiuscole contano, un secondo file con quel nome è un conflitto vero.
        if cfg!(target_os = "linux") {
            touch(d.path(), "Solaris (1972).mkv");
            assert_eq!(bp(&[a], None, &f, None)[0].status, Status::Exists);
        }
    }
}
