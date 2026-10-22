//! Moteur de transferts : file d'attente, reprises, limites de débit,
//! politique d'écrasement, octets/s, journaux.

use crate::fs::{RemoteFs, Sink};
use crate::hub::{Event, LogHub};
use crate::models::{
    Direction, JobState, Priority, TransferJob, QueueStats,
};
use crate::util::{human_size, now_ms};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// Limitateur de débit à fenêtre glissante.
pub struct Throttle {
    limit: Option<u64>,
    window_ms: i64,
    window_start: i64,
    window_bytes: u64,
}

impl Throttle {
    pub fn new(limit: Option<u64>) -> Throttle {
        Throttle {
            limit,
            window_ms: 200,
            window_start: now_ms(),
            window_bytes: 0,
        }
    }

    pub async fn charge(&mut self, n: usize) {
        let Some(limit) = self.limit else { return };
        let now = now_ms();
        if now - self.window_start >= self.window_ms {
            self.window_start = now;
            self.window_bytes = 0;
        }
        self.window_bytes += n as u64;
        if self.window_bytes > limit * (self.window_ms as u64) / 1000 {
            let wait = (self.window_bytes * 1000 / limit).max(1) as u64
                - (now - self.window_start).max(0) as u64;
            if wait > 0 {
                tokio::time::sleep(Duration::from_millis(wait.min(1500))).await;
            }
        }
    }

    pub fn rate(&self) -> f64 {
        match self.limit {
            Some(_) => 0.0,
            None => 0.0,
        }
    }
}

/// Mesure de vitesse (moyenne glissante).
pub struct Meter {
    pub total: u64,
    pub start: i64,
    pub last_ts: i64,
    pub last_bytes: u64,
    pub speed: f64,
}

impl Meter {
    pub fn new(total: u64) -> Meter {
        let now = now_ms();
        Meter {
            total,
            start: now,
            last_ts: now,
            last_bytes: 0,
            speed: 0.0,
        }
    }

    pub fn tick(&mut self, n: usize) -> bool {
        self.last_bytes += n as u64;
        let now = now_ms();
        let dt = now - self.last_ts;
        if dt >= 250 {
            self.speed = (self.last_bytes as f64) * 1000.0 / (dt as f64);
            self.last_ts = now;
            self.last_bytes = 0;
            return true;
        }
        false
    }
}

/// État de la file.
struct Queue {
    jobs: Vec<TransferJob>,
    paused: bool,
}

/// Moteur de file d'attente.
pub struct Transfers {
    queue: Mutex<Queue>,
    hub: Arc<LogHub>,
    notify: Arc<Notify>,
    running: AtomicBool,
    #[allow(dead_code)]
    active_count: AtomicU64,
    chunk: usize,
}

impl Transfers {
    pub fn new(hub: Arc<LogHub>, chunk_kb: usize) -> Arc<Transfers> {
        Arc::new(Transfers {
            queue: Mutex::new(Queue {
                jobs: Vec::new(),
                paused: false,
            }),
            hub,
            notify: Arc::new(Notify::new()),
            running: AtomicBool::new(false),
            active_count: AtomicU64::new(0),
            chunk: (chunk_kb.max(4)) * 1024,
        })
    }

    pub fn set_chunk(&self, kb: usize) {
        // Le réglage est lu à chaque tâche ; celui-ci reste un défaut.
        let _ = kb;
    }

    pub fn notify(&self) {
        self.notify.notify_waiters();
    }

    pub fn snapshot(&self) -> Vec<TransferJob> {
        self.queue.lock().unwrap().jobs.clone()
    }

    pub fn stats(&self) -> QueueStats {
        let q = self.queue.lock().unwrap();
        let mut s = QueueStats {
            pending: 0,
            active: 0,
            done: 0,
            failed: 0,
            paused: q.paused,
            bytes_per_sec: 0.0,
            total_bytes: 0,
            done_bytes: 0,
        };
        let mut speed = 0f64;
        for j in &q.jobs {
            s.total_bytes += j.size;
            match j.state {
                JobState::Waiting => s.pending += 1,
                JobState::Active => {
                    s.active += 1;
                    speed += j.speed;
                }
                JobState::Done => {
                    s.done += 1;
                    s.done_bytes += j.size;
                }
                JobState::Error => {
                    s.failed += 1;
                    s.done_bytes += j.transferred;
                }
                JobState::Paused => s.pending += 1,
                JobState::Cancelled => {}
            }
        }
        s.bytes_per_sec = speed;
        s
    }

    pub fn is_paused(&self) -> bool {
        self.queue.lock().unwrap().paused
    }

    pub fn set_paused(&self, on: bool) {
        self.queue.lock().unwrap().paused = on;
        if !on {
            self.notify.notify_waiters();
        }
    }

    pub fn add(&self, job: TransferJob) -> TransferJob {
        let mut q = self.queue.lock().unwrap();
        q.jobs.push(job.clone());
        job
    }

    pub fn add_many(&self, jobs: Vec<TransferJob>) -> usize {
        let mut q = self.queue.lock().unwrap();
        let n = jobs.len();
        q.jobs.extend(jobs);
        n
    }

    pub fn clear_done(&self) -> usize {
        let mut q = self.queue.lock().unwrap();
        let before = q.jobs.len();
        q.jobs.retain(|j| j.state != JobState::Done);
        before - q.jobs.len()
    }

    pub fn clear_failed(&self) -> usize {
        let mut q = self.queue.lock().unwrap();
        let before = q.jobs.len();
        q.jobs.retain(|j| j.state != JobState::Error);
        before - q.jobs.len()
    }

    pub fn clear_all(&self) {
        let mut q = self.queue.lock().unwrap();
        q.jobs.clear();
    }

    pub fn cancel(&self, id: &str) -> bool {
        let mut q = self.queue.lock().unwrap();
        let mut found = false;
        for j in q.jobs.iter_mut() {
            if j.id == id {
                if j.state == JobState::Waiting || j.state == JobState::Paused {
                    j.state = JobState::Cancelled;
                    j.finished = now_ms();
                    found = true;
                } else if j.state == JobState::Active {
                    j.state = JobState::Cancelled;
                    j.finished = now_ms();
                    found = true;
                }
            }
        }
        found
    }

    pub fn cancel_site(&self, site_id: &str) -> usize {
        let mut q = self.queue.lock().unwrap();
        let mut n = 0;
        for j in q.jobs.iter_mut() {
            if j.site_id == site_id && (j.state == JobState::Waiting || j.state == JobState::Paused)
            {
                j.state = JobState::Cancelled;
                n += 1;
            }
        }
        n
    }

    pub fn set_priority(&self, id: &str, p: Priority) -> bool {
        let mut q = self.queue.lock().unwrap();
        let mut found = false;
        for j in q.jobs.iter_mut() {
            if j.id == id {
                j.priority = p;
                found = true;
            }
        }
        if found {
            q.jobs.sort_by_key(|j| (std::cmp::Reverse(j.priority), j.state as u32));
        }
        found
    }

    pub fn pause(&self, id: &str) -> bool {
        let mut q = self.queue.lock().unwrap();
        for j in q.jobs.iter_mut() {
            if j.id == id && (j.state == JobState::Waiting || j.state == JobState::Paused) {
                j.state = if j.state == JobState::Paused {
                    JobState::Waiting
                } else {
                    JobState::Paused
                };
                return true;
            }
        }
        false
    }

    pub fn retry(&self, id: &str) -> bool {
        let mut q = self.queue.lock().unwrap();
        for j in q.jobs.iter_mut() {
            if j.id == id && j.state == JobState::Error {
                j.state = JobState::Waiting;
                j.error.clear();
                return true;
            }
        }
        false
    }

    pub fn retry_all_failed(&self) -> usize {
        let mut q = self.queue.lock().unwrap();
        let mut n = 0;
        for j in q.jobs.iter_mut() {
            if j.state == JobState::Error {
                j.state = JobState::Waiting;
                j.error.clear();
                n += 1;
            }
        }
        n
    }

    pub fn job(&self, id: &str) -> Option<TransferJob> {
        self.queue
            .lock()
            .unwrap()
            .jobs
            .iter()
            .find(|j| j.id == id)
            .cloned()
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Boucle de supervision, à lancer une fois par appli.
    pub async fn run(self: Arc<Self>, app: Arc<crate::app::App>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut was_busy = false;
        loop {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            if self.is_paused() {
                tokio::time::sleep(Duration::from_millis(400)).await;
                continue;
            }
            let max = {
                let s = app.store.read().unwrap();
                s.settings.transfers.max_parallel.max(1)
            };
            // Notification de fin de file (bouton « avertir » de FileZilla).
            let busy = {
                let q = self.queue.lock().unwrap();
                q.jobs
                    .iter()
                    .any(|j| j.state == JobState::Active || j.state == JobState::Waiting)
            };
            if was_busy && !busy {
                let nf = app.store.read().unwrap().settings.general.notify_finish;
                if nf {
                    self.hub.success("transfers", "La file des transferts est vide.");
                    self.hub.publish(Event::toast("ok", "Tous les transferts sont terminés."));
                }
            }
            was_busy = busy;
            let taken = {
                let mut q = self.queue.lock().unwrap();
                let mut ids = Vec::new();
                let live = q
                    .jobs
                    .iter()
                    .filter(|j| j.state == JobState::Active)
                    .count();
                if live >= max {
                    Vec::new()
                } else {
                    let mut cand: Vec<(usize, usize, usize)> = q
                        .jobs
                        .iter()
                        .enumerate()
                        .filter(|(_, j)| j.state == JobState::Waiting)
                        .map(|(i, j)| (i, j.priority as usize, 0))
                        .collect();
                    cand.sort_by(|a, b| b.1.cmp(&a.1));
                    let slots = max - live;
                    let picked: Vec<usize> = cand.iter().take(slots).map(|(i, _, _)| *i).collect();
                    for i in picked {
                        q.jobs[i].state = JobState::Active;
                        q.jobs[i].started = now_ms();
                        ids.push(q.jobs[i].id.clone());
                    }
                    ids
                }
            };
            if taken.is_empty() {
                // Rien à faire, on attend une notification ou un job qui finit.
                tokio::select! {
                    _ = self.notify.notified() => {}
                    _ = tokio::time::sleep(Duration::from_millis(1200)) => {}
                }
                continue;
            }
            for id in taken {
                let me = self.clone();
                let app = app.clone();
                tokio::spawn(async move {
                    me.execute(app, &id).await;
                });
            }
        }
    }

    /// Exécute une ligne de la file.
    pub async fn execute(self: Arc<Self>, app: Arc<crate::app::App>, id: &str) {
        let job = match self.job(id) {
            Some(j) => j,
            None => return,
        };
        let site = {
            app.site(&job.site_id)
        };
        let (settings, hub) = {
            let s = app.store.read().unwrap();
            (s.settings.clone(), app.hub.clone())
        };
        let site = match site {
            Some(s) => s,
            None => {
                self.fail(id, "le site de ce transfert n'existe plus");
                return;
            }
        };
        if site.disabled {
            self.fail(id, "le site est désactivé");
            return;
        }
        let dir = job.direction;
        let pool = match app.pool_for(&site, &settings).await {
            Ok(p) => p,
            Err(e) => {
                self.fail(id, &format!("connexion impossible : {e}"));
                return;
            }
        };
        let lease = match pool.lease(Duration::from_secs(settings.net.connect_timeout.max(10) + 5)).await {
            Ok(l) => l,
            Err(e) => {
                self.fail(id, &format!("pas de connexion disponible : {e}"));
                return;
            }
        };
        let fs = lease.fs().clone();
        let remote = job.remote.clone();
        let local = PathBuf::from(&job.local);

        let res: Result<u64, String> = match dir {
            Direction::Down => {
                download(&self, &fs, &remote, &local, &job, &settings, &hub).await
            }
            Direction::Up => upload(&self, &fs, &remote, &local, &job, &settings, &hub).await,
        };

        drop(lease);
        match res {
            Ok(n) => {
                self.complete(id, n);
            }
            Err(e) => {
                if e == "__CANCELLED__" {
                    let mut q = self.queue.lock().unwrap();
                    if let Some(j) = q.jobs.iter_mut().find(|j| j.id == id) {
                        j.state = JobState::Cancelled;
                        j.finished = now_ms();
                    }
                    hub.info("transfers", format!("Transfert annulé : {}", crate::util::remote_base(&remote)));
                } else {
                    let attempts = self.job(id).map(|j| j.attempts).unwrap_or(0);
                    if attempts < settings.transfers.retries && is_transient(&e) {
                        let delay = settings.transfers.retry_delay;
                        hub.warn(
                            "transfers",
                            format!(
                                "Échec ({}), nouvelle tentative dans {delay}s : {e}",
                                crate::util::remote_base(&remote)
                            ),
                        );
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        let mut q = self.queue.lock().unwrap();
                        if let Some(j) = q.jobs.iter_mut().find(|j| j.id == id) {
                            j.attempts += 1;
                            j.state = JobState::Waiting;
                            j.error = e.clone();
                        }
                        drop(q);
                        self.notify.notify_waiters();
                        return;
                    }
                    self.fail(id, &e);
                }
            }
        }
        self.notify.notify_waiters();
    }

    fn complete(&self, id: &str, n: u64) {
        let mut q = self.queue.lock().unwrap();
        if let Some(j) = q.jobs.iter_mut().find(|j| j.id == id) {
            j.transferred = n;
            j.state = JobState::Done;
            j.finished = now_ms();
            j.speed = 0.0;
            j.error.clear();
        }
    }

    pub fn fail(&self, id: &str, e: &str) {
        let mut q = self.queue.lock().unwrap();
        if let Some(j) = q.jobs.iter_mut().find(|j| j.id == id) {
            j.state = JobState::Error;
            j.error = e.to_string();
            j.finished = now_ms();
            j.speed = 0.0;
        }
        self.hub.err("transfers", e.to_string());
        self.hub.publish(crate::hub::Event::job_done(id, false, e));
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.notify.notify_waiters();
    }
}

fn is_transient(e: &str) -> bool {
    let l = e.to_lowercase();
    l.contains("délai")
        || l.contains("déconnect")
        || l.contains("connexion")
        || l.contains("timed out")
        || l.contains("reset")
        || l.contains("temporaire")
}

/// Progression partagée : met à jour la ligne et le hub.
struct Tracker<'a> {
    id: &'a str,
    engine: &'a Transfers,
    meter: Meter,
    throttle: Throttle,
    last_emit: i64,
}

impl<'a> Tracker<'a> {
    fn new(id: &'a str, engine: &'a Transfers, total: u64, limit: Option<u64>) -> Tracker<'a> {
        Tracker {
            id,
            engine,
            meter: Meter::new(total),
            throttle: Throttle::new(limit),
            last_emit: 0,
        }
    }

    async fn step(&mut self, n: usize, transferred: u64) -> Result<(), String> {
        self.throttle.charge(n).await;
        let dirty = self.meter.tick(n);
        if dirty || now_ms() - self.last_emit > 500 {
            self.last_emit = now_ms();
            {
                let mut q = self.engine.queue.lock().unwrap();
                if let Some(j) = q.jobs.iter_mut().find(|j| j.id == self.id) {
                    if j.state == JobState::Cancelled {
                        return Err("__CANCELLED__".into());
                    }
                    j.transferred = transferred;
                    j.speed = self.meter.speed;
                }
            }
            self.engine
                .hub
                .publish(crate::hub::Event::dirty("queue"));
        }
        Ok(())
    }
}

/// Téléchargement complet d'un fichier distant vers le disque local.
async fn download(
    engine: &Transfers,
    fs: &Arc<dyn RemoteFs>,
    remote: &str,
    local: &Path,
    job: &TransferJob,
    settings: &crate::settings::Settings,
    hub: &Arc<LogHub>,
) -> Result<u64, String> {
    let chunk = engine.chunk;
    if let Some(p) = local.parent() {
        if let Err(e) = tokio::fs::create_dir_all(p).await {
            return Err(format!("dossier local : {e}"));
        }
    }
    // Taille déjà présente pour la reprise.
    let offset = if settings.transfers.auto_resume {
        tokio::fs::metadata(local).await.map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    let total = if job.size > 0 { job.size } else { fs.size(remote).await.unwrap_or(0) };
    if offset >= total && total > 0 && settings.transfers.auto_resume {
        hub.success("transfers", format!("{} est déjà à jour", crate::util::remote_base(remote)));
        return Ok(0);
    }
    hub.status(format!(
        "Téléchargement de {} ({}) vers {}",
        crate::util::remote_base(remote),
        human_size(total),
        local.display()
    ));
    let mut src = fs
        .open_read(remote, offset)
        .await
        .map_err(|e| e.to_string())?;
    let mut sink: Box<dyn Sink + Send> = if offset > 0 {
        Box::new(FileSink::append(local).await?)
    } else {
        Box::new(FileSink::create(local).await?)
    };
    let mut tr = Tracker::new(&job.id, engine, total, settings.speed_limit(false));
    let mut done = offset;
    let mut buf = vec![0u8; chunk.max(8192)];
    loop {
        match src.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                sink.write(&buf[..n]).await.map_err(|e| e.to_string())?;
                done += n as u64;
                tr.step(n, done).await?;
            }
            Err(e) => {
                let _ = src.close().await;
                return Err(format!("lecture distante : {e}"));
            }
        }
    }
    src.close().await.map_err(|e| e.to_string())?;
    sink.finish().await.map_err(|e| e.to_string())?;
    if settings.transfers.preserve_mtime {
        if let Ok(e) = fs.stat(remote).await {
            if let Some(t) = e.mtime {
                let _ = crate::local::set_mtime(local, t);
            }
        }
    }
    hub.success(
        "transfers",
        format!(
            "Transfert réussi : {} ({} reçus)",
            crate::util::remote_base(remote),
            human_size(done.saturating_sub(offset))
        ),
    );
    hub.publish(crate::hub::Event::toast("success", "Transfert terminé"));
    Ok(done)
}

/// Envoi d'un fichier local vers le serveur.
async fn upload(
    engine: &Transfers,
    fs: &Arc<dyn RemoteFs>,
    remote: &str,
    local: &Path,
    job: &TransferJob,
    settings: &crate::settings::Settings,
    hub: &Arc<LogHub>,
) -> Result<u64, String> {
    let chunk = engine.chunk;
    let md = tokio::fs::metadata(local)
        .await
        .map_err(|e| format!("{} : {e}", local.display()))?;
    let size = md.len();
    let existing = fs.size(remote).await.unwrap_or(0);
    let mut offset = 0u64;
    let append;
    if settings.transfers.auto_resume && existing > 0 && existing < size {
        offset = existing;
        append = true;
    } else {
        append = false;
    }
    hub.status(format!(
        "Envoi de {} ({}) vers {}",
        local.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        human_size(size),
        remote
    ));
    let mut sink = fs.open_write(remote, offset, append).await.map_err(|e| e.to_string())?;
    let mut src = FileSrc::open(local, offset).await?;
    let mut tr = Tracker::new(&job.id, engine, size, settings.speed_limit(true));
    let mut done = offset;
    let mut buf = vec![0u8; chunk.max(8192)];
    loop {
        match src.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                sink.write(&buf[..n]).await.map_err(|e| e.to_string())?;
                done += n as u64;
                tr.step(n, done).await?;
            }
            Err(e) => {
                return Err(format!("lecture locale : {e}"));
            }
        }
    }
    sink.finish().await.map_err(|e| e.to_string())?;
    if settings.transfers.preserve_mtime {
        if let Ok(t) = md.modified() {
            let ts = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let _ = fs.set_mtime(remote, ts).await;
        }
    }
    hub.success(
        "transfers",
        format!("Transfert réussi : {} ({} envoyés)", remote, human_size(done.saturating_sub(offset))),
    );
    hub.publish(crate::hub::Event::toast("success", "Envoi terminé"));
    Ok(done)
}

/// File Src locale.
struct FileSrc {
    file: tokio::fs::File,
}

impl FileSrc {
    async fn open(path: &Path, offset: u64) -> Result<FileSrc, String> {
        use tokio::io::AsyncSeekExt;
        let mut f = tokio::fs::File::open(path)
            .await
            .map_err(|e| format!("{} : {e}", path.display()))?;
        if offset > 0 {
            f.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(FileSrc { file: f })
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        use tokio::io::AsyncReadExt;
        self.file.read(buf).await.map_err(|e| e.to_string())
    }
}

/// Sink fichier local.
struct FileSink {
    file: tokio::fs::File,
}

impl FileSink {
    async fn create(path: &Path) -> Result<FileSink, String> {
        let file = tokio::fs::File::create(path)
            .await
            .map_err(|e| format!("{} : {e}", path.display()))?;
        Ok(FileSink { file })
    }
    async fn append(path: &Path) -> Result<FileSink, String> {
        let file = tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .await
            .map_err(|e| format!("{} : {e}", path.display()))?;
        Ok(FileSink { file })
    }
}

#[async_trait::async_trait]
impl Sink for FileSink {
    async fn write(&mut self, data: &[u8]) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.file.write_all(data).await
    }
    async fn finish(&mut self) -> std::io::Result<()> {
        use tokio::io::AsyncWriteExt;
        self.file.flush().await
    }
}

