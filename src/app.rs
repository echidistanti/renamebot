//! Finestra principale: elenco file, maschera, formati, anteprima della rinomina e sottotitoli.
//! Il lavoro lento (rete, spostamenti di file) gira in thread separati che rispondono con `Msg`.

use eframe::egui;
use renamebot::config::{Config, Preset, Provider, builtin_presets};
use renamebot::error::Error;
use renamebot::http::{Cached, Http, Ureq};
use renamebot::i18n::{Lang, tr};
use renamebot::mask::Mask;
use renamebot::metadata::{self, Candidate, Episode, GroupSpec};
use renamebot::ops::{self, Action, Batch, History, Report};
use renamebot::plan::{Formats, PlanItem, Status, build_plan, info_from_name};
use renamebot::subs::{self, Client, Session, SubCandidate};
use renamebot::template::{MediaInfo, Template};
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

enum Busy {
    Applying(usize, usize),
    Undoing,
}

pub struct App {
    config: Config,
    config_path: Option<PathBuf>,
    files: Vec<PathBuf>,
    preset_name: String,
    plan: Vec<PlanItem>,
    plan_key: String,
    error: Option<String>,
    action: Action,
    with_sidecars: bool,
    history: History,
    history_path: Option<PathBuf>,
    message: Option<String>,
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
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
        Self {
            config,
            config_path,
            files: Vec::new(),
            preset_name: String::new(),
            plan: Vec::new(),
            plan_key: String::new(),
            error: None,
            action: Action::Move,
            with_sidecars: true,
            history,
            history_path,
            message: None,
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
        }
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
            self.message = Some(format!(
                "{}: {e}",
                tr(self.lang(), "msg_settings_not_saved")
            ));
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
        path: &Path,
    ) -> Option<MediaInfo> {
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
        self.error = None;

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
                    &|p| Self::resolve_info(mask.as_ref(), online, groups, p),
                );
            }
            (e, m, k) => {
                self.error = e.err().or(m.err()).or(k.err());
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
            let info = Self::resolve_info(mask.as_ref(), self.online, &self.groups, f);
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
                    self.message =
                        Some(format!("{}: {e}", tr(self.lang(), "msg_history_not_saved")));
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
        if !forward {
            for e in batch.entries.iter().filter(|e| e.action == Action::Move) {
                self.add_path(&e.from);
            }
        }
        self.files.retain(|f| f.exists());
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
                            Err(e) => g.state = GroupState::Failed(e),
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
                            Err(e) => r.state = SubState::Failed(e),
                        }
                    }
                }
                Msg::SubDone { path, res } => {
                    if let Some(r) = self.sub_rows.iter_mut().find(|r| r.path == path) {
                        r.state = match res {
                            Ok(p) => SubState::Saved(p),
                            Err(e) => SubState::Failed(e),
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
                    if let Err(e) = saved {
                        msg.push_str(&format!(" — {}: {e}", tr(l, "msg_history_not_saved")));
                    }
                    self.message = Some(msg);
                    self.follow_batch(&report.batch, true);
                    match &self.history_path {
                        Some(p) => self.history = History::load(p),
                        None => self.history.upsert(report.batch),
                    }
                }
                Msg::Undone { batch, errors } => {
                    self.busy = None;
                    self.message = Some(format!(
                        "{}{}",
                        tr(l, "msg_undone"),
                        self.errors_text(&errors)
                    ));
                    self.follow_batch(&batch, false);
                }
            }
        }
    }

    fn status_text(&self, s: &Status) -> String {
        let l = self.lang();
        match s {
            Status::Ok => tr(l, "st_ok").into(),
            Status::Unchanged => tr(l, "st_same").into(),
            Status::Exists => tr(l, "st_exists").into(),
            Status::Duplicate => tr(l, "st_dup").into(),
            Status::NoMatch => tr(l, "st_nomatch").into(),
            Status::Missing(f) => format!("{}: {}", tr(l, "st_missing"), f.join(", ")),
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.view, View::Rename, tr(l, "tab_rename"));
            ui.selectable_value(&mut self.view, View::Subs, tr(l, "tab_subs"));
            ui.separator();
            let idle = self.busy.is_none();
            if ui
                .add_enabled(idle, egui::Button::new(tr(l, "add_files")))
                .clicked()
                && let Some(paths) = rfd::FileDialog::new().pick_files()
            {
                for p in paths {
                    self.add_path(&p);
                }
            }
            if ui
                .add_enabled(idle, egui::Button::new(tr(l, "add_folder")))
                .clicked()
                && let Some(p) = rfd::FileDialog::new().pick_folder()
            {
                self.add_path(&p);
            }
            if ui
                .add_enabled(idle, egui::Button::new(tr(l, "clear")))
                .clicked()
            {
                self.files.clear();
            }
            ui.label(format!("{} {}", self.files.len(), tr(l, "files_count")));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(self.config.ui_lang.name())
                    .show_ui(ui, |ui| {
                        for lang in Lang::ALL {
                            ui.selectable_value(&mut self.config.ui_lang, lang, lang.name());
                        }
                    });
                ui.label(tr(l, "language"));
            });
        });
    }

    fn formats_section(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui.label(tr(l, "mask"));
        ui.add(
            egui::TextEdit::singleline(&mut self.config.mask)
                .hint_text(tr(l, "mask_hint"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(10.0);

        let presets = self.presets();
        let builtin = builtin_presets().len();
        let current = presets
            .iter()
            .position(|p| p.episode == self.config.fmt_episode && p.movie == self.config.fmt_movie);
        ui.label(tr(l, "preset"));
        let mut pick = current;
        egui::ComboBox::from_id_salt("preset")
            .selected_text(current.map(|i| presets[i].name.as_str()).unwrap_or("—"))
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
            if i >= builtin {
                self.preset_name = presets[i].name.clone();
            }
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_name)
                    .hint_text(tr(l, "preset_name"))
                    .desired_width(150.0),
            );
            let name = self.preset_name.trim().to_string();
            if ui
                .add_enabled(!name.is_empty(), egui::Button::new(tr(l, "preset_save")))
                .clicked()
            {
                self.config.upsert_preset(Preset {
                    name,
                    episode: self.config.fmt_episode.clone(),
                    movie: self.config.fmt_movie.clone(),
                });
                self.save_config();
            }
            let user = current.filter(|&i| i >= builtin);
            if ui
                .add_enabled(user.is_some(), egui::Button::new(tr(l, "preset_delete")))
                .clicked()
                && let Some(i) = user
            {
                self.config.presets.remove(i - builtin);
                self.save_config();
            }
        });
        ui.add_space(6.0);

        ui.label(tr(l, "fmt_episode"));
        ui.add(
            egui::TextEdit::multiline(&mut self.config.fmt_episode)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.label(tr(l, "fmt_movie"));
        ui.add(
            egui::TextEdit::multiline(&mut self.config.fmt_movie)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        ui.label(tr(l, "dest"));
        ui.add(
            egui::TextEdit::singleline(&mut self.config.dest)
                .hint_text(tr(l, "dest_hint"))
                .desired_width(f32::INFINITY),
        );
    }

    fn action_section(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui.label(tr(l, "action"));
        ui.radio_value(&mut self.action, Action::Move, tr(l, "act_move"));
        ui.radio_value(&mut self.action, Action::Copy, tr(l, "act_copy"));
        ui.checkbox(&mut self.with_sidecars, tr(l, "sidecars"));
        ui.add_space(8.0);
        // Durante un download di sottotitoli i percorsi non devono cambiare.
        let downloading = self
            .sub_rows
            .iter()
            .any(|r| r.state == SubState::Downloading);
        let idle = self.busy.is_none() && !downloading;
        let ready = idle && self.plan.iter().any(|i| i.status == Status::Ok);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new(tr(l, "apply")))
                .clicked()
            {
                self.start_apply();
            }
            let can_undo = idle && !self.history.batches.is_empty();
            if ui
                .add_enabled(can_undo, egui::Button::new(tr(l, "undo")))
                .clicked()
            {
                self.start_undo();
            }
        });
        match self.busy {
            Some(Busy::Applying(done, total)) => {
                ui.add(
                    egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                        .text(format!("{} {done}/{total}", tr(l, "working"))),
                );
            }
            Some(Busy::Undoing) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr(l, "undoing"));
                });
            }
            None => {}
        }
        if let Some(m) = &self.message {
            ui.add_space(6.0);
            ui.label(m.as_str());
        }
        if let Some(err) = &self.error {
            ui.add_space(8.0);
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("{}: {err}", tr(l, "errors")),
            );
        }
    }

    fn online_section(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui.checkbox(&mut self.online, tr(l, "online"));
        if !self.online {
            return;
        }
        if ui
            .add_enabled(!self.files.is_empty(), egui::Button::new(tr(l, "search")))
            .clicked()
        {
            self.start_search();
        }
        let mut pick: Option<(usize, usize)> = None;
        for (gi, g) in self.groups.iter().enumerate() {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(&g.spec.query).strong());
            match &g.state {
                GroupState::Loading => {
                    ui.label(tr(l, "searching"));
                }
                GroupState::Failed(e) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, e.localized(l));
                }
                GroupState::Ready => {}
            }
            if !g.candidates.is_empty() {
                let mut sel = g.selected;
                egui::ComboBox::from_id_salt(("cand", gi))
                    .width(300.0)
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
    }

    fn subs_section(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        ui.label(tr(l, "sub_lang"));
        egui::ComboBox::from_id_salt("sublang")
            .selected_text(match self.sub_lang {
                Some(i) => LANGS[i].1,
                None => tr(l, "sub_lang_pick"),
            })
            .show_ui(ui, |ui| {
                for (i, (_, name)) in LANGS.iter().enumerate() {
                    ui.selectable_value(&mut self.sub_lang, Some(i), *name);
                }
            });
        ui.horizontal(|ui| {
            let can_search =
                self.sub_lang.is_some() && !self.files.is_empty() && self.busy.is_none();
            if ui
                .add_enabled(can_search, egui::Button::new(tr(l, "sub_search")))
                .clicked()
            {
                self.view = View::Subs;
                self.sub_search();
            }
            let can_dl = self.busy.is_none()
                && self
                    .sub_rows
                    .iter()
                    .any(|r| r.include && r.state == SubState::Found);
            if ui
                .add_enabled(can_dl, egui::Button::new(tr(l, "sub_download")))
                .clicked()
            {
                self.sub_download();
            }
        });
    }

    fn settings_section(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        egui::CollapsingHeader::new(tr(l, "settings")).show(ui, |ui| {
            ui.label(tr(l, "provider"));
            egui::ComboBox::from_id_salt("provider")
                .selected_text(self.config.provider.name())
                .show_ui(ui, |ui| {
                    for p in Provider::ALL {
                        ui.selectable_value(&mut self.config.provider, p, p.name());
                    }
                });
            ui.label(tr(l, "tmdb_key"));
            ui.add(
                egui::TextEdit::singleline(&mut self.config.tmdb_key)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            ui.label(tr(l, "meta_lang"));
            egui::ComboBox::from_id_salt("metalang")
                .selected_text(self.config.meta_lang.clone())
                .show_ui(ui, |ui| {
                    for code in ["it", "en"] {
                        ui.selectable_value(&mut self.config.meta_lang, code.to_string(), code);
                    }
                });
            ui.label(tr(l, "os_key"));
            ui.add(
                egui::TextEdit::singleline(&mut self.config.os_api_key)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            ui.label(tr(l, "os_user"));
            ui.add(
                egui::TextEdit::singleline(&mut self.config.os_username)
                    .desired_width(f32::INFINITY),
            );
            ui.label(tr(l, "os_pass"));
            ui.add(
                egui::TextEdit::singleline(&mut self.config.os_password)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            if ui.button(tr(l, "save")).clicked() {
                self.save_config();
            }
        });
    }

    fn plan_view(&self, ui: &mut egui::Ui) {
        let l = self.lang();
        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                egui::Grid::new("plan")
                    .striped(true)
                    .num_columns(3)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        ui.strong(tr(l, "col_current"));
                        ui.strong(tr(l, "col_new"));
                        ui.strong(tr(l, "col_status"));
                        ui.end_row();
                        for item in &self.plan {
                            ui.label(
                                item.source
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?"),
                            );
                            match &item.target {
                                Some(t) => {
                                    let shown = item
                                        .source
                                        .parent()
                                        .and_then(|b| t.strip_prefix(b).ok())
                                        .unwrap_or(t);
                                    ui.label(shown.display().to_string());
                                }
                                None => {
                                    ui.label("—");
                                }
                            }
                            let text = self.status_text(&item.status);
                            if item.status.is_blocking() {
                                ui.colored_label(egui::Color32::LIGHT_RED, text);
                            } else {
                                ui.label(text);
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    fn subs_view(&mut self, ui: &mut egui::Ui) {
        let l = self.lang();
        if self.sub_rows.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new(tr(l, "subs_hint")).size(20.0).weak());
            });
            return;
        }
        egui::ScrollArea::both()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                egui::Grid::new("subs")
                    .striped(true)
                    .num_columns(4)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        for (i, r) in self.sub_rows.iter_mut().enumerate() {
                            ui.checkbox(&mut r.include, "");
                            ui.label(r.path.file_name().and_then(|n| n.to_str()).unwrap_or("?"));
                            if r.candidates.is_empty() {
                                ui.label("—");
                            } else {
                                egui::ComboBox::from_id_salt(("subcand", i))
                                    .width(460.0)
                                    .selected_text(r.candidates[r.selected].label())
                                    .show_ui(ui, |ui| {
                                        for (k, c) in r.candidates.iter().enumerate() {
                                            ui.selectable_value(&mut r.selected, k, c.label());
                                        }
                                    });
                            }
                            match &r.state {
                                SubState::Searching => ui.label(tr(l, "searching")),
                                SubState::Found => ui.label(tr(l, "sub_found")),
                                SubState::NotFound => ui.label(tr(l, "sub_none")),
                                SubState::Downloading => ui.label(tr(l, "sub_downloading")),
                                SubState::Saved(p) => ui.label(format!(
                                    "{}: {}",
                                    tr(l, "sub_saved"),
                                    p.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                                )),
                                SubState::Failed(e) => {
                                    ui.colored_label(egui::Color32::LIGHT_RED, e.localized(l))
                                }
                            };
                            ui.end_row();
                        }
                    });
            });
    }
}

fn is_video(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXT.contains(&e.to_lowercase().as_str()))
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.viewport().close_requested()) {
            self.save_config();
        }
        if self.busy.is_none() {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .collect()
            });
            for p in dropped {
                self.add_path(&p);
            }
        }
        self.poll();
        self.refresh_plan();
        let l = self.lang();

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));

        egui::Panel::left("settings")
            .min_size(360.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    self.formats_section(ui);
                    ui.add_space(10.0);
                    self.action_section(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    self.online_section(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    self.subs_section(ui);
                    ui.add_space(12.0);
                    self.settings_section(ui);
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(tr(l, "drop_hint")).size(20.0).weak());
                });
            } else if self.view == View::Subs {
                self.subs_view(ui);
            } else {
                self.plan_view(ui);
            }
        });
    }
}
