//! Esecuzione del piano (sposta o copia), file collegati e registro per l'annulla.
//!
//! Regole: non si sovrascrive mai un file esistente; le cartelle create vengono registrate
//! e tolte in annulla solo se rimaste vuote.

use crate::error::Error;
use crate::fsx::write_atomic;
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
    pub errors: Vec<(PathBuf, Error)>,
    pub batch: Batch,
}

/// Vero se `a` e `b` sono lo stesso file e differiscono solo per maiuscole e minuscole
/// (succede su Windows e macOS, dove i nomi non distinguono le maiuscole).
pub fn is_case_rename(a: &Path, b: &Path) -> bool {
    a != b
        && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
        && same_file(a, b)
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}

/// Etichetta ammessa tra il nome del video e l'estensione del file collegato:
/// un codice lingua (`it`, `eng`, `pt-BR`) o un'indicazione nota (`forced`, `sdh`…).
fn is_sidecar_tag(tag: &str) -> bool {
    const FLAGS: &[&str] = &["forced", "sdh", "hi", "cc", "default"];
    if FLAGS.contains(&tag.to_lowercase().as_str()) {
        return true;
    }
    let (lang, region) = tag.split_once('-').unwrap_or((tag, ""));
    (2..=3).contains(&lang.len())
        && lang.chars().all(|c| c.is_ascii_alphabetic())
        && (region.is_empty()
            || ((2..=4).contains(&region.len())
                && region.chars().all(|c| c.is_ascii_alphanumeric())))
}

/// File collegati a `video`: stesso nome base, estensione tra quelle note, al più etichette di lingua in mezzo.
/// Ritorna (percorso, suffisso dopo il nome base): `Film.it.srt` dà il suffisso `.it.srt`.
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
        let Some(rest) = name.strip_prefix(stem).and_then(|r| r.strip_prefix('.')) else {
            continue;
        };
        let mut parts: Vec<&str> = rest.split('.').collect();
        let ext = parts.pop().unwrap_or("").to_lowercase();
        if SIDECAR_EXT.contains(&ext.as_str())
            && parts.len() <= 2
            && parts.iter().all(|t| is_sidecar_tag(t))
        {
            found.push((p.clone(), format!(".{rest}")));
        }
    }
    found.sort();
    found
}

/// Esegue gli elementi pronti (`Status::Ok`) del piano. Dopo ogni video chiama `progress`
/// con il lotto fin qui e il numero di video elaborati, così chi chiama può salvare il registro.
pub fn apply(
    items: &[PlanItem],
    action: Action,
    with_sidecars: bool,
    progress: &mut dyn FnMut(&Batch, usize),
) -> Report {
    let mut rep = Report {
        batch: Batch {
            id: new_batch_id(),
            ..Default::default()
        },
        ..Default::default()
    };
    let ready = items.iter().filter(|i| i.status == Status::Ok);
    for (n, item) in ready.enumerate() {
        if let Some(target) = &item.target {
            match transfer(&item.source, target, action, &mut rep.batch) {
                Ok(()) => {
                    rep.done += 1;
                    if with_sidecars {
                        move_sidecars(&item.source, target, action, &mut rep);
                    }
                }
                Err(e) => rep.errors.push((item.source.clone(), e)),
            }
        }
        progress(&rep.batch, n + 1);
    }
    rep
}

fn move_sidecars(video: &Path, target: &Path, action: Action, rep: &mut Report) {
    let target_base = target.with_extension("");
    let target_name = target_base
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    for (src, suffix) in sidecars(video) {
        let dst = target_base.with_file_name(format!("{target_name}{suffix}"));
        if let Err(e) = transfer(&src, &dst, action, &mut rep.batch) {
            rep.errors.push((src, e));
        }
    }
}

fn transfer(from: &Path, to: &Path, action: Action, batch: &mut Batch) -> Result<(), Error> {
    // Una copia sullo stesso file lo rovinerebbe: la sola eccezione è rinominare cambiando maiuscole.
    if to.exists() && !(action == Action::Move && is_case_rename(from, to)) {
        return Err(Error::TargetExists);
    }
    if let Some(parent) = to.parent() {
        create_dirs_tracked(parent, batch)?;
    }
    match action {
        Action::Copy => copy_file(from, to)?,
        Action::Move => move_file(from, to)?,
    }
    batch.entries.push(Entry {
        action,
        from: from.into(),
        to: to.into(),
    });
    Ok(())
}

/// Copia; se fallisce a metà toglie il file parziale.
fn copy_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to).map(|_| ()).inspect_err(|_| {
        let _ = fs::remove_file(to);
    })
}

/// Rinomina. Solo tra volumi diversi ripiega su copia e cancellazione; se la cancellazione
/// dell'originale fallisce toglie la copia, così non restano due file.
fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_file(from, to)?;
            fs::remove_file(from).inspect_err(|_| {
                let _ = fs::remove_file(to);
            })
        }
        other => other,
    }
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
    batch.created_dirs.extend(missing);
    Ok(())
}

/// Annulla un lotto. Ritorna gli errori per file; ciò che non si può ripristinare resta com'è.
pub fn undo(batch: &Batch) -> Vec<(PathBuf, Error)> {
    let mut errors = Vec::new();
    for e in batch.entries.iter().rev() {
        let res = match e.action {
            Action::Copy if e.to.exists() => fs::remove_file(&e.to).map_err(Error::from),
            Action::Copy => Ok(()),
            Action::Move if e.from.exists() && !is_case_rename(&e.to, &e.from) => {
                Err(Error::OriginalExists)
            }
            Action::Move if !e.to.exists() => Err(Error::MovedFileGone),
            Action::Move => move_file(&e.to, &e.from).map_err(Error::from),
        };
        if let Err(err) = res {
            errors.push((e.to.clone(), err));
        }
    }
    // Dalla cartella più profonda alla meno profonda: una cartella si toglie solo dopo le sue sottocartelle.
    let mut dirs = batch.created_dirs.clone();
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in &dirs {
        let _ = fs::remove_dir(d); // riesce solo se la cartella è vuota
    }
    errors
}

/// Dove si trova `p` dopo il lotto (`forward`) o dopo averlo annullato. `None` se il lotto non l'ha spostato.
pub fn path_after(batch: &Batch, p: &Path, forward: bool) -> Option<PathBuf> {
    batch
        .entries
        .iter()
        .filter(|e| e.action == Action::Move)
        .find_map(|e| {
            let (a, b) = if forward {
                (&e.from, &e.to)
            } else {
                (&e.to, &e.from)
            };
            (a == p).then(|| b.clone())
        })
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
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
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        write_atomic(path, &json, false)
    }

    /// Rilegge il registro dal disco, applica `f` e lo salva. Così due finestre aperte
    /// non si cancellano i lotti a vicenda. Ritorna il registro aggiornato e il risultato di `f`.
    pub fn update<R>(path: &Path, f: impl FnOnce(&mut History) -> R) -> io::Result<(History, R)> {
        let mut h = Self::load(path);
        let r = f(&mut h);
        h.save(path)?;
        Ok((h, r))
    }

    /// Inserisce o sostituisce il lotto con lo stesso id; un lotto vuoto viene tolto.
    pub fn upsert(&mut self, batch: Batch) {
        self.batches.retain(|b| b.id != batch.id);
        if batch.entries.is_empty() {
            return;
        }
        self.batches.push(batch);
        self.batches.sort_by_key(|b| b.id);
        let excess = self.batches.len().saturating_sub(MAX_BATCHES);
        self.batches.drain(..excess);
    }
}

/// Id del lotto in nanosecondi: due finestre aperte non producono lo stesso id.
fn new_batch_id() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Formats, build_plan, info_from_name};
    use crate::template::Template;

    fn touch(p: &Path, body: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn plan_with(files: &[PathBuf], episode: &str, movie: &str) -> Vec<PlanItem> {
        let e = Template::parse(episode).unwrap();
        let m = Template::parse(movie).unwrap();
        build_plan(
            files,
            &Formats {
                episode: &e,
                movie: &m,
            },
            None,
            &|p| info_from_name(None, p),
        )
    }

    fn plan_for(files: &[PathBuf]) -> Vec<PlanItem> {
        plan_with(files, "{n}/Season {s}/{n} - {s00e00}", "{n}")
    }

    fn run(items: &[PlanItem], action: Action, side: bool) -> Report {
        apply(items, action, side, &mut |_, _| {})
    }

    #[test]
    fn sposta_con_file_collegati_e_annulla() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        touch(&d.path().join("show.s01e02.it.srt"), "sub");
        touch(&d.path().join("show.s01e02.nfo"), "nfo");
        touch(&d.path().join("show.s01e03.srt"), "altro");

        let rep = run(&plan_for(std::slice::from_ref(&v)), Action::Move, true);
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
    fn non_prende_i_sottotitoli_di_un_altro_film() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("Dune.mkv");
        touch(&v, "v");
        touch(&d.path().join("Dune.Part.Two.mkv"), "v2");
        touch(&d.path().join("Dune.Part.Two.it.srt"), "s2");
        touch(&d.path().join("Dune.Part2.srt"), "s3");
        touch(&d.path().join("Dune.it.forced.srt"), "s1");
        touch(&d.path().join("Dune.pt-BR.srt"), "s4");
        let names: Vec<String> = sidecars(&v).into_iter().map(|(_, s)| s).collect();
        assert_eq!(names, vec![".it.forced.srt", ".pt-BR.srt"]);
    }

    #[test]
    fn annulla_toglie_tutte_le_cartelle_create_anche_con_due_stagioni() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("show.s01e01.mkv");
        let b = d.path().join("show.s02e01.mkv");
        touch(&a, "a");
        touch(&b, "b");
        let items = plan_with(
            &[a.clone(), b.clone()],
            "TV/{n}/Season {s}/{n} - {s00e00}",
            "{n}",
        );
        let rep = run(&items, Action::Move, false);
        assert_eq!(rep.done, 2);
        assert!(undo(&rep.batch).is_empty());
        assert!(a.exists() && b.exists());
        assert!(!d.path().join("TV").exists());
    }

    #[test]
    fn rinomina_cambiando_solo_le_maiuscole() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("solaris (1972).mkv");
        touch(&v, "v");
        let items = plan_with(std::slice::from_ref(&v), "{n}", "Solaris (1972)");
        let rep = run(&items, Action::Move, false);
        assert!(rep.errors.is_empty(), "{:?}", rep.errors);
        let names: Vec<String> = fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["Solaris (1972).mkv"]);
        assert!(undo(&rep.batch).is_empty());
        let names: Vec<String> = fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["solaris (1972).mkv"]);
    }

    #[cfg(unix)]
    #[test]
    fn spostamento_fallito_non_lascia_copie() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let src_dir = d.path().join("sola-lettura");
        let v = src_dir.join("show.s01e02.mkv");
        touch(&v, "video");
        fs::set_permissions(&src_dir, fs::Permissions::from_mode(0o555)).unwrap();
        // Da root i permessi non contano e il test non direbbe nulla.
        let root = fs::write(src_dir.join("prova"), "").is_ok();
        if !root {
            let out = d.path().join("out");
            let e = Template::parse("{n} - {s00e00}").unwrap();
            let m = Template::parse("{n}").unwrap();
            let items = build_plan(
                std::slice::from_ref(&v),
                &Formats {
                    episode: &e,
                    movie: &m,
                },
                Some(&out),
                &|p| info_from_name(None, p),
            );
            let rep = run(&items, Action::Move, false);
            assert_eq!(rep.errors.len(), 1);
            assert!(v.exists());
            assert!(
                !out.join("show - S01E02.mkv").exists(),
                "nessuna copia rimasta"
            );
        }
        fs::set_permissions(&src_dir, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn copia_e_annulla_cancella_solo_la_copia() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        let rep = run(&plan_for(std::slice::from_ref(&v)), Action::Copy, false);
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
        let rep = run(&items, Action::Move, false);
        assert_eq!(rep.done, 0);
        assert_eq!(rep.errors, vec![(v.clone(), Error::TargetExists)]);
        assert_eq!(
            fs::read_to_string(items[0].target.as_ref().unwrap()).unwrap(),
            "preesistente"
        );
        assert!(v.exists());
    }

    #[test]
    fn annulla_non_sovrascrive_se_l_originale_e_tornato() {
        let d = tempfile::tempdir().unwrap();
        let v = d.path().join("show.s01e02.mkv");
        touch(&v, "video");
        let rep = run(&plan_for(std::slice::from_ref(&v)), Action::Move, false);
        touch(&v, "nuovo file con lo stesso nome");
        let errs = undo(&rep.batch);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].1, Error::OriginalExists);
        assert_eq!(
            fs::read_to_string(&v).unwrap(),
            "nuovo file con lo stesso nome"
        );
    }

    #[test]
    fn avanzamento_e_percorsi_dopo_il_lotto() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("show.s01e01.mkv");
        let b = d.path().join("show.s01e02.mkv");
        touch(&a, "a");
        touch(&b, "b");
        let mut seen = Vec::new();
        let rep = apply(
            &plan_for(&[a.clone(), b.clone()]),
            Action::Move,
            false,
            &mut |batch, n| seen.push((n, batch.entries.len())),
        );
        assert_eq!(seen, vec![(1, 1), (2, 2)]);
        let moved = path_after(&rep.batch, &a, true).unwrap();
        assert!(moved.ends_with("show/Season 1/show - S01E01.mkv"));
        assert_eq!(path_after(&rep.batch, &moved, false).unwrap(), a);
        assert_eq!(path_after(&rep.batch, Path::new("/altro.mkv"), true), None);
    }

    #[test]
    fn registro_rilegge_il_disco_prima_di_salvare() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("dati/history.json");
        let b = |id| Batch {
            id,
            entries: vec![Entry {
                action: Action::Move,
                from: "a".into(),
                to: "b".into(),
            }],
            created_dirs: vec![],
        };
        // Due finestre: ognuna aggiunge il suo lotto, nessuna cancella quello dell'altra.
        History::update(&path, |h| h.upsert(b(1))).unwrap();
        History::update(&path, |h| h.upsert(b(2))).unwrap();
        let (h, popped) = History::update(&path, |h| h.batches.pop()).unwrap();
        assert_eq!(popped.map(|x| x.id), Some(2));
        assert_eq!(h.batches, vec![b(1)]);
        assert_eq!(History::load(&path), h);

        let mut h = History::default();
        h.upsert(Batch {
            id: 3,
            ..Default::default()
        });
        assert!(h.batches.is_empty(), "un lotto vuoto non entra");
        assert!(
            History::load(&d.path().join("non-esiste.json"))
                .batches
                .is_empty()
        );
    }
}
