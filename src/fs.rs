//! Abstraction du fichier distant (FTP/SFTP) et gestion de parc de connexions.

use crate::models::{ConnInfo, Entry, Listing, Proto};
use crate::util::{now_ms, remote_join, remote_parent};
use async_trait::async_trait;
use serde_json::Value;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Résultat des opérations distantes.
pub type FsResult<T> = Result<T, FsError>;

#[derive(Debug, Clone)]
pub enum FsError {
    /// Code de réponse FTP.
    Ftp(u16, String),
    Io(String),
    Tls(String),
    Auth(String),
    Timeout(String),
    NotFound(String),
    Denied(String),
    Closed,
    Protocol(String),
    Other(String),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::Ftp(c, t) => write!(f, "erreur serveur {c} : {t}"),
            FsError::Io(s) => write!(f, "{s}"),
            FsError::Tls(s) => write!(f, "erreur TLS : {s}"),
            FsError::Auth(s) => write!(f, "authentification refusée : {s}"),
            FsError::Timeout(s) => write!(f, "délai dépassé : {s}"),
            FsError::NotFound(s) => write!(f, "introuvable : {s}"),
            FsError::Denied(s) => write!(f, "droits insuffisants : {s}"),
            FsError::Closed => write!(f, "connexion fermée par le serveur"),
            FsError::Protocol(s) => write!(f, "protocole inattendu : {s}"),
            FsError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for FsError {}

impl From<io::Error> for FsError {
    fn from(e: io::Error) -> Self {
        FsError::Io(e.to_string())
    }
}

impl FsError {
    pub fn code(&self) -> Option<u16> {
        match self {
            FsError::Ftp(c, _) => Some(*c),
            _ => None,
        }
    }

    /// Traduit une réponse FTP en message explicite (esprit FileZilla).
    pub fn from_reply(code: u16, text: &str) -> FsError {
        let t = text.trim().to_string();
        let low = t.to_lowercase();
        match code {
            550 if low.contains("permission") || low.contains("denied") => FsError::Denied(t),
            550 | 553 => FsError::NotFound(t),
            450 => FsError::Other(format!("fichier indisponible ({t})")),
            421 | 426 => FsError::Timeout(t),
            530 | 532 => FsError::Auth(t),
            552 => FsError::Denied(format!("plus de place disponible : {t}")),
            501 | 504 | 520 => FsError::Other(format!("syntaxe de commande invalide : {t}")),
            _ => FsError::Ftp(code, t),
        }
    }

    pub fn is_transient(&self) -> bool {
        match self {
            FsError::Timeout(_) | FsError::Closed | FsError::Io(_) => true,
            FsError::Ftp(c, _) => (400..500).contains(c),
            _ => false,
        }
    }
}

/// Source d'octets distants.
#[async_trait]
pub trait Src: Send {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    async fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Destination d'octets distants.
#[async_trait]
pub trait Sink: Send {
    async fn write(&mut self, data: &[u8]) -> io::Result<()>;
    async fn finish(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Source en mémoire (tests, aperçus).
pub struct MemSrc(pub Vec<u8>, pub usize);
#[async_trait]
impl Src for MemSrc {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.1 >= self.0.len() {
            return Ok(0);
        }
        let n = (self.0.len() - self.1).min(buf.len());
        buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
        self.1 += n;
        Ok(n)
    }
}

/// Destination mémoire.
#[derive(Default)]
pub struct VecSink(pub Vec<u8>);
#[async_trait]
impl Sink for VecSink {
    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.0.extend_from_slice(data);
        Ok(())
    }
}

/// Interface d'un système de fichiers distant.
#[async_trait]
pub trait RemoteFs: Send + Sync {
    async fn list(&self, path: &str) -> FsResult<Listing>;
    async fn stat(&self, path: &str) -> FsResult<Entry>;
    async fn realpath(&self, path: &str) -> FsResult<String>;
    async fn mkdir(&self, path: &str) -> FsResult<()>;
    async fn rmdir(&self, path: &str) -> FsResult<()>;
    async fn remove(&self, path: &str) -> FsResult<()>;
    async fn rename(&self, from: &str, to: &str) -> FsResult<()>;
    async fn chmod(&self, path: &str, mode: u32) -> FsResult<()>;
    async fn chown(&self, path: &str, user: &str) -> FsResult<()>;
    async fn set_mtime(&self, path: &str, ts: i64) -> FsResult<()>;

    async fn exists(&self, path: &str) -> bool {
        self.stat(path).await.is_ok()
    }

    /// Crée un dossier distant et tous ses parents, comme le fait FileZilla
    /// quand on envoie vers une arborescence qui n'existe pas encore.
    async fn mkdir_p(&self, path: &str) -> FsResult<()> {
        if self.exists(path).await {
            return Ok(());
        }
        // remonte la chaîne jusqu'au premier ancêtre existant
        let mut missing: Vec<String> = Vec::new();
        let mut cur = path.to_string();
        loop {
            if self.exists(&cur).await {
                break;
            }
            missing.push(cur.clone());
            let parent = crate::util::remote_parent(&cur);
            if parent == cur || parent.is_empty() {
                break;
            }
            cur = parent;
            if missing.len() > 64 {
                break;
            }
        }
        for m in missing.into_iter().rev() {
            if !self.exists(&m).await {
                self.mkdir(&m).await?;
            }
        }
        Ok(())
    }

    async fn size(&self, path: &str) -> Option<u64> {
        self.stat(path).await.ok().map(|e| e.size)
    }

    /// Ouvre un fichier pour lecture, à `offset`.
    async fn open_read(&self, path: &str, offset: u64) -> FsResult<Box<dyn Src + Send>>;
    /// Ouvre un fichier pour écriture.
    async fn open_write(&self, path: &str, offset: u64, append: bool) -> FsResult<Box<dyn Sink + Send>>;

    async fn ping(&self) -> FsResult<()>;
    async fn abort(&self) -> FsResult<()> {
        Ok(())
    }
    async fn shutdown(&self);
    async fn info(&self) -> ConnInfo;
    fn id(&self) -> String;
    fn is_closed(&self) -> bool;
    fn last_used(&self) -> i64;
    fn touch(&self);
    fn proto(&self) -> Proto;
    fn features(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Copie une source vers une destination, avec progression.
pub async fn copy<R: Src + ?Sized, W: Sink + ?Sized>(
    r: &mut R,
    w: &mut W,
    chunk: usize,
    mut on_progress: impl FnMut(usize),
) -> io::Result<u64> {
    let mut buf = vec![0u8; chunk.max(4096)];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        w.write(&buf[..n]).await?;
        total += n as u64;
        on_progress(n);
    }
    r.close().await?;
    w.finish().await?;
    Ok(total)
}

/// Connexion distante partagée, avec état d'occupation.
pub struct RemoteConn {
    pub fs: Arc<dyn RemoteFs>,
    busy: AtomicBool,
}

impl RemoteConn {
    pub fn new(fs: Arc<dyn RemoteFs>) -> RemoteConn {
        RemoteConn {
            fs,
            busy: AtomicBool::new(false),
        }
    }

    pub fn try_claim(self: &Arc<RemoteConn>) -> Option<Lease> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            Some(Lease {
                conn: self.clone(),
                done: false,
            })
        } else {
            None
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
}

/// Garde de connexion : rend le créneau libre à la destruction.
pub struct Lease {
    conn: Arc<RemoteConn>,
    done: bool,
}

impl Lease {
    pub fn fs(&self) -> &Arc<dyn RemoteFs> {
        &self.conn.fs
    }

    pub fn conn(&self) -> &Arc<RemoteConn> {
        &self.conn
    }

    pub fn manual_release(mut self) {
        self.conn.fs.touch();
        self.conn.busy.store(false, Ordering::Release);
        self.done = true;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if !self.done {
            self.conn.fs.touch();
            self.conn.busy.store(false, Ordering::Release);
        }
    }
}

pub type Boxed = std::pin::Pin<Box<dyn std::future::Future<Output = FsResult<Arc<dyn RemoteFs>>> + Send>>;

/// Parc de connexions à la FileZilla : N créneaux par site, réutilisation,
/// purge des connexions oisives.
pub struct Pool {
    pub max: usize,
    slots: RwLock<Vec<Option<Arc<RemoteConn>>>>,
    idle_reap_secs: i64,
    factory: Arc<dyn Fn() -> Boxed + Send + Sync>,
}

impl Pool {
    pub fn new<F>(max: usize, reap_secs: i64, factory: F) -> Pool
    where
        F: Fn() -> Boxed + Send + Sync + 'static,
    {
        Pool {
            max: max.max(1),
            slots: RwLock::new(Vec::new()),
            idle_reap_secs: reap_secs,
            factory: Arc::new(factory),
        }
    }

    pub fn live_count(&self) -> usize {
        self.slots
            .read()
            .unwrap()
            .iter()
            .filter(|s| s.as_ref().map(|c| !c.fs.is_closed()).unwrap_or(false))
            .count()
    }

    pub fn free_count(&self) -> usize {
        self.slots
            .read()
            .unwrap()
            .iter()
            .filter(|s| {
                s.as_ref()
                    .map(|c| !c.fs.is_closed() && !c.is_busy())
                    .unwrap_or(false)
            })
            .count()
    }

    /// Récupère une connexion libre, en crée une si nécessaire.
    pub async fn lease(&self, timeout: Duration) -> FsResult<Lease> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            // 1. un créneau vivant et libre ?
            let candidate = {
                let slots = self.slots.read().unwrap();
                slots
                    .iter()
                    .flatten()
                    .find(|c| !c.is_busy() && !c.fs.is_closed())
                    .cloned()
            };
            if let Some(c) = candidate {
                if let Some(l) = c.try_claim() {
                    return Ok(l);
                }
                continue;
            }
            // 2. on peut ouvrir une connexion de plus.
            let may_open = {
                let slots = self.slots.read().unwrap();
                slots
                    .iter()
                    .filter(|s| s.as_ref().map(|c| !c.fs.is_closed()).unwrap_or(false))
                    .count()
                    < self.max
            };
            if may_open {
                let fs = (self.factory)().await?;
                let conn = Arc::new(RemoteConn::new(fs));
                let lease = conn.try_claim().expect("connexion fraîche libre");
                {
                    let mut slots = self.slots.write().unwrap();
                    match slots.iter_mut().find(|s| s.is_none()) {
                        Some(slot) => *slot = Some(conn.clone()),
                        None => slots.push(Some(conn.clone())),
                    }
                }
                return Ok(lease);
            }
            if std::time::Instant::now() >= deadline {
                return Err(FsError::Timeout(
                    "aucune connexion libre pour ce site (limite atteinte)".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// Connexion de navigation, avec création à la demande.
    pub async fn browse(&self, timeout: Duration) -> FsResult<Arc<dyn RemoteFs>> {
        let l = self.lease(timeout).await?;
        let fs = l.fs().clone();
        l.manual_release();
        Ok(fs)
    }

    /// Ferme les connexions restées oisives trop longtemps.
    pub async fn reap_idle(&self) -> usize {
        self.reap_idle_now();
        self.slots
            .read()
            .unwrap()
            .iter()
            .flatten()
            .filter(|c| c.fs.is_closed())
            .count()
    }

    pub fn reap_idle_now(&self) {
        let slots: Vec<Arc<RemoteConn>> = self
            .slots
            .read()
            .unwrap()
            .iter()
            .flatten()
            .filter(|c| !c.is_busy())
            .cloned()
            .collect();
        for c in slots {
            let idle = now_ms() - c.fs.last_used();
            if idle > self.idle_reap_secs * 1000 || c.fs.is_closed() {
                let fs = c.fs.clone();
                tokio::spawn(async move {
                    fs.shutdown().await;
                });
            }
        }
    }

    pub fn close_all(&self) {
        let mut slots = self.slots.write().unwrap();
        for slot in slots.iter_mut() {
            if let Some(c) = slot.take() {
                if !c.fs.is_closed() {
                    let fs = c.fs.clone();
                    tokio::spawn(async move {
                        fs.shutdown().await;
                    });
                }
            }
        }
        slots.clear();
    }
}

/// Gestion des sites : un parc par site connecté.
pub struct SiteManager {
    pub pools: Mutex<std::collections::HashMap<String, Arc<Pool>>>,
}

impl SiteManager {
    pub fn new() -> SiteManager {
        SiteManager {
            pools: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<Pool>> {
        self.pools.lock().unwrap().get(id).cloned()
    }

    pub fn put(&self, id: &str, pool: Arc<Pool>) {
        self.pools.lock().unwrap().insert(id.to_string(), pool);
    }

    pub fn drop_pool(&self, id: &str) -> Option<Arc<Pool>> {
        let p = self.pools.lock().unwrap().remove(id);
        if let Some(p) = &p {
            p.close_all();
        }
        p
    }

    pub fn list_connected(&self) -> Vec<(String, usize, usize)> {
        self.pools
            .lock()
            .unwrap()
            .iter()
            .map(|(k, p)| (k.clone(), p.live_count(), p.max))
            .collect()
    }

    pub fn close_all(&self) {
        let mut m = self.pools.lock().unwrap();
        for p in m.values() {
            p.close_all();
        }
        m.clear();
    }
}

impl Default for SiteManager {
    fn default() -> Self {
        SiteManager::new()
    }
}

/// Filtre de nom de fichier.
pub fn apply_filter(list: &mut Vec<Entry>, pattern: &str, dirs_first: bool) {
    if !pattern.is_empty() {
        let p = pattern.to_lowercase();
        list.retain(|e| e.name.to_lowercase().contains(&p) || e.is_dir());
    }
    if dirs_first {
        list.sort_by_key(|e| (!e.is_dir(), e.name.to_lowercase()));
    }
}

/// Tri type façon FileZilla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SortKey {
    Name,
    Size,
    Kind,
    Time,
    Perm,
    Owner,
}

impl SortKey {
    pub fn parse(s: &str) -> SortKey {
        match s {
            "size" => SortKey::Size,
            "type" => SortKey::Kind,
            "modified" | "time" => SortKey::Time,
            "perm" | "permissions" => SortKey::Perm,
            "user" | "owner" => SortKey::Owner,
            _ => SortKey::Name,
        }
    }
}

pub fn sort_entries(v: &mut Vec<Entry>, key: SortKey, desc: bool, dirs_first: bool) {
    v.sort_by(|a, b| {
        if dirs_first {
            match (a.is_dir(), b.is_dir()) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {}
            }
        }
        let ord = match key {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Kind => a.kind.label().cmp(b.kind.label()),
            SortKey::Time => a.mtime.cmp(&b.mtime),
            SortKey::Perm => a.mode.cmp(&b.mode),
            SortKey::Owner => a.user.cmp(&b.user),
        };
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });
}

/// Récapitulatif d'un listing pour la barre de statut.
pub fn summary(l: &Listing) -> Value {
    serde_json::json!({
        "files": l.entries.iter().filter(|e| !e.is_dir()).count(),
        "dirs": l.entries.iter().filter(|e| e.is_dir()).count(),
        "bytes": l.entries.iter().map(|e| e.size).sum::<u64>(),
        "source": l.source,
    })
}

/// Un chemin vide devient « / ».
pub fn norm(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else {
        path.to_string()
    }
}

/// Join distant.
pub fn join(dir: &str, name: &str) -> String {
    remote_join(dir, name)
}

/// Remonter d'un cran.
pub fn parent(dir: &str) -> String {
    remote_parent(dir)
}

pub fn now() -> i64 {
    now_ms()
}
