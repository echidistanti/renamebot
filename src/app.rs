//! Finestra principale: elenco file, maschera, formati e anteprima della rinomina.

use bobina::config::{Config, Provider};
use bobina::guess::guess;
use bobina::http::{Cached, Fetch, Http};
use bobina::i18n::{Lang, tr};
use bobina::mask::Mask;
use bobina::metadata::{self, Candidate, Episode};
use bobina::ops::{self, Action, History};
use bobina::plan::{Formats, PlanItem, Status, build_plan_with};
use bobina::subs::{self, Client, Session, SubCandidate, SubsHttp, UreqSubs};
use bobina::template::{MediaInfo, Template};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    Failed(String),
}

struct SubRow {
    path: PathBuf,
    state: SubState,
    candidates: Vec<SubCandidate>,
    selected: usize,
    include: bool,
}

enum SubMsg {
    Found {
        path: PathBuf,
        res: Result<Vec<SubCandidate>, String>,
    },
    Done {
        path: PathBuf,
        res: Result<PathBuf, String>,
    },
}

struct Preset {
    name: &'static str,
    episode: &'static str,
    movie: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "Plex",
        episode: "TV Shows/{n}{y|prefix: (|suffix:)}/Season {s00}/{n} - {s00e00}{t|prefix: - }",
        movie: "Movies/{n}{y|prefix: (|suffix:)}/{n}{y|prefix: (|suffix:)}",
    },
    Preset {
        name: "Jellyfin",
        episode: "Shows/{n}{y|prefix: (|suffix:)}/Season {s}/{n} S{s00}E{e00}{t|prefix: - }",
        movie: "Movies/{n}{y|prefix: (|suffix:)}/{n}{y|prefix: (|suffix:)}",
    },
    Preset {
        name: "Kodi",
        episode: "TV Shows/{n}/Season {s}/{n} - {s00e00}{t|prefix: - }",
        movie: "Movies/{n}{y|prefix: (|suffix:)}/{n}{y|prefix: (|suffix:)}",
    },
];

#[derive(Clone, PartialEq)]
enum GroupState {
    Loading,
    Ready,
    Failed(String),
}

/// File che condividono lo stesso titolo riconosciuto: si cercano online una volta sola.
struct Group {
    key: String,
    query: String,
    is_episode: bool,
    year: Option<u32>,
    seasons: Vec<u32>,
    candidates: Vec<Candidate>,
    selected: usize,
    episodes: Vec<Episode>,
    state: GroupState,
}

/// Dati di un gruppo per il thread di ricerca: chiave, titolo, è serie, anno, stagioni.
type Job = (String, String, bool, Option<u32>, Vec<u32>);

enum Msg {
    Found {
        key: String,
        res: Result<(Vec<Candidate>, usize, Vec<Episode>), String>,
    },
    Episodes {
        key: String,
        idx: usize,
        res: Result<Vec<Episode>, String>,
    },
}

fn group_key(info: &MediaInfo) -> Option<String> {
    Some(format!(
        "{}|{}",
        info.n.as_deref()?.to_lowercase(),
        info.is_episode()
    ))
}

pub struct App {
    lang: Lang,
    files: Vec<PathBuf>,
    mask: String,
    fmt_episode: String,
    fmt_movie: String,
    dest: String,
    preset: usize,
    plan: Vec<PlanItem>,
    plan_key: String,
    error: Option<String>,
    action: Action,
    with_sidecars: bool,
    history: History,
    history_path: Option<PathBuf>,
    message: Option<String>,
    config: Config,
    config_path: Option<PathBuf>,
    fetcher: Arc<dyn Fetch>,
    online: bool,
    groups: Vec<Group>,
    revision: u64,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    view: View,
    sub_lang: Option<usize>,
    sub_rows: Vec<SubRow>,
    sub_tx: Sender<SubMsg>,
    sub_rx: Receiver<SubMsg>,
    subs_http: Arc<dyn SubsHttp>,
    session: Arc<Mutex<Option<Session>>>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let history_path = History::default_path();
        let history = history_path
            .as_deref()
            .map(History::load)
            .unwrap_or_default();
        let config_path = Config::default_path();
        let config = config_path.as_deref().map(Config::load).unwrap_or_default();
        let cache_dir = Cached::<Http>::default_dir()
            .unwrap_or_else(|| std::env::temp_dir().join("bobina-http"));
        let fetcher: Arc<dyn Fetch> = Arc::new(Cached::new(
            Http::default(),
            cache_dir,
            Duration::from_secs(7 * 24 * 3600),
        ));
        let (tx, rx) = channel();
        let (sub_tx, sub_rx) = channel();
        Self {
            view: View::Rename,
            sub_lang: None,
            sub_rows: Vec::new(),
            sub_tx,
            sub_rx,
            subs_http: Arc::new(UreqSubs::default()),
            session: Arc::new(Mutex::new(None)),
            config,
            config_path,
            fetcher,
            online: false,
            groups: Vec::new(),
            revision: 0,
            tx,
            rx,
            action: Action::Move,
            with_sidecars: true,
            history,
            history_path,
            message: None,
            lang: Lang::It,
            files: Vec::new(),
            mask: String::new(),
            fmt_episode: PRESETS[0].episode.into(),
            fmt_movie: PRESETS[0].movie.into(),
            dest: String::new(),
            preset: 0,
            plan: Vec::new(),
            plan_key: String::new(),
            error: None,
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

    fn base_info(mask: Option<&Mask>, path: &Path) -> Option<MediaInfo> {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        match mask {
            Some(m) => m.extract(stem),
            None => Some(guess(stem)),
        }
    }

    /// Dati di un file: dal nome (maschera o riconoscimento automatico), completati online se attivo.
    fn resolve_info(
        mask: Option<&Mask>,
        online: bool,
        groups: &[Group],
        path: &Path,
    ) -> Option<MediaInfo> {
        let base = Self::base_info(mask, path)?;
        if online
            && let Some(k) = group_key(&base)
            && let Some(g) = groups.iter().find(|g| g.key == k)
            && let Some(c) = g.candidates.get(g.selected)
        {
            return Some(metadata::enrich(&base, c, &g.episodes));
        }
        Some(base)
    }

    fn compile_mask(&self) -> Result<Option<Mask>, String> {
        if self.mask.trim().is_empty() {
            Ok(None)
        } else {
            Mask::compile(&self.mask)
                .map(Some)
                .map_err(|e| e.to_string())
        }
    }

    fn refresh_plan(&mut self) {
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            self.mask,
            self.fmt_episode,
            self.fmt_movie,
            self.dest,
            self.files.len(),
            self.online,
            self.revision
        );
        if key == self.plan_key {
            return;
        }
        self.plan_key = key;
        self.error = None;

        let episode = Template::parse(&self.fmt_episode).map_err(|e| e.to_string());
        let movie = Template::parse(&self.fmt_movie).map_err(|e| e.to_string());
        let mask = self.compile_mask();
        match (episode, movie, mask) {
            (Ok(e), Ok(m), Ok(mask)) => {
                let dest = (!self.dest.trim().is_empty()).then(|| PathBuf::from(self.dest.trim()));
                let groups = &self.groups;
                let online = self.online;
                let plan = build_plan_with(
                    &self.files,
                    &Formats {
                        episode: &e,
                        movie: &m,
                    },
                    dest.as_deref(),
                    &|p| Self::resolve_info(mask.as_ref(), online, groups, p),
                );
                self.plan = plan;
            }
            (e, m, k) => {
                self.error = e.err().or(m.err()).or(k.err());
                self.plan.clear();
            }
        }
    }

    /// Raggruppa i file per titolo riconosciuto e avvia la ricerca online in un thread.
    fn start_search(&mut self) {
        let Ok(mask) = self.compile_mask() else {
            return;
        };
        let mut groups: Vec<Group> = Vec::new();
        for f in &self.files {
            let Some(base) = Self::base_info(mask.as_ref(), f) else {
                continue;
            };
            let (Some(key), Some(name)) = (group_key(&base), base.n.clone()) else {
                continue;
            };
            let idx = match groups.iter().position(|g| g.key == key) {
                Some(i) => i,
                None => {
                    groups.push(Group {
                        key,
                        query: name,
                        is_episode: base.is_episode(),
                        year: base.y,
                        seasons: Vec::new(),
                        candidates: Vec::new(),
                        selected: 0,
                        episodes: Vec::new(),
                        state: GroupState::Loading,
                    });
                    groups.len() - 1
                }
            };
            if let Some(s) = base.s
                && !groups[idx].seasons.contains(&s)
            {
                groups[idx].seasons.push(s);
            }
        }
        let jobs: Vec<Job> = groups
            .iter()
            .map(|g| {
                (
                    g.key.clone(),
                    g.query.clone(),
                    g.is_episode,
                    g.year,
                    g.seasons.clone(),
                )
            })
            .collect();
        self.groups = groups;
        self.revision += 1;

        let (f, cfg, tx) = (self.fetcher.clone(), self.config.clone(), self.tx.clone());
        std::thread::spawn(move || {
            for (key, query, is_episode, year, seasons) in jobs {
                let res = (|| {
                    let cands = if is_episode {
                        metadata::search_series(f.as_ref(), &cfg, &query)?
                    } else {
                        metadata::search_movies(f.as_ref(), &cfg, &query)?
                    };
                    if cands.is_empty() {
                        return Err("nessun risultato".to_string());
                    }
                    let best = metadata::pick_best(&cands, year);
                    let eps = if is_episode {
                        metadata::episodes(f.as_ref(), &cfg, &cands[best], &seasons)?
                    } else {
                        Vec::new()
                    };
                    Ok((cands, best, eps))
                })();
                if tx.send(Msg::Found { key, res }).is_err() {
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
        if !g.is_episode {
            return;
        }
        g.state = GroupState::Loading;
        let (f, cfg, tx) = (self.fetcher.clone(), self.config.clone(), self.tx.clone());
        let (key, cand, seasons) = (g.key.clone(), g.candidates[sel].clone(), g.seasons.clone());
        std::thread::spawn(move || {
            let res = metadata::episodes(f.as_ref(), &cfg, &cand, &seasons);
            let _ = tx.send(Msg::Episodes { key, idx: sel, res });
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Found { key, res } => {
                    if let Some(g) = self.groups.iter_mut().find(|g| g.key == key) {
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
                }
                Msg::Episodes { key, idx, res } => {
                    if let Some(g) = self
                        .groups
                        .iter_mut()
                        .find(|g| g.key == key && g.selected == idx)
                    {
                        match res {
                            Ok(eps) => {
                                g.episodes = eps;
                                g.state = GroupState::Ready;
                            }
                            Err(e) => g.state = GroupState::Failed(e),
                        }
                    }
                }
            }
            self.revision += 1;
        }
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
        let tx = self.sub_tx.clone();
        std::thread::spawn(move || {
            for (path, info) in jobs {
                let res = match info {
                    None => Err("nome non riconosciuto".to_string()),
                    Some(info) => {
                        let hash = subs::moviehash(&path).ok();
                        client.search(&info, hash.as_deref(), &lang)
                    }
                };
                if tx.send(SubMsg::Found { path, res }).is_err() {
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
        let session = self.session.clone();
        let tx = self.sub_tx.clone();
        std::thread::spawn(move || {
            for (path, cand) in jobs {
                let res = (|| {
                    let target = subs::subtitle_path(&path, &lang, &cand.file_name);
                    if target.exists() {
                        return Err(format!(
                            "esiste già: {}",
                            target.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                        ));
                    }
                    let mut guard = session
                        .lock()
                        .map_err(|_| "sessione non disponibile".to_string())?;
                    if guard.is_none() {
                        *guard = Some(client.login()?);
                    }
                    let s = guard.clone().expect("sessione appena creata");
                    drop(guard);
                    let (bytes, _name) = client.download(&s, cand.file_id).inspect_err(|_| {
                        // Il token può essere scaduto: al prossimo tentativo si rifà il login.
                        if let Ok(mut g) = session.lock() {
                            *g = None;
                        }
                    })?;
                    std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
                    Ok(target)
                })();
                if tx.send(SubMsg::Done { path, res }).is_err() {
                    return;
                }
            }
        });
    }

    fn poll_subs(&mut self) {
        while let Ok(msg) = self.sub_rx.try_recv() {
            match msg {
                SubMsg::Found { path, res } => {
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
                SubMsg::Done { path, res } => {
                    if let Some(r) = self.sub_rows.iter_mut().find(|r| r.path == path) {
                        r.state = match res {
                            Ok(p) => SubState::Saved(p),
                            Err(e) => SubState::Failed(e),
                        };
                    }
                }
            }
        }
    }

    fn subs_view(&mut self, ui: &mut egui::Ui) {
        let l = self.lang;
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
                                    ui.colored_label(egui::Color32::LIGHT_RED, e.as_str())
                                }
                            };
                            ui.end_row();
                        }
                    });
            });
    }

    fn save_history(&mut self) {
        if let Some(p) = &self.history_path
            && let Err(e) = self.history.save(p)
        {
            self.message = Some(format!("registro non salvato: {e}"));
        }
    }

    fn apply(&mut self) {
        let rep = ops::apply(&self.plan, self.action, self.with_sidecars);
        let l = self.lang;
        let mut msg = format!("{} {}", rep.done, tr(l, "msg_done"));
        if !rep.errors.is_empty() {
            let first = &rep.errors[0];
            msg.push_str(&format!(
                ", {} {} ({}: {})",
                rep.errors.len(),
                tr(l, "msg_errors"),
                first.0.display(),
                first.1
            ));
        }
        self.message = Some(msg);
        for e in &rep.batch.entries {
            if e.action == Action::Move
                && let Some(pos) = self.files.iter().position(|f| *f == e.from)
            {
                self.files[pos] = e.to.clone();
            }
        }
        self.history.push(rep.batch);
        self.save_history();
        self.files.retain(|f| f.exists());
        self.plan_key.clear();
    }

    fn undo_last(&mut self) {
        let Some(batch) = self.history.batches.pop() else {
            return;
        };
        let errors = ops::undo(&batch);
        for e in &batch.entries {
            if e.action == Action::Move {
                self.add_path(&e.from.clone());
            }
        }
        self.message = Some(match errors.first() {
            None => tr(self.lang, "msg_undone").to_string(),
            Some((p, e)) => format!("{} ({}: {e})", tr(self.lang, "msg_undone"), p.display()),
        });
        self.save_history();
        self.plan_key.clear();
    }

    fn status_text(&self, s: &Status) -> String {
        let l = self.lang;
        match s {
            Status::Ok => tr(l, "st_ok").into(),
            Status::Unchanged => tr(l, "st_same").into(),
            Status::Exists => tr(l, "st_exists").into(),
            Status::Duplicate => tr(l, "st_dup").into(),
            Status::NoMatch => tr(l, "st_nomatch").into(),
            Status::Missing(f) => format!("{}: {}", tr(l, "st_missing"), f.join(", ")),
        }
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
        self.poll();
        self.poll_subs();
        let busy = self
            .sub_rows
            .iter()
            .any(|r| matches!(r.state, SubState::Searching | SubState::Downloading));
        if busy || self.groups.iter().any(|g| g.state == GroupState::Loading) {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        self.refresh_plan();
        let l = self.lang;

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.view, View::Rename, tr(l, "tab_rename"));
                ui.selectable_value(&mut self.view, View::Subs, tr(l, "tab_subs"));
                ui.separator();
                if ui.button(tr(l, "add_files")).clicked()
                    && let Some(paths) = rfd::FileDialog::new().pick_files()
                {
                    for p in paths {
                        self.add_path(&p);
                    }
                }
                if ui.button(tr(l, "add_folder")).clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    self.add_path(&p);
                }
                if ui.button(tr(l, "clear")).clicked() {
                    self.files.clear();
                }
                ui.label(format!("{} {}", self.files.len(), tr(l, "files_count")));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("lang")
                        .selected_text(self.lang.name())
                        .show_ui(ui, |ui| {
                            for lang in Lang::ALL {
                                ui.selectable_value(&mut self.lang, lang, lang.name());
                            }
                        });
                    ui.label(tr(l, "language"));
                });
            });
        });

        egui::Panel::left("settings")
            .min_size(360.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.label(tr(l, "mask"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.mask)
                            .hint_text(tr(l, "mask_hint"))
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(10.0);

                    ui.label(tr(l, "preset"));
                    let before = self.preset;
                    egui::ComboBox::from_id_salt("preset")
                        .selected_text(PRESETS[self.preset].name)
                        .show_ui(ui, |ui| {
                            for (i, p) in PRESETS.iter().enumerate() {
                                ui.selectable_value(&mut self.preset, i, p.name);
                            }
                        });
                    if self.preset != before {
                        self.fmt_episode = PRESETS[self.preset].episode.into();
                        self.fmt_movie = PRESETS[self.preset].movie.into();
                    }
                    ui.add_space(6.0);

                    ui.label(tr(l, "fmt_episode"));
                    ui.add(
                        egui::TextEdit::multiline(&mut self.fmt_episode)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    ui.label(tr(l, "fmt_movie"));
                    ui.add(
                        egui::TextEdit::multiline(&mut self.fmt_movie)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(6.0);

                    ui.label(tr(l, "dest"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.dest)
                            .hint_text(tr(l, "dest_hint"))
                            .desired_width(f32::INFINITY),
                    );

                    ui.add_space(10.0);
                    ui.label(tr(l, "action"));
                    ui.radio_value(&mut self.action, Action::Move, tr(l, "act_move"));
                    ui.radio_value(&mut self.action, Action::Copy, tr(l, "act_copy"));
                    ui.checkbox(&mut self.with_sidecars, tr(l, "sidecars"));
                    ui.add_space(8.0);
                    let ready = self.plan.iter().any(|i| i.status == Status::Ok);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(ready, egui::Button::new(tr(l, "apply")))
                            .clicked()
                        {
                            self.apply();
                        }
                        let can_undo = !self.history.batches.is_empty();
                        if ui
                            .add_enabled(can_undo, egui::Button::new(tr(l, "undo")))
                            .clicked()
                        {
                            self.undo_last();
                        }
                    });
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

                    ui.add_space(12.0);
                    ui.separator();
                    ui.checkbox(&mut self.online, tr(l, "online"));
                    if self.online {
                        if ui
                            .add_enabled(!self.files.is_empty(), egui::Button::new(tr(l, "search")))
                            .clicked()
                        {
                            self.start_search();
                        }
                        let mut pick: Option<(usize, usize)> = None;
                        for (gi, g) in self.groups.iter().enumerate() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&g.query).strong());
                            match &g.state {
                                GroupState::Loading => {
                                    ui.label(tr(l, "searching"));
                                }
                                GroupState::Failed(e) => {
                                    ui.colored_label(egui::Color32::LIGHT_RED, e.as_str());
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

                    ui.add_space(12.0);
                    ui.separator();
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
                        let can_search = self.sub_lang.is_some() && !self.files.is_empty();
                        if ui
                            .add_enabled(can_search, egui::Button::new(tr(l, "sub_search")))
                            .clicked()
                        {
                            self.view = View::Subs;
                            self.sub_search();
                        }
                        let can_dl = self
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

                    ui.add_space(12.0);
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
                                    ui.selectable_value(
                                        &mut self.config.meta_lang,
                                        code.to_string(),
                                        code,
                                    );
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
                        if ui.button(tr(l, "save")).clicked()
                            && let Some(p) = &self.config_path
                            && let Err(e) = self.config.save(p)
                        {
                            self.message = Some(format!("impostazioni non salvate: {e}"));
                        }
                    });
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if self.files.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new(tr(l, "drop_hint")).size(20.0).weak());
                });
                return;
            }
            if self.view == View::Subs {
                self.subs_view(ui);
                return;
            }
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
                                let name = item
                                    .source
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("?");
                                ui.label(name);
                                match &item.target {
                                    Some(t) => {
                                        let shown = match item.source.parent() {
                                            Some(base) => t.strip_prefix(base).unwrap_or(t),
                                            None => t,
                                        };
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
        });
    }
}
