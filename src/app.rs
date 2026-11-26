//! Finestra principale: elenco file, maschera, formati, anteprima della rinomina e sottotitoli.
//! Il lavoro lento (rete, spostamenti di file) gira in thread separati che rispondono con `Msg`.

use crate::ui::{self, Kind, muted};
use eframe::egui::{self, Margin, RichText, Stroke};
use renamebot::config::{Config, Preset, Provider, builtin_presets};
use renamebot::error::Error;
use renamebot::http::{Cached, Http, Ureq};
use renamebot::i18n::{Lang, tr};
use renamebot::mask::Mask;
use renamebot::metadata::{self, Candidate, Episode, GroupSpec};
use renamebot::ops::{self, Action, Batch, History, Report};
use renamebot::plan::{Formats, PlanItem, Status, build_plan, info_from_name};
use renamebot::subs::{self, Client, Session, SubCandidate};
use renamebot::template::{Fields, MediaInfo, Template};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const VIDEO_EXT: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "m4v", "wmv", "ts", "mpg", "mpeg", "webm", "flv",
];

/// Lingue proposte per i sottotitoli: codice OpenSubtitles e nome.
const LANGS: &[(&str, &str)] = &[
    ("it", "Italiano"),
    ("en", "English"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("pt-BR", "Português (Brasil)"),
    ("pt-PT", "Português"),
    ("ru", "Russian"),
    ("ja", "Japanese"),
    ("zh-CN", "Chinese (Simplified)"),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Rename,
    Subs,
}

#[derive(Clone, PartialEq)]
enum SubState {
    Searching,
    Found,
    NotFound,
    Downloading,
    Saved(PathBuf),
    Failed(Error),
}

struct SubRow {
    path: PathBuf,
    state: SubState,
    candidates: Vec<SubCandidate>,
    selected: usize,
    include: bool,
}

#[derive(Clone, PartialEq)]
enum GroupState {
    Loading,
    Ready,
    Failed(Error),
}

/// File che condividono lo stesso titolo: si cercano online una volta sola.
struct Group {
    spec: GroupSpec,
    candidates: Vec<Candidate>,
    selected: usize,
    episodes: Vec<Episode>,
    state: GroupState,
}

type Found = Result<(Vec<Candidate>, usize, Vec<Episode>), Error>;

enum Msg {
    Found {
        key: String,
        res: Found,
    },
    Episodes {
        key: String,
        idx: usize,
        res: Result<Vec<Episode>, Error>,
    },
    SubFound {
        path: PathBuf,
        res: Result<Vec<SubCandidate>, Error>,
    },
    SubDone {
        path: PathBuf,
        res: Result<PathBuf, Error>,
    },
    Progress(usize, usize),
    Applied {
        report: Report,
        saved: Result<(), String>,
    },
    Undone {
        batch: Batch,
        errors: Vec<(PathBuf, Error)>,
    },
}

/// Canale verso la finestra: ogni messaggio risveglia l'interfaccia.
#[derive(Clone)]
struct Notify {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Notify {
    fn send(&self, m: Msg) -> bool {
        let ok = self.tx.send(m).is_ok();
        self.ctx.request_repaint();
        ok
    }
}

/// Esito dell'ultima operazione, mostrato sopra la barra in basso finché non lo si chiude.
struct Notice {
    text: String,
    bad: bool,
}

/// Il riquadro per correggere a mano i dati di un file.
struct Editing {
    path: PathBuf,
    fields: Fields,
}

enum Busy {
    Applying(usize, usize),
    Undoing,
}

pub struct App {
    config: Config,
    config_path: Option<PathBuf>,
    files: Vec<PathBuf>,
    plan: Vec<PlanItem>,
    plan_key: String,
    mask_error: Option<String>,
    format_error: Option<String>,
    show_options: bool,
    /// Nome del preset da salvare, mentre lo si scrive.
    save_as: Option<String>,
    /// Dati corretti a mano, per file: prendono il posto di quelli riconosciuti.
    overrides: HashMap<PathBuf, MediaInfo>,
    editing: Option<Editing>,
    action: Action,
    with_sidecars: bool,
    history: History,
    history_path: Option<PathBuf>,
    notice: Option<Notice>,
    busy: Option<Busy>,
    meta_http: Arc<dyn Http>,
    subs_http: Arc<dyn Http>,
    online: bool,
    groups: Vec<Group>,
    revision: u64,
    notify: Notify,
    rx: Receiver<Msg>,
    view: View,
    sub_lang: Option<usize>,
    sub_rows: Vec<SubRow>,
    session: Arc<Mutex<Option<Session>>>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Vec<PathBuf>) -> Self {
        let history_path = History::default_path();
        let history = history_path
            .as_deref()
            .map(History::load)
            .unwrap_or_default();
        let config_path = Config::default_path();
        let config = config_path.as_deref().map(Config::load).unwrap_or_default();
        let cache_dir = Cached::<Ureq>::default_dir()
            .unwrap_or_else(|| std::env::temp_dir().join("renamebot-http"));
        let week = Duration::from_secs(7 * 24 * 3600);
        let (tx, rx) = channel();
        let mut app = Self {
            config,
            config_path,
            files: Vec::new(),
            plan: Vec::new(),
            plan_key: String::new(),
            mask_error: None,
            format_error: None,
            show_options: false,
            save_as: None,
            overrides: HashMap::new(),
            editing: None,
            action: Action::Move,
            with_sidecars: true,
            history,
            history_path,
            notice: None,
            busy: None,
            meta_http: Arc::new(Cached::new(Ureq::default(), cache_dir, week)),
            subs_http: Arc::new(Ureq::default()),
            online: false,
            groups: Vec::new(),
            revision: 0,
            notify: Notify {
                tx,
                ctx: cc.egui_ctx.clone(),
            },
            rx,
            view: View::Rename,
            sub_lang: None,
            sub_rows: Vec::new(),
            session: Arc::new(Mutex::new(None)),
        };
        for p in open {
            app.add_path(&p);
        }
        app
    }

    fn lang(&self) -> Lang {
        self.config.ui_lang
    }

    fn presets(&self) -> Vec<Preset> {
        let mut all = builtin_presets();
        all.extend(self.config.presets.iter().cloned());
        all
    }

    fn save_config(&mut self) {
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            self.notice = Some(Notice {
                text: format!("{}: {e}", tr(self.lang(), "msg_settings_not_saved")),
                bad: true,
            });
        }
    }

    fn add_path(&mut self, path: &Path) {
        if path.is_dir() {
            if let Ok(rd) = std::fs::read_dir(path) {
                let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                entries.sort();
                for p in entries {
                    self.add_path(&p);
                }
            }
        } else if is_video(path) && !self.files.iter().any(|f| f == path) {
            self.files.push(path.to_path_buf());
        }
    }

    /// Dati di un file: dal nome (maschera o riconoscimento automatico), completati online se attivo.
    fn resolve_info(
        mask: Option<&Mask>,
        online: bool,
        groups: &[Group],
        overrides: &HashMap<PathBuf, MediaInfo>,
        path: &Path,
    ) -> Option<MediaInfo> {
        if let Some(fixed) = overrides.get(path) {
            return Some(fixed.clone());
        }
        let base = info_from_name(mask, path)?;
        if online
            && let Some(k) = metadata::group_key(&base)
            && let Some(g) = groups.iter().find(|g| g.spec.key == k)
            && let Some(c) = g.candidates.get(g.selected)
        {
            return Some(metadata::enrich(&base, c, &g.episodes));
        }
        Some(base)
    }

    fn compile_mask(&self) -> Result<Option<Mask>, String> {
        if self.config.mask.trim().is_empty() {
            Ok(None)
        } else {
            Mask::compile(&self.config.mask)
                .map(Some)
                .map_err(|e| e.localized(self.lang()))
        }
    }

    fn refresh_plan(&mut self) {
        let c = &self.config;
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{:?}",
            c.mask,
            c.fmt_episode,
            c.fmt_movie,
            c.dest,
            self.files.len(),
            self.online,
            self.revision,
            c.ui_lang
        );
        if key == self.plan_key {
            return;
        }
        self.plan_key = key;
        self.mask_error = None;
        self.format_error = None;

        let l = self.lang();
        let episode = Template::parse(&self.config.fmt_episode).map_err(|e| e.localized(l));
        let movie = Template::parse(&self.config.fmt_movie).map_err(|e| e.localized(l));
        match (episode, movie, self.compile_mask()) {
            (Ok(e), Ok(m), Ok(mask)) => {
                let dest = self.config.dest.trim();
                let dest = (!dest.is_empty()).then(|| PathBuf::from(dest));
                let (groups, online) = (&self.groups, self.online);
                self.plan = build_plan(
                    &self.files,
                    &Formats {
                        episode: &e,
                        movie: &m,
                    },
                    dest.as_deref(),
                    &|p| Self::resolve_info(mask.as_ref(), online, groups, &self.overrides, p),
                );
            }
            (e, m, k) => {
                self.format_error = e.err().or(m.err());
                self.mask_error = k.err();
                self.plan.clear();
            }
        }
    }

    /// Raggruppa i file per titolo e avvia la ricerca online in un thread.
    fn start_search(&mut self) {
        let Ok(mask) = self.compile_mask() else {
            return;
        };
        let infos: Vec<MediaInfo> = self
            .files
            .iter()
            .filter_map(|f| info_from_name(mask.as_ref(), f))
            .collect();
        let specs = metadata::group_files(&infos);
        self.groups = specs
            .iter()
            .map(|spec| Group {
                spec: spec.clone(),
                candidates: Vec::new(),
                selected: 0,
                episodes: Vec::new(),
                state: GroupState::Loading,
            })
            .collect();
        self.revision += 1;
        let (f, cfg, n) = (
            self.meta_http.clone(),
            self.config.clone(),
            self.notify.clone(),
        );
        std::thread::spawn(move || {
            for spec in specs {
                let res = metadata::find(f.as_ref(), &cfg, &spec);
                if !n.send(Msg::Found { key: spec.key, res }) {
                    return;
                }
            }
        });
    }

    /// L'utente sceglie un altro candidato: si scaricano gli episodi della nuova serie.
    fn select_candidate(&mut self, gi: usize, sel: usize) {
        let g = &mut self.groups[gi];
        g.selected = sel;
        self.revision += 1;
        if !g.spec.is_episode {
            return;
        }
        g.state = GroupState::Loading;
        let (f, cfg, n) = (
            self.meta_http.clone(),
            self.config.clone(),
            self.notify.clone(),
        );
        let (key, cand, seasons) = (
            g.spec.key.clone(),
            g.candidates[sel].clone(),
            g.spec.seasons.clone(),
        );
        std::thread::spawn(move || {
            let res = metadata::episodes(f.as_ref(), &cfg, &cand, &seasons);
            n.send(Msg::Episodes { key, idx: sel, res });
        });
    }

    fn sub_search(&mut self) {
        let Some(li) = self.sub_lang else { return };
        let lang = LANGS[li].0.to_string();
        let mask = self.compile_mask().ok().flatten();
        let mut jobs = Vec::new();
        self.sub_rows.clear();
        for f in &self.files {
            let info =
                Self::resolve_info(mask.as_ref(), self.online, &self.groups, &self.overrides, f);
            self.sub_rows.push(SubRow {
                path: f.clone(),
                state: SubState::Searching,
                candidates: Vec::new(),
                selected: 0,
                include: true,
            });
            jobs.push((f.clone(), info));
        }
        let client = Client::new(self.subs_http.clone(), &self.config);
        let n = self.notify.clone();
        std::thread::spawn(move || {
            for (path, info) in jobs {
                let res = match info {
                    None => Err(Error::NameNotRecognized),
                    Some(info) => {
                        let hash = subs::moviehash(&path).ok();
                        client.search(&info, hash.as_deref(), &lang)
                    }
                };
                if !n.send(Msg::SubFound { path, res }) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
    }

    fn sub_download(&mut self) {
        let Some(li) = self.sub_lang else { return };
        let lang = LANGS[li].0.to_string();
        let mut jobs = Vec::new();
        for r in self.sub_rows.iter_mut() {
            if r.include
                && r.state == SubState::Found
                && let Some(c) = r.candidates.get(r.selected)
            {
                r.state = SubState::Downloading;
                jobs.push((r.path.clone(), c.clone()));
            }
        }
        if jobs.is_empty() {
            return;
        }
        let client = Client::new(self.subs_http.clone(), &self.config);
        let (session, n) = (self.session.clone(), self.notify.clone());
        std::thread::spawn(move || {
            for (path, cand) in jobs {
                let res = (|| {
                    let target = subs::subtitle_path(&path, &lang, &cand.file_name);
                    if target.exists() {
                        return Err(Error::TargetExists);
                    }
                    let mut guard = session.lock().unwrap_or_else(|e| e.into_inner());
                    if guard.is_none() {
                        *guard = Some(client.login()?);
                    }
                    let s = guard.clone().expect("sessione appena creata");
                    drop(guard);
                    let (bytes, _name) = client.download(&s, cand.file_id).inspect_err(|_| {
                        // Il token può essere scaduto: al prossimo tentativo si rifà il login.
                        *session.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    })?;
                    std::fs::write(&target, bytes)?;
                    Ok(target)
                })();
                if !n.send(Msg::SubDone { path, res }) {
                    return;
                }
            }
        });
    }

    /// Esegue il piano in un thread, con avanzamento; il registro si salva durante il lavoro.
    fn start_apply(&mut self) {
        let items: Vec<PlanItem> = self
            .plan
            .iter()
            .filter(|i| i.status == Status::Ok)
            .cloned()
            .collect();
        let total = items.len();
        if total == 0 {
            return;
        }
        self.busy = Some(Busy::Applying(0, total));
        let (action, side, hp, n) = (
            self.action,
            self.with_sidecars,
            self.history_path.clone(),
            self.notify.clone(),
        );
        std::thread::spawn(move || {
            let save = |b: &Batch| match &hp {
                Some(p) => History::update(p, |h| h.upsert(b.clone()))
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                None => Ok(()),
            };
            let mut last_save = Instant::now();
            let report = ops::apply(&items, action, side, &mut |batch, done| {
                if last_save.elapsed() >= Duration::from_secs(1) {
                    let _ = save(batch);
                    last_save = Instant::now();
                }
                n.send(Msg::Progress(done, total));
            });
            let saved = save(&report.batch);
            n.send(Msg::Applied { report, saved });
        });
    }

    fn start_undo(&mut self) {
        let batch = match &self.history_path {
            Some(p) => match History::update(p, |h| h.batches.pop()) {
                Ok((h, b)) => {
                    self.history = h;
                    b
                }
                Err(e) => {
                    self.notice = Some(Notice {
                        text: format!("{}: {e}", tr(self.lang(), "msg_history_not_saved")),
                        bad: true,
                    });
                    return;
                }
            },
            None => self.history.batches.pop(),
        };
        let Some(batch) = batch else { return };
        self.busy = Some(Busy::Undoing);
        let n = self.notify.clone();
        std::thread::spawn(move || {
            let errors = ops::undo(&batch);
            n.send(Msg::Undone { batch, errors });
        });
    }

    /// Le liste seguono i file spostati dal lotto (o rimessi a posto dall'annulla).
    fn follow_batch(&mut self, batch: &Batch, forward: bool) {
        for f in self.files.iter_mut() {
            if let Some(p) = ops::path_after(batch, f, forward) {
                *f = p;
            }
        }
        for r in self.sub_rows.iter_mut() {
            if let Some(p) = ops::path_after(batch, &r.path, forward) {
                r.path = p;
            }
        }
        let moved: Vec<(PathBuf, PathBuf)> = self
            .overrides
            .keys()
            .filter_map(|k| ops::path_after(batch, k, forward).map(|n| (k.clone(), n)))
            .collect();
        for (old, new) in moved {
            if let Some(v) = self.overrides.remove(&old) {
                self.overrides.insert(new, v);
            }
        }
        if !forward {
            for e in batch.entries.iter().filter(|e| e.action == Action::Move) {
                self.add_path(&e.from);
            }
        }
        self.files.retain(|f| f.exists());
        self.overrides.retain(|k, _| k.exists());
        self.sub_rows.retain(|r| r.path.exists());
        self.plan_key.clear();
    }

    fn errors_text(&self, errors: &[(PathBuf, Error)]) -> String {
        match errors.first() {
            None => String::new(),
            Some((p, e)) => format!(
                ", {} {} ({}: {})",
                errors.len(),
                tr(self.lang(), "msg_errors"),
                p.display(),
                e.localized(self.lang())
            ),
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            let l = self.lang();
            match msg {
                Msg::Found { key, res } => {
                    if let Some(g) = self.groups.iter_mut().find(|g| g.spec.key == key) {
                        match res {
                            Ok((c, best, eps)) => {
                                g.candidates = c;
                                g.selected = best;
                                g.episodes = eps;
                                g.state = GroupState::Ready;
                            }
                            Err(e) => {
                                if is_setup_error(&e) {
                                    self.notice = Some(Notice {
                                        text: e.localized(l),
                                        bad: true,
                                    });
                                }
                                g.state = GroupState::Failed(e);
                            }
                        }
                    }
                    self.revision += 1;
                }
                Msg::Episodes { key, idx, res } => {
                    if let Some(g) = self
                        .groups
                        .iter_mut()
                        .find(|g| g.spec.key == key && g.selected == idx)
                    {
                        match res {
                            Ok(eps) => {
                                g.episodes = eps;
                                g.state = GroupState::Ready;
                            }
                            Err(e) => g.state = GroupState::Failed(e),
                        }
                    }
                    self.revision += 1;
                }
                Msg::SubFound { path, res } => {
                    if let Some(r) = self.sub_rows.iter_mut().find(|r| r.path == path) {
                        match res {
                            Ok(c) if c.is_empty() => r.state = SubState::NotFound,
                            Ok(c) => {
                                r.candidates = c;
                                r.selected = 0;
                                r.state = SubState::Found;
                            }
                            Err(e) => {
                                if is_setup_error(&e) {
                                    self.notice = Some(Notice {
                                        text: e.localized(l),
                                        bad: true,
                                    });
                                }
                                r.state = SubState::Failed(e);
                            }
                        }
                    }
                }
                Msg::SubDone { path, res } => {
                    if let Some(r) = self.sub_rows.iter_mut().find(|r| r.path == path) {
                        r.state = match res {
                            Ok(p) => SubState::Saved(p),
                            Err(e) => {
                                if is_setup_error(&e) {
                                    self.notice = Some(Notice {
                                        text: e.localized(l),
                                        bad: true,
                                    });
                                }
                                SubState::Failed(e)
                            }
                        };
                    }
                }
                Msg::Progress(done, total) => self.busy = Some(Busy::Applying(done, total)),
                Msg::Applied { report, saved } => {
                    self.busy = None;
                    let mut msg = format!(
                        "{} {}{}",
                        report.done,
                        tr(l, "msg_done"),
                        self.errors_text(&report.errors)
                    );
                    let bad = !report.errors.is_empty() || saved.is_err();
                    if let Err(e) = saved {
                        msg.push_str(&format!(" — {}: {e}", tr(l, "msg_history_not_saved")));
                    }
                    self.notice = Some(Notice { text: msg, bad });
                    self.follow_batch(&report.batch, true);
                    match &self.history_path {
                        Some(p) => self.history = History::load(p),
                        None => self.history.upsert(report.batch),
                    }
                }
                Msg::Undone { batch, errors } => {
                    self.busy = None;
                    self.notice = Some(Notice {
                        text: format!("{}{}", tr(l, "msg_undone"), self.errors_text(&errors)),
                        bad: !errors.is_empty(),
                    });
                    self.follow_batch(&batch, false);
                }
            }
        }
    }

    /// Ricomincia da capo con altri file: svuota l'elenco e tutto ciò che dipende da quei file.
    /// Formati, maschera e opzioni restano, perché sono scelte e non dati dei file.
    fn clear_all(&mut self) {
        self.files.clear();
        self.overrides.clear();
        self.sub_rows.clear();
        self.groups.clear();
        self.plan.clear();
        self.editing = None;
        self.notice = None;
        self.plan_key.clear();
    }

    /// Dopo aver aggiunto file: con i dati online attivi la ricerca riparte (la cache la rende leggera).
    fn after_add(&mut self) {
        if self.online && !self.files.is_empty() {
            self.start_search();
        }
    }

    fn pick_files(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Video", VIDEO_EXT)
            .pick_files()
        {
            for p in paths {
                self.add_path(&p);
            }
            self.after_add();
        }
    }

    fn pick_folder(&mut self) {
        if let Some(p) = rfd::FileDialog::new().pick_folder() {
            self.add_path(&p);
            self.after_add();
        }
    }

    fn accept_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() || self.busy.is_some() {
            return;
        }
        for p in dropped {
            self.add_path(&p);
        }
        self.after_add();
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let (open, esc) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if open && self.busy.is_none() {
            self.pick_files();
        }
        if esc {
            self.show_options = false;
            self.save_as = None;
            self.editing = None;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        let idle = self.busy.is_none();
        ui.horizontal(|ui| {
            ui.add_enabled_ui(idle, |ui| {
                if ui::pill(ui, tr(l, "add_files"), Kind::Soft, 15.0)
                    .on_hover_text(tr(l, "tip_add_files"))
                    .clicked()
                {
                    self.pick_files();
                }
                if ui::pill(ui, tr(l, "add_folder"), Kind::Soft, 15.0)
                    .on_hover_text(tr(l, "tip_add_folder"))
                    .clicked()
                {
                    self.pick_folder();
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let kind = if self.show_options {
                    Kind::Selected
                } else {
                    Kind::Soft
                };
                if ui::pill(ui, tr(l, "options"), kind, 15.0).clicked() {
                    self.show_options = !self.show_options;
                }
            });
        });
    }

    /// Le schede e, a destra, quanti file ci sono e come svuotare l'elenco.
    fn tabs_row(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui.horizontal(|ui| {
            for (view, key) in [(View::Rename, "tab_rename"), (View::Subs, "tab_subs")] {
                let kind = if self.view == view {
                    Kind::Selected
                } else {
                    Kind::Soft
                };
                if ui::button(ui, tr(l, key), kind).clicked() {
                    self.view = view;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.files.is_empty() {
                    ui.add_enabled_ui(self.busy.is_none(), |ui| {
                        if ui::button(ui, tr(l, "clear"), Kind::Soft).clicked() {
                            self.clear_all();
                        }
                    });
                    ui.label(muted(format!(
                        "{} {}",
                        self.files.len(),
                        tr(l, "files_count")
                    )));
                }
            });
        });
    }

    fn empty_card(&mut self, ui: &mut egui::Ui, hovering: bool) {
        let l = self.lang();
        let drop = ui::drop_works();
        ui.add_space((ui.available_height() * 0.12).max(0.0));
        let outline = if hovering {
            Stroke::new(1.5, ui::ACCENT)
        } else {
            Stroke::new(1.0, ui::LINE)
        };
        ui::card(ui, outline, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(18.0);
                ui::empty_icon(ui, hovering);
                ui.add_space(8.0);
                let (title, body) = if drop {
                    ("drop_title", "drop_body")
                } else {
                    ("open_title", "open_body")
                };
                let title = if hovering {
                    tr(l, "drop_release")
                } else {
                    tr(l, title)
                };
                ui.label(RichText::new(title).size(19.0).strong());
                ui.label(muted(tr(l, body)));
                ui.add_space(14.0);
                // Due pulsanti centrati: la loro larghezza decide il margine a sinistra.
                let w = 300.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(w, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        if ui::pill(ui, tr(l, "add_files"), Kind::Primary, 15.0).clicked() {
                            self.pick_files();
                        }
                        if ui::pill(ui, tr(l, "add_folder"), Kind::Soft, 15.0).clicked() {
                            self.pick_folder();
                        }
                    },
                );
                ui.add_space(18.0);
            });
        });
    }

    fn reading_card(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui::card(ui, Stroke::new(1.0, ui::LINE), |ui| {
            ui::heading(ui, tr(l, "card_reading"));
            ui.add_space(4.0);
            ui.label(muted(tr(l, "mask")));
            ui.add(
                egui::TextEdit::singleline(&mut self.config.mask)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(tr(l, "mask_hint"))
                    .desired_width(f32::INFINITY),
            );
            if let Some(e) = &self.mask_error {
                ui.label(RichText::new(e).size(13.0).color(ui::DANGER));
            }
            ui.add(egui::Label::new(muted(tr(l, "mask_help"))).wrap());
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            if ui
                .checkbox(&mut self.online, RichText::new(tr(l, "online")).strong())
                .changed()
            {
                self.revision += 1;
                self.after_add();
            }
            ui.add(egui::Label::new(muted(tr(l, "online_help"))).wrap());
            if !self.online {
                return;
            }
            ui.add_space(4.0);
            ui.add_enabled_ui(!self.files.is_empty(), |ui| {
                if ui::button(ui, tr(l, "search_again"), Kind::Soft).clicked() {
                    self.start_search();
                }
            });
            // Un errore che vale per tutti (la chiave) si scrive una volta sola, non sotto ogni titolo.
            let setup = self.groups.iter().find_map(|g| match &g.state {
                GroupState::Failed(e) if is_setup_error(e) => Some(e.localized(l)),
                _ => None,
            });
            if let Some(e) = &setup {
                ui.label(RichText::new(e).size(13.0).color(ui::DANGER));
            }
            let mut pick: Option<(usize, usize)> = None;
            for (gi, g) in self.groups.iter().enumerate() {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if g.state == GroupState::Loading {
                        ui.add(egui::Spinner::new().size(14.0).color(ui::MUTED));
                    }
                    ui.add(egui::Label::new(RichText::new(&g.spec.query).strong()).truncate());
                });
                if let GroupState::Failed(e) = &g.state
                    && !is_setup_error(e)
                {
                    ui.label(RichText::new(e.localized(l)).size(13.0).color(ui::DANGER));
                }
                if !g.candidates.is_empty() {
                    let mut sel = g.selected;
                    egui::ComboBox::from_id_salt(("cand", gi))
                        .width(ui.available_width())
                        .selected_text(g.candidates[g.selected].label())
                        .show_ui(ui, |ui| {
                            for (i, c) in g.candidates.iter().enumerate() {
                                ui.selectable_value(&mut sel, i, c.label());
                            }
                        });
                    if sel != g.selected {
                        pick = Some((gi, sel));
                    }
                }
            }
            if let Some((gi, sel)) = pick {
                self.select_candidate(gi, sel);
            }
        });
    }

    fn format_card(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        let presets = self.presets();
        let builtin = builtin_presets().len();
        let current = presets
            .iter()
            .position(|p| p.episode == self.config.fmt_episode && p.movie == self.config.fmt_movie);
        ui::card(ui, Stroke::new(1.0, ui::LINE), |ui| {
            ui::heading(ui, tr(l, "card_format"));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(muted(tr(l, "preset")));
                let mut pick = current;
                egui::ComboBox::from_id_salt("preset")
                    .width(150.0)
                    .selected_text(
                        current
                            .map(|i| presets[i].name.as_str())
                            .unwrap_or(tr(l, "preset_custom")),
                    )
                    .show_ui(ui, |ui| {
                        for (i, p) in presets.iter().enumerate() {
                            ui.selectable_value(&mut pick, Some(i), p.name.as_str());
                        }
                    });
                if pick != current
                    && let Some(i) = pick
                {
                    self.config.fmt_episode = presets[i].episode.clone();
                    self.config.fmt_movie = presets[i].movie.clone();
                }
                if ui::button(ui, tr(l, "preset_save_as"), Kind::Soft).clicked() {
                    let name = current
                        .filter(|&i| i >= builtin)
                        .map(|i| presets[i].name.clone());
                    self.save_as = Some(name.unwrap_or_default());
                }
                let user = current.filter(|&i| i >= builtin);
                if let Some(i) = user
                    && ui::button(ui, tr(l, "preset_delete"), Kind::Ghost).clicked()
                {
                    self.config.presets.remove(i - builtin);
                    self.save_config();
                }
            });
            let mut save = None;
            let mut cancel = false;
            if let Some(name) = &mut self.save_as {
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text(tr(l, "preset_name"))
                            .desired_width(170.0),
                    );
                    r.request_focus();
                    let ok = !name.trim().is_empty();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.add_enabled_ui(ok, |ui| {
                        if ui::button(ui, tr(l, "save"), Kind::Primary).clicked() || (enter && ok) {
                            save = Some(name.trim().to_string());
                        }
                    });
                    if ui::close(ui).clicked() {
                        cancel = true;
                    }
                });
            }
            if let Some(name) = save {
                self.config.upsert_preset(Preset {
                    name,
                    episode: self.config.fmt_episode.clone(),
                    movie: self.config.fmt_movie.clone(),
                });
                self.save_as = None;
                self.save_config();
            }
            if cancel {
                self.save_as = None;
            }
            ui.add_space(4.0);
            ui.label(muted(tr(l, "fmt_episode")));
            ui.add(
                egui::TextEdit::multiline(&mut self.config.fmt_episode)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.label(muted(tr(l, "fmt_movie")));
            ui.add(
                egui::TextEdit::multiline(&mut self.config.fmt_movie)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            if let Some(e) = &self.format_error {
                ui.label(
                    RichText::new(format!("{}: {e}", tr(l, "errors")))
                        .size(13.0)
                        .color(ui::DANGER),
                );
            }
            ui.add_space(4.0);
            egui::CollapsingHeader::new(muted(tr(l, "placeholders")))
                .id_salt("placeholders")
                .show(ui, |ui| {
                    egui::Grid::new("ph")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            for (code, key) in [
                                ("{n}", "ph_n"),
                                ("{y}", "ph_y"),
                                ("{s} {e}", "ph_se"),
                                ("{s00} {e00}", "ph_se00"),
                                ("{s00e00}", "ph_s00e00"),
                                ("{sxe}", "ph_sxe"),
                                ("{t}", "ph_t"),
                                ("/", "ph_slash"),
                                ("|upper |lower |title", "ph_case"),
                                ("|default:testo", "ph_default"),
                                ("|prefix:… |suffix:…", "ph_affix"),
                            ] {
                                ui.label(RichText::new(code).monospace().color(ui::ACCENT_TEXT));
                                ui.label(muted(tr(l, key)));
                                ui.end_row();
                            }
                        });
                });
        });
    }

    /// Testo, colore, suggerimento completo e, per gli stati da rivedere, il rimedio in breve.
    fn status_look(
        &self,
        s: &Status,
    ) -> (String, egui::Color32, &'static str, Option<&'static str>) {
        let l = self.lang();
        match s {
            Status::Ok => (tr(l, "st_ok").into(), ui::SUCCESS, "tip_ok", None),
            Status::Unchanged => (tr(l, "st_same").into(), ui::MUTED, "tip_same", None),
            Status::Exists => (
                tr(l, "st_exists").into(),
                ui::WARN,
                "tip_exists",
                Some("fix_exists"),
            ),
            Status::Duplicate => (tr(l, "st_dup").into(), ui::WARN, "tip_dup", Some("fix_dup")),
            Status::NoMatch => (
                tr(l, "st_nomatch").into(),
                ui::DANGER,
                "tip_nomatch",
                Some("fix_nomatch"),
            ),
            Status::Missing(f) => {
                let fields: Vec<String> = f.iter().map(|x| format!("{{{x}}}")).collect();
                (
                    format!("{} {}", tr(l, "st_missing"), fields.join(" ")),
                    ui::DANGER,
                    "tip_missing",
                    Some("fix_missing"),
                )
            }
        }
    }

    /// Apre il riquadro di correzione con i dati che il file ha adesso.
    fn open_editor(&mut self, path: PathBuf) {
        let mask = self.compile_mask().ok().flatten();
        let info = Self::resolve_info(
            mask.as_ref(),
            self.online,
            &self.groups,
            &self.overrides,
            &path,
        )
        .or_else(|| info_from_name(None, &path))
        .unwrap_or_default();
        self.editing = Some(Editing {
            fields: Fields::from_info(&info),
            path,
        });
    }

    /// Riquadro per correggere a mano i dati di un file, con il nome che ne risulterà.
    fn edit_modal(&mut self, ctx: &egui::Context) {
        let l = self.lang();
        let Some(ed) = &self.editing else { return };
        let path = ed.path.clone();
        let parsed = ed.fields.parse();
        let had_override = self.overrides.contains_key(&path);
        // Il nome che risulterà, con le stesse regole dell'anteprima.
        let preview = parsed.as_ref().ok().and_then(|info| {
            let e = Template::parse(&self.config.fmt_episode).ok()?;
            let m = Template::parse(&self.config.fmt_movie).ok()?;
            let dest = self.config.dest.trim();
            let dest = (!dest.is_empty()).then(|| PathBuf::from(dest));
            let item = build_plan(
                std::slice::from_ref(&path),
                &Formats {
                    episode: &e,
                    movie: &m,
                },
                dest.as_deref(),
                &|_| Some(info.clone()),
            )
            .pop()?;
            let shown = item.target.as_ref().map(|t| {
                let base = item.source.parent();
                base.and_then(|b| t.strip_prefix(b).ok())
                    .unwrap_or(t)
                    .display()
                    .to_string()
            });
            Some((shown, item.status))
        });
        let preview_look = preview.as_ref().map(|(_, st)| self.status_look(st));
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        enum Choice {
            Apply(MediaInfo),
            Reset,
            Cancel,
        }
        let mut choice = None;
        let ed = self.editing.as_mut().expect("controllato sopra");
        let resp = egui::Modal::new(egui::Id::NULL.with("edit"))
            .frame(
                egui::Frame::new()
                    .fill(ui::CARD)
                    .stroke(Stroke::new(1.0, ui::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.set_width(540.0);
                ui::heading(ui, tr(l, "edit_title"));
                ui.add(egui::Label::new(muted(&name)).truncate());
                ui.add(egui::Label::new(muted(tr(l, "edit_help")).size(12.0)).wrap());
                ui.add_space(8.0);
                egui::Grid::new("edit_grid")
                    .num_columns(2)
                    .min_col_width(150.0)
                    .spacing([18.0, 10.0])
                    .show(ui, |ui| {
                        for (key, field, width) in [
                            ("f_n", &mut ed.fields.n, 340.0),
                            ("f_y", &mut ed.fields.y, 90.0),
                            ("f_s", &mut ed.fields.s, 90.0),
                            ("f_e", &mut ed.fields.e, 90.0),
                            ("f_t", &mut ed.fields.t, 340.0),
                        ] {
                            ui.label(muted(tr(l, key)));
                            ui.add(egui::TextEdit::singleline(field).desired_width(width));
                            ui.end_row();
                        }
                    });
                if let Err(bad) = &parsed {
                    let key = match *bad {
                        "y" => "f_y",
                        "s" => "f_s",
                        _ => "f_e",
                    };
                    ui.label(
                        RichText::new(format!("{}: {}", tr(l, key), tr(l, "f_invalid")))
                            .size(13.0)
                            .color(ui::DANGER),
                    );
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.label(muted(tr(l, "f_result")));
                if let (Some((shown, _)), Some((text, color, tip, _))) = (&preview, &preview_look) {
                    ui.horizontal(|ui| {
                        ui::chip(ui, text, *color).on_hover_text(tr(l, tip));
                        ui.add(
                            egui::Label::new(
                                RichText::new(shown.clone().unwrap_or_else(|| "—".into()))
                                    .size(14.0),
                            )
                            .truncate(),
                        );
                    });
                }
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(parsed.is_ok(), |ui| {
                        if ui::button(ui, tr(l, "apply_edit"), Kind::Primary).clicked()
                            && let Ok(info) = &parsed
                        {
                            choice = Some(Choice::Apply(info.clone()));
                        }
                    });
                    if ui::button(ui, tr(l, "cancel"), Kind::Soft).clicked() {
                        choice = Some(Choice::Cancel);
                    }
                    if had_override && ui::button(ui, tr(l, "reset"), Kind::Ghost).clicked() {
                        choice = Some(Choice::Reset);
                    }
                });
            });
        if resp.should_close() && choice.is_none() {
            choice = Some(Choice::Cancel);
        }
        match choice {
            Some(Choice::Apply(info)) => {
                self.overrides.insert(path, info);
                self.editing = None;
                self.revision += 1;
            }
            Some(Choice::Reset) => {
                self.overrides.remove(&path);
                self.editing = None;
                self.revision += 1;
            }
            Some(Choice::Cancel) => self.editing = None,
            None => {}
        }
    }

    /// L'anteprima riempie lo spazio che resta; le righe fuori vista non si disegnano.
    fn preview_card(&mut self, ui: &mut egui::Ui) {
        const ROW: f32 = 50.0;
        let l = self.lang();
        let ready = self.plan.iter().filter(|i| i.status == Status::Ok).count();
        let same = self
            .plan
            .iter()
            .filter(|i| i.status == Status::Unchanged)
            .count();
        let review = self.plan.len() - ready - same;
        let mut remove: Option<PathBuf> = None;
        let mut edit: Option<PathBuf> = None;
        egui::Frame::new()
            .fill(ui::CARD)
            .stroke(Stroke::new(1.0, ui::LINE))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui::heading(ui, tr(l, "card_preview"));
                    ui.add_space(6.0);
                    ui::chip(ui, &format!("{ready} {}", tr(l, "sum_ready")), ui::SUCCESS);
                    if review > 0 {
                        ui::chip(ui, &format!("{review} {}", tr(l, "sum_review")), ui::WARN);
                    }
                    if same > 0 {
                        ui::chip(ui, &format!("{same} {}", tr(l, "sum_same")), ui::MUTED);
                    }
                });
                ui.add_space(6.0);
                let width = ui.available_width();
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW, self.plan.len(), |ui, range| {
                        for i in range {
                            let item = &self.plan[i];
                            let (rect, resp) = ui
                                .allocate_exact_size(egui::vec2(width, ROW), egui::Sense::hover());
                            if i > 0 {
                                ui.painter().hline(
                                    rect.x_range(),
                                    rect.top(),
                                    Stroke::new(1.0, ui::LINE),
                                );
                            }
                            if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink(1.0),
                                    egui::CornerRadius::same(8),
                                    ui::PANEL,
                                );
                            }
                            let mut row = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(rect.shrink2(egui::vec2(8.0, 4.0)))
                                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            );
                            if ui::close(&mut row).on_hover_text(tr(l, "remove")).clicked() {
                                remove = Some(item.source.clone());
                            }
                            let manual = self.overrides.contains_key(&item.source);
                            if ui::icon_button(&mut row, ui::Icon::Pencil, manual, 26.0)
                                .on_hover_text(tr(l, "edit"))
                                .clicked()
                            {
                                edit = Some(item.source.clone());
                            }
                            let (text, color, tip, fix) = self.status_look(&item.status);
                            ui::chip(&mut row, &text, color).on_hover_text(tr(l, tip));
                            row.add_space(8.0);
                            row.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                let old = item
                                    .source
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                // Senza nome nuovo la prima riga mostra quello attuale, così si sa di che file si parla.
                                let (first, old_shown) = match &item.target {
                                    Some(t) => {
                                        let base = item.source.parent();
                                        let shown =
                                            base.and_then(|b| t.strip_prefix(b).ok()).unwrap_or(t);
                                        (shown.display().to_string(), true)
                                    }
                                    None => (old.clone(), false),
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(first).size(14.0).color(ui::TEXT),
                                    )
                                    .truncate(),
                                );
                                // La seconda riga dice da dove viene e, se serve, come rimediare: senza dover passare col mouse.
                                let mut job = egui::text::LayoutJob::default();
                                let small = |c: egui::Color32| egui::TextFormat {
                                    font_id: egui::FontId::proportional(12.5),
                                    color: c,
                                    ..Default::default()
                                };
                                let mut parts: Vec<(String, egui::Color32)> = Vec::new();
                                if old_shown {
                                    parts.push((format!("{} {old}", tr(l, "from")), ui::MUTED));
                                }
                                if let Some(fix) = fix {
                                    parts.push((tr(l, fix).to_string(), color));
                                }
                                if manual {
                                    parts.push((tr(l, "manual").to_string(), ui::ACCENT_TEXT));
                                }
                                for (k, (t, c)) in parts.into_iter().enumerate() {
                                    if k > 0 {
                                        job.append(" · ", 0.0, small(ui::MUTED));
                                    }
                                    job.append(&t, 0.0, small(c));
                                }
                                ui.add(egui::Label::new(job).truncate());
                            });
                        }
                    });
            });
        if let Some(p) = edit {
            self.open_editor(p);
        }
        if let Some(p) = remove {
            self.overrides.remove(&p);
            self.files.retain(|f| *f != p);
            self.sub_rows.retain(|r| r.path != p);
            self.plan_key.clear();
        }
    }

    fn rename_page(&mut self, ui: &mut egui::Ui) {
        const SIDE: f32 = 360.0;
        if ui.available_width() >= 900.0 {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let h = ui.available_height();
                ui.allocate_ui_with_layout(
                    egui::vec2(SIDE, h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(SIDE);
                        egui::ScrollArea::vertical()
                            .id_salt("left")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.reading_card(ui);
                                ui.add_space(12.0);
                                self.format_card(ui);
                            });
                    },
                );
                let w = ui.available_width();
                ui.allocate_ui_with_layout(
                    egui::vec2(w, h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.preview_card(ui);
                    },
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("narrow")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.reading_card(ui);
                    ui.add_space(12.0);
                    self.format_card(ui);
                    ui.add_space(12.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), 420.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| self.preview_card(ui),
                    );
                });
        }
    }

    fn subs_page(&mut self, ui: &mut egui::Ui) {
        const ROW: f32 = 72.0;
        let l = self.lang();
        if self.sub_rows.is_empty() {
            ui::card(ui, Stroke::new(1.0, ui::LINE), |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(tr(l, "subs_hint_title")).size(17.0).strong());
                    ui.add(egui::Label::new(muted(tr(l, "subs_hint"))).wrap());
                    ui.add_space(12.0);
                });
            });
            return;
        }
        let found = self
            .sub_rows
            .iter()
            .filter(|r| !r.candidates.is_empty())
            .count();
        let none = self
            .sub_rows
            .iter()
            .filter(|r| r.state == SubState::NotFound)
            .count();
        egui::Frame::new()
            .fill(ui::CARD)
            .stroke(Stroke::new(1.0, ui::LINE))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    ui::heading(ui, tr(l, "card_subs"));
                    ui.add_space(6.0);
                    ui::chip(ui, &format!("{found} {}", tr(l, "sum_found")), ui::SUCCESS);
                    if none > 0 {
                        ui::chip(ui, &format!("{none} {}", tr(l, "sum_none")), ui::MUTED);
                    }
                });
                ui.add_space(6.0);
                let width = ui.available_width();
                ui.spacing_mut().item_spacing.y = 0.0;
                let rows = self.sub_rows.len();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW, rows, |ui, range| {
                        for i in range {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(width, ROW), egui::Sense::hover());
                            if i > 0 {
                                ui.painter().hline(
                                    rect.x_range(),
                                    rect.top(),
                                    Stroke::new(1.0, ui::LINE),
                                );
                            }
                            let mut cell = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(rect.shrink2(egui::vec2(8.0, 6.0)))
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            cell.spacing_mut().item_spacing.y = 4.0;
                            let r = &mut self.sub_rows[i];
                            let (text, color) = match &r.state {
                                SubState::Searching => (tr(l, "searching").to_string(), ui::MUTED),
                                SubState::Found => (tr(l, "sub_found").to_string(), ui::SUCCESS),
                                SubState::NotFound => (tr(l, "sub_none").to_string(), ui::MUTED),
                                SubState::Downloading => {
                                    (tr(l, "sub_downloading").to_string(), ui::ACCENT_TEXT)
                                }
                                SubState::Saved(_) => (tr(l, "sub_saved").to_string(), ui::SUCCESS),
                                SubState::Failed(_) => (tr(l, "sub_error").to_string(), ui::DANGER),
                            };
                            let name = r
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            cell.horizontal(|ui| {
                                ui.set_min_height(28.0);
                                // La casella c'è solo dove c'è qualcosa da scaricare.
                                if r.candidates.is_empty() {
                                    ui.add_space(26.0);
                                } else {
                                    ui.checkbox(&mut r.include, "");
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui::chip(ui, &text, color);
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(name).size(14.0),
                                                    )
                                                    .truncate(),
                                                );
                                            },
                                        );
                                    },
                                );
                            });
                            cell.horizontal(|ui| {
                                ui.add_space(26.0);
                                match &r.state {
                                    // Un errore che vale per tutti è già scritto una volta sola, sopra la barra in basso.
                                    SubState::Failed(e) if !is_setup_error(e) => {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(e.localized(l))
                                                    .size(13.0)
                                                    .color(ui::DANGER),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    SubState::Saved(p) => {
                                        let saved = p
                                            .file_name()
                                            .map(|n| n.to_string_lossy().into_owned())
                                            .unwrap_or_default();
                                        ui.add(egui::Label::new(muted(saved)).truncate());
                                    }
                                    _ if !r.candidates.is_empty() => {
                                        egui::ComboBox::from_id_salt(("subcand", i))
                                            .width(ui.available_width())
                                            .selected_text(r.candidates[r.selected].label())
                                            .show_ui(ui, |ui| {
                                                for (k, c) in r.candidates.iter().enumerate() {
                                                    ui.selectable_value(
                                                        &mut r.selected,
                                                        k,
                                                        c.label(),
                                                    );
                                                }
                                            });
                                    }
                                    _ => {}
                                }
                            });
                        }
                    });
            });
    }

    fn rename_bar(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        let downloading = self
            .sub_rows
            .iter()
            .any(|r| r.state == SubState::Downloading);
        let idle = self.busy.is_none() && !downloading;
        let ready = self.plan.iter().filter(|i| i.status == Status::Ok).count();
        let next = self.config.dest.trim().is_empty();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(muted(tr(l, "dest")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // L'azione della finestra sta in basso a destra, dove l'occhio arriva per ultimo.
                let verb = if self.action == Action::Move {
                    "do_rename"
                } else {
                    "do_copy"
                };
                let label = if ready > 0 {
                    format!("{} {ready} {}", tr(l, verb), tr(l, "files_count"))
                } else {
                    tr(l, verb).to_string()
                };
                ui.add_enabled_ui(idle && ready > 0, |ui| {
                    if ui::pill(ui, &label, Kind::Primary, 16.0).clicked() {
                        self.start_apply();
                    }
                });
                ui.add_enabled_ui(idle && !self.history.batches.is_empty(), |ui| {
                    if ui::pill(ui, tr(l, "undo"), Kind::Soft, 15.0)
                        .on_hover_text(tr(l, "tip_undo"))
                        .clicked()
                    {
                        self.start_undo();
                    }
                });
                ui.add_space(10.0);
                ui.checkbox(&mut self.with_sidecars, tr(l, "sidecars"))
                    .on_hover_text(tr(l, "tip_sidecars"));
                egui::ComboBox::from_id_salt("action")
                    .width(90.0)
                    .selected_text(tr(
                        l,
                        if self.action == Action::Move {
                            "act_move"
                        } else {
                            "act_copy"
                        },
                    ))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.action, Action::Move, tr(l, "act_move"));
                        ui.selectable_value(&mut self.action, Action::Copy, tr(l, "act_copy"));
                    });
                ui.add_space(10.0);
                let mut beside = next;
                if ui.checkbox(&mut beside, tr(l, "dest_next")).changed() {
                    if beside {
                        self.config.dest.clear();
                    } else if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.config.dest = p.display().to_string();
                    }
                }
                if ui::icon_button(ui, ui::Icon::Folder, false, 34.0)
                    .on_hover_text(tr(l, "dest_pick"))
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.config.dest = p.display().to_string();
                }
                if !next {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&self.config.dest).size(13.0).color(ui::TEXT),
                            )
                            .truncate(),
                        );
                    });
                }
            });
        });
    }

    fn subs_bar(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        let idle = self.busy.is_none();
        let working = self
            .sub_rows
            .iter()
            .any(|r| matches!(r.state, SubState::Searching | SubState::Downloading));
        let to_get = self
            .sub_rows
            .iter()
            .filter(|r| r.include && r.state == SubState::Found)
            .count();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(muted(tr(l, "sub_lang")));
            egui::ComboBox::from_id_salt("sublang")
                .width(200.0)
                .selected_text(match self.sub_lang {
                    Some(i) => LANGS[i].1,
                    None => tr(l, "sub_lang_pick"),
                })
                .show_ui(ui, |ui| {
                    for (i, (_, name)) in LANGS.iter().enumerate() {
                        ui.selectable_value(&mut self.sub_lang, Some(i), *name);
                    }
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if to_get > 0 {
                    format!("{} {to_get} {}", tr(l, "sub_download"), tr(l, "subs_word"))
                } else {
                    tr(l, "sub_download").to_string()
                };
                ui.add_enabled_ui(idle && !working && to_get > 0, |ui| {
                    if ui::pill(ui, &label, Kind::Primary, 16.0).clicked() {
                        self.sub_download();
                    }
                });
                let can_search =
                    idle && !working && self.sub_lang.is_some() && !self.files.is_empty();
                ui.add_enabled_ui(can_search, |ui| {
                    if ui::pill(ui, tr(l, "sub_search"), Kind::Soft, 15.0).clicked() {
                        self.sub_search();
                    }
                });
            });
        });
    }

    /// Lavoro in corso, oppure l'esito dell'ultima operazione.
    fn strip(&mut self, ui: &mut egui::Ui) -> bool {
        let l = self.lang();
        let count =
            |s: &dyn Fn(&SubState) -> bool| self.sub_rows.iter().filter(|r| s(&r.state)).count();
        let searching = count(&|s| *s == SubState::Searching);
        let downloading = count(&|s| *s == SubState::Downloading);
        let (title, frac) = match self.busy {
            Some(Busy::Applying(done, total)) => (
                Some((tr(l, "working"), format!("{done}/{total}"))),
                Some(done as f32 / total.max(1) as f32),
            ),
            Some(Busy::Undoing) => (Some((tr(l, "undoing"), String::new())), None),
            None if searching > 0 => {
                let n = self.sub_rows.len();
                (
                    Some((tr(l, "subs_searching"), format!("{}/{n}", n - searching))),
                    Some((n - searching) as f32 / n as f32),
                )
            }
            None if downloading > 0 => (
                Some((tr(l, "subs_downloading"), format!("{downloading}"))),
                None,
            ),
            None => (None, None),
        };
        if let Some((title, detail)) = title {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(14.0).strong());
                ui.label(muted(detail));
            });
            ui.add_space(4.0);
            ui::bar(ui, frac);
            return true;
        }
        let Some(n) = &self.notice else { return false };
        let mut dismiss = false;
        let mut clear = false;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                dismiss = ui::close(ui).on_hover_text(tr(l, "dismiss")).clicked();
                // Finito un lavoro, il passo dopo è di solito ricominciare con altri file.
                if !n.bad && !self.files.is_empty() {
                    clear = ui::button(ui, tr(l, "clear"), Kind::Soft).clicked();
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (word, color) = if n.bad {
                        ("attention", ui::WARN)
                    } else {
                        ("done", ui::SUCCESS)
                    };
                    ui.label(RichText::new(tr(l, word)).size(14.0).strong().color(color));
                    ui.add(egui::Label::new(muted(&n.text)).truncate());
                });
            });
        });
        if dismiss {
            self.notice = None;
        }
        if clear {
            self.clear_all();
        }
        true
    }

    fn strip_visible(&self) -> bool {
        self.busy.is_some()
            || self.notice.is_some()
            || self
                .sub_rows
                .iter()
                .any(|r| matches!(r.state, SubState::Searching | SubState::Downloading))
    }

    /// Le opzioni galleggiano sopra la finestra, sotto il loro pulsante, così il resto non si sposta.
    fn options_area(&mut self, ctx: &egui::Context) {
        let l = self.lang();
        egui::Area::new(egui::Id::NULL.with("options"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, [-16.0, ui::TITLE_HEIGHT + 62.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui::CARD)
                    .stroke(Stroke::new(1.0, ui::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(Margin::same(18))
                    .shadow(egui::Shadow {
                        offset: [0, 6],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(130),
                    })
                    .show(ui, |ui| {
                        ui.set_width(520.0);
                        ui::heading(ui, tr(l, "options"));
                        ui.add_space(6.0);
                        egui::Grid::new("opts")
                            .num_columns(2)
                            .min_col_width(150.0)
                            .spacing([18.0, 10.0])
                            .show(ui, |ui| {
                                ui.label(muted(tr(l, "ui_lang")));
                                egui::ComboBox::from_id_salt("uilang")
                                    .width(220.0)
                                    .selected_text(self.config.ui_lang.name())
                                    .show_ui(ui, |ui| {
                                        for lang in Lang::ALL {
                                            ui.selectable_value(
                                                &mut self.config.ui_lang,
                                                lang,
                                                lang.name(),
                                            );
                                        }
                                    });
                                ui.end_row();
                                ui.label(muted(tr(l, "provider")));
                                egui::ComboBox::from_id_salt("provider")
                                    .width(220.0)
                                    .selected_text(self.config.provider.name())
                                    .show_ui(ui, |ui| {
                                        for p in Provider::ALL {
                                            ui.selectable_value(
                                                &mut self.config.provider,
                                                p,
                                                p.name(),
                                            );
                                        }
                                    });
                                ui.end_row();
                                if metadata::series_provider(&self.config) != self.config.provider {
                                    ui.label("");
                                    ui.label(muted(tr(l, "provider_fallback")).size(12.0));
                                    ui.end_row();
                                }
                                ui.label(muted(tr(l, "meta_lang")));
                                egui::ComboBox::from_id_salt("metalang")
                                    .width(220.0)
                                    .selected_text(match self.config.meta_lang.as_str() {
                                        "en" => "English",
                                        _ => "Italiano",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.config.meta_lang,
                                            "it".to_string(),
                                            "Italiano",
                                        );
                                        ui.selectable_value(
                                            &mut self.config.meta_lang,
                                            "en".to_string(),
                                            "English",
                                        );
                                    });
                                ui.end_row();
                                ui.label(muted(tr(l, "tmdb_key")));
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.config.tmdb_key)
                                            .password(true)
                                            .desired_width(300.0),
                                    );
                                    ui.label(muted(tr(l, "tmdb_help")).size(12.0));
                                });
                                ui.end_row();
                            });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.label(RichText::new(tr(l, "os_title")).strong());
                        ui.label(muted(tr(l, "os_help")).size(12.0));
                        ui.add_space(4.0);
                        egui::Grid::new("opts_os")
                            .num_columns(2)
                            .min_col_width(150.0)
                            .spacing([18.0, 10.0])
                            .show(ui, |ui| {
                                ui.label(muted(tr(l, "os_key")));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.config.os_api_key)
                                        .password(true)
                                        .desired_width(300.0),
                                );
                                ui.end_row();
                                ui.label(muted(tr(l, "os_user")));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.config.os_username)
                                        .desired_width(300.0),
                                );
                                ui.end_row();
                                ui.label(muted(tr(l, "os_pass")));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.config.os_password)
                                        .password(true)
                                        .desired_width(300.0),
                                );
                                ui.end_row();
                            });
                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(4.0);
                        // Le attribuzioni che TMDB e TVmaze chiedono di mostrare nel programma.
                        ui.label(RichText::new(format!("{} · RenameBot {}", tr(l, "about"), env!("CARGO_PKG_VERSION"))).strong());
                        ui.add(egui::Label::new(muted(tr(l, "about_text")).size(12.0)).wrap());
                        ui.add(
                            egui::Label::new(
                                muted("This product uses the TMDB API but is not endorsed or certified by TMDB.").size(12.0),
                            )
                            .wrap(),
                        );
                        ui.add(egui::Label::new(muted(tr(l, "about_tvmaze")).size(12.0)).wrap());
                        ui.add_space(14.0);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui::button(ui, tr(l, "save"), Kind::Primary).clicked() {
                                self.save_config();
                                self.show_options = false;
                            }
                            if ui::button(ui, tr(l, "close"), Kind::Soft).clicked() {
                                self.show_options = false;
                            }
                        });
                    });
            });
    }
}

/// Errori che valgono per tutti i file (chiave, account, limite): si mostrano una volta sola.
fn is_setup_error(e: &Error) -> bool {
    matches!(
        e,
        Error::TmdbKeyMissing
            | Error::TmdbKeyInvalid
            | Error::OsKeyMissing
            | Error::OsKeyInvalid
            | Error::OsAccountMissing
            | Error::OsLoginFailed
            | Error::OsDailyLimit
            | Error::Http(429)
    )
}

fn is_video(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXT.contains(&e.to_lowercase().as_str()))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.accept_drops(&ctx);
        self.shortcuts(&ctx);
        self.poll();
        self.refresh_plan();
        let l = self.lang();
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::Panel::top("title")
            .exact_size(ui::TITLE_HEIGHT)
            .frame(egui::Frame::new().fill(ui::TITLE))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui::title_bar(
                    ui,
                    "RenameBot",
                    tr(l, "minimize"),
                    tr(l, "maximize"),
                    tr(l, "restore"),
                )
            });

        egui::Panel::top("bar")
            .frame(
                egui::Frame::new()
                    .fill(ui::PANEL)
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ui, |ui| self.top_bar(ui));

        egui::Panel::bottom("actions")
            .frame(
                egui::Frame::new()
                    .fill(ui::PANEL)
                    .inner_margin(Margin::symmetric(16, 10)),
            )
            .show(ui, |ui| match self.view {
                View::Rename => self.rename_bar(ui),
                View::Subs => self.subs_bar(ui),
            });

        if self.strip_visible() {
            egui::Panel::bottom("strip")
                .frame(
                    egui::Frame::new()
                        .fill(ui::CARD)
                        .inner_margin(Margin::symmetric(16, 10)),
                )
                .show(ui, |ui| {
                    self.strip(ui);
                });
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ui::BG)
                    .inner_margin(Margin::same(16)),
            )
            .show(ui, |ui| {
                self.tabs_row(ui);
                ui.add_space(12.0);
                if self.files.is_empty() {
                    self.empty_card(ui, hovering);
                } else if self.view == View::Subs {
                    self.subs_page(ui);
                } else {
                    self.rename_page(ui);
                }
            });

        if self.show_options {
            self.options_area(&ctx);
        }
        self.edit_modal(&ctx);
        ui::border(&ctx);
        ui::resize_edges(&ctx);
    }

    fn on_exit(&mut self) {
        self.save_config();
    }
}
