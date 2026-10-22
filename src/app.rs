//! État applicatif : sites, parcs de connexions, file d'attente, réglages.

use crate::fs::{FsError, FsResult, Lease, Pool, RemoteFs, SiteManager};
use crate::models::RemoteOp;
use crate::ftp;
use crate::hub::{Event, LogHub};
use crate::models::{
    Bookmark, ConnInfo, Direction, Entry, EntryKind, Listing, LoginType, Proto, Site,
    TlsLevel, TransferJob,
};
use crate::settings::Settings;
use crate::sftp;
use crate::store::Store;
use crate::transfers::Transfers;
use crate::util::{now_ms, remote_join, short_id};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Application Zilla-RS.
pub struct App {
    pub dir: PathBuf,
    pub store: RwLock<Store>,
    pub hub: Arc<LogHub>,
    pub pools: SiteManager,
    pub transfers: Arc<Transfers>,
    pub started: i64,
    pub token: String,
    pub version: &'static str,
    /// Sites crés par la connexion rapide (non enregistrés).
    ephemeral: Mutex<HashMap<String, Site>>,
    running: AtomicBool,
}

impl App {
    pub fn new(dir: PathBuf, version: &'static str) -> std::io::Result<Arc<App>> {
        let store = Store::load(&dir)?;
        let hub = Arc::new(LogHub::new(store.settings.logs.memory_lines));
        hub.set_verbose(store.settings.logs.level == "verbose");
        if store.settings.logs.to_file {
            let p = if store.settings.logs.file_path.is_empty() {
                dir.join("logs").join("zilla.log")
            } else {
                PathBuf::from(&store.settings.logs.file_path)
            };
            hub.set_file(Some(&p));
        }
        let transfers = Transfers::new(hub.clone(), store.settings.transfers.chunk_size);
        let app = Arc::new(App {
            dir,
            store: RwLock::new(store),
            hub,
            pools: SiteManager::new(),
            transfers,
            started: now_ms(),
            token: crate::api::new_token(),
            version,
            ephemeral: Mutex::new(HashMap::new()),
            running: AtomicBool::new(false),
        });
        app.hub.banner(version, &app.dir);
        let clone = app.clone();
        tokio::spawn(async move {
            clone.transfers.clone().run(clone.clone()).await;
        });
        let clone = app.clone();
        tokio::spawn(async move {
            App::background(clone).await;
        });
        Ok(app)
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Tâche d'arrière-plan : keep-alive, purge des connexions oisives,
    /// sauvegarde de la file.
    async fn background(app: Arc<App>) {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let (reap, idle_secs) = {
                let s = app.store.read().unwrap();
                (
                    s.settings.net.keepalive_idle.max(15) as i64,
                    s.settings.net.keepalive_idle.max(30) as i64 + 30,
                )
            };
            let _ = reap;
            let list = app.pools.list_connected();
            for (id, live, max) in list {
                if live == 0 {
                    app.pools.drop_pool(&id);
                    continue;
                }
                let _ = max;
                if let Some(p) = app.pools.get(&id) {
                    p.reap_idle_now();
                    // keepalive : on ping une connexion inactive si l'utilisateur
                    // ne l'a pas désactivé.
                    if let Ok(l) = p.lease(Duration::from_millis(10)).await {
                        if now_ms() - l.fs().last_used() > idle_secs * 1000 {
                            let _ = l.fs().ping().await;
                        }
                        drop(l);
                    }
                }
            }
            let jobs = app.transfers.snapshot();
            let mut s = app.store.write().unwrap();
            s.save_queue_jobs(&jobs);
        }
    }

    /// Site connu (enregistré ou connexion rapide).
    pub fn site(&self, id: &str) -> Option<Site> {
        if let Some(s) = self.store.read().unwrap().find_site(id) {
            return Some(s.clone());
        }
        self.ephemeral.lock().unwrap().get(id).cloned()
    }

    pub fn put_ephemeral(&self, site: Site) -> String {
        let id = site.id.clone();
        self.ephemeral.lock().unwrap().insert(id.clone(), site);
        id
    }

    pub fn forget_ephemeral(&self, id: &str) {
        self.ephemeral.lock().unwrap().remove(id);
    }

    pub fn settings(&self) -> Settings {
        self.store.read().unwrap().settings.clone()
    }

    /// Parc de connexions d'un site, créé à la demande.
    pub async fn pool_for(&self, site: &Site, settings: &Settings) -> FsResult<Arc<Pool>> {
        if let Some(p) = self.pools.get(&site.id) {
            if self
                .ephemeral
                .lock()
                .unwrap()
                .get(&site.id)
                .map(|s| s.host == site.host && s.port == site.port)
                .unwrap_or(true)
            {
                return Ok(p);
            }
        }
        let max = if site.proto == Proto::Sftp {
            site.max_connections.unwrap_or(1).clamp(1, 4)
        } else {
            site.max_connections.unwrap_or(settings.ftp.max_connections).clamp(1, 32)
        };
        let reap = settings.net.keepalive_idle.max(30) as i64 + 120;
        let hub = self.hub.clone();
        let s = site.clone();
        let st = settings.clone();
        let dir = self.dir.clone();
        let pool = Arc::new(Pool::new(max, reap, move || {
            let hub = hub.clone();
            let s = s.clone();
            let st = st.clone();
            let dir = dir.clone();
            let fut: crate::fs::Boxed = Box::pin(async move {
                match s.proto {
                    Proto::Sftp => sftp::make_fs(&s, &st, &hub, &dir).await,
                    _ => ftp::make_fs(&s, &st, &hub).await,
                }
            });
            fut
        }));
        self.pools.put(&site.id, pool.clone());
        Ok(pool)
    }

    /// Se connecte à un site (validation par une commande PWD).
    pub async fn connect(&self, site: &Site) -> FsResult<ConnInfo> {
        let settings = self.settings();
        if site.host.trim().is_empty() {
            return Err(FsError::Other("le nom d'hôte est vide".into()));
        }
        if site.disabled {
            return Err(FsError::Other("ce site est désactivé".into()));
        }
        self.hub.status(format!(
            "Connexion à {}…",
            if site.user.is_empty() {
                site.host.clone()
            } else {
                format!("{}@{}", site.user, site.host)
            }
        ));
        let _t = std::time::Instant::now();
        let pool = self.pool_for(site, &settings).await?;
        let fs = pool.browse(Duration::from_secs(settings.net.connect_timeout + 10)).await?;
        let info = fs.info().await;
        drop(fs);
        let mut info = info;
        info.connections = pool.live_count();
        info.max_connections = pool.max;
        // on mémorise le chemin initial si vide
        let start = if site.remote_dir.is_empty() || site.remote_dir == "/" {
            "/".to_string()
        } else {
            site.remote_dir.clone()
        };
        self.hub.info("connect", format!("Répertoire distant de départ : {}", start));
        self.hub.publish(Event::dirty("connections"));
        Ok(info)
    }

    /// Connexion rapide à partir de la barre d'outils.
    pub async fn quickconnect(
        &self,
        host: &str,
        user: &str,
        pass: &str,
        port: Option<u16>,
        proto: &str,
    ) -> FsResult<(String, ConnInfo)> {
        self.quickconnect_ex(host, user, pass, port, proto, None, None).await
    }

    /// Connexion rapide élargie : niveau TLS et mode passif/actif explicites.
    pub async fn quickconnect_ex(
        &self,
        host: &str,
        user: &str,
        pass: &str,
        port: Option<u16>,
        proto: &str,
        tls: Option<&str>,
        passive: Option<bool>,
    ) -> FsResult<(String, ConnInfo)> {
        let mut site = Site::default();
        site.id = format!("qc-{}", short_id());
        site.name = if user.is_empty() {
            host.to_string()
        } else {
            format!("{}@{}", user, host)
        };
        site.host = host.trim().to_string();
        site.proto = Proto::parse(proto);
        site.port = port.or(if site.proto == Proto::Ftp { None } else { Some(site.proto.default_port()) });
        site.user = user.to_string();
        site.password = pass.to_string();
        site.login = if user.is_empty() {
            LoginType::Anonymous
        } else {
            LoginType::Normal
        };
        site.tls = match tls {
            Some(t) => TlsLevel::parse(t),
            None if site.proto.is_tls() => TlsLevel::IfAvailable,
            None => TlsLevel::Plain,
        };
        if let Some(p) = passive {
            site.passive = Some(p);
        }
        self.push_recent_host(site.display());
        let id = self.put_ephemeral(site.clone());
        match self.connect(&site).await {
            Ok(info) => {
                self.hub.success("general", format!("Connecté à {}", site.display()));
                Ok((id, info))
            }
            Err(e) => {
                self.forget_ephemeral(&id);
                self.hub.err("general", format!("Échec de connexion : {e}"));
                Err(e)
            }
        }
    }

    pub fn disconnect(&self, id: &str) {
        if let Some(pool) = self.pools.drop_pool(id) {
            let _ = pool;
            self.hub.status("Déconnecté.");
        }
        self.transfers.cancel_site(id);
        self.forget_ephemeral(id);
        self.hub.publish(Event::dirty("connections"));
    }

    pub fn is_connected(&self, id: &str) -> bool {
        self.pools
            .get(id)
            .map(|p| p.live_count() > 0)
            .unwrap_or(false)
    }

    /// Résumé d'état pour l'interface.
    pub fn status_json(&self) -> serde_json::Value {
        let conns: Vec<serde_json::Value> = self
            .pools
            .list_connected()
            .into_iter()
            .filter_map(|(id, live, max)| {
                if live == 0 {
                    return None;
                }
                let site = self.site(&id);
                Some(serde_json::json!({
                    "site_id": id,
                    "name": site.as_ref().map(|s| s.full_name()).unwrap_or_default(),
                    "host": site.as_ref().map(|s| s.host.clone()).unwrap_or_default(),
                    "proto": site.as_ref().map(|s| s.proto.name()).unwrap_or("FTP"),
                    "live": live,
                    "max": max,
                    "connected": live > 0,
                }))
            })
            .collect();
        serde_json::json!({
            "version": self.version,
            "uptime": (now_ms() - self.started) / 1000,
            "connections": conns,
            "transfers": self.transfers.stats(),
            "settings": self.settings(),
            "logs": self.hub.len(),
            "dir": self.dir.to_string_lossy(),
            "token": self.token,
        })
    }

    pub async fn remote_list(&self, site_id: &str, path: &str) -> FsResult<Listing> {
        let site = self.site(site_id).ok_or_else(|| FsError::Other("site inconnu".into()))?;
        let settings = self.settings();
        let pool = self.pool_for(&site, &settings).await?;
        let lease: Lease = pool.lease(Duration::from_secs(settings.net.connect_timeout + 15)).await?;
        let l = lease.fs().list(path).await;
        match &l {
            Ok(list) => {
                self.push_recent_remote(list.path.clone());
                self.hub.trace("file", format!("Dossier distant : {} ({} entrées)", list.path, list.entries.len()));
            }
            Err(e) => self.hub.warn("file", format!("Listage de {path} : {e}")),
        }
        l
    }

    pub fn local_list(&self, path: &str, show_hidden: bool) -> FsResult<Listing> {
        let p = crate::util::local_normalize(path);
        let s = self.settings();
        let l = crate::local::list(&p, show_hidden || s.general.show_hidden_local, s.transfers.follow_symlinks)?;
        self.push_recent_local(l.path.clone());
        Ok(l)
    }

    pub async fn remote_op(&self, site_id: &str, op: RemoteOp) -> FsResult<()> {
        let site = self.site(site_id).ok_or_else(|| FsError::Other("site inconnu".into()))?;
        let settings = self.settings();
        let pool = self.pool_for(&site, &settings).await?;
        let lease = pool.lease(Duration::from_secs(settings.net.connect_timeout + 15)).await?;
        let r = match &op {
            RemoteOp::Mkdir(p) => {
                let r = lease.fs().mkdir(p).await;
                self.hub.info("file", format!("Création du dossier {}", p));
                r
            }
            RemoteOp::Rmdir(p) => {
                let r = lease.fs().rmdir(p).await;
                self.hub.info("file", format!("Suppression du dossier {}", p));
                r
            }
            RemoteOp::Remove(p) => {
                let r = lease.fs().remove(p).await;
                self.hub.info("file", format!("Suppression de {}", p));
                r
            }
            RemoteOp::Rename { from, to } => {
                let r = lease.fs().rename(from, to).await;
                self.hub.info("file", format!("Renommage {} → {}", from, to));
                r
            }
            RemoteOp::Chmod { path, mode } => {
                let r = lease.fs().chmod(path, *mode).await;
                self.hub.info("file", format!("Permissions de {} : {:o}", path, mode));
                r
            }
            RemoteOp::Chown { path, user } => {
                let r = lease.fs().chown(path, user).await;
                self.hub.info("file", format!("Propriétaire de {} : {}", path, user));
                r
            }
            RemoteOp::SetTimes { path, mtime } => {
                let r = lease.fs().set_mtime(path, *mtime).await;
                self.hub.info("file", format!("Horodatage de {} mis à jour", path));
                r
            }
            RemoteOp::Touch(p) => {
                let r = lease.fs().open_write(p, 0, false).await;
                match r {
                    Ok(mut s) => {
                        let _ = s.finish().await;
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
        };
        self.hub.publish(Event::dirty("remote"));
        r
    }

    /// Ajoute des fichiers distants à la file de téléchargement.
    pub async fn enqueue_download(
        &self,
        site_id: &str,
        paths: &[String],
        local_dir: &str,
        dirs_too: bool,
    ) -> FsResult<Vec<TransferJob>> {
        let site = self.site(site_id).ok_or_else(|| FsError::Other("site inconnu".into()))?;
        let settings = self.settings();
        let pool = self.pool_for(&site, &settings).await?;
        let mut jobs = Vec::new();
        for p in paths {
            let lease = pool.lease(Duration::from_secs(settings.net.connect_timeout + 15)).await?;
            let fs = lease.fs().clone();
            let st = match fs.stat(p).await {
                Ok(e) => e,
                Err(e) => {
                    self.hub.err("file", format!("{} : {e}", p));
                    continue;
                }
            };
            if st.kind == EntryKind::Dir {
                if dirs_too {
                    let rel = local_join(local_dir, &st.name);
                    let mut stack = vec![(p.clone(), rel.clone())];
                    while let Some((rdir, ldir)) = stack.pop() {
                        let l = fs.list(&rdir).await?;
                        let _ = tokio::fs::create_dir_all(&ldir).await;
                        for e in l.entries {
                            let rp = remote_join(&rdir, &e.name);
                            let lp = if e.kind == EntryKind::Dir {
                                remote_join(&ldir, &e.name)
                            } else {
                                crate::util::local_normalize(&remote_join(&ldir, &e.name))
                                    .to_string_lossy()
                                    .to_string()
                            };
                            if e.kind == EntryKind::Dir {
                                stack.push((rp, lp));
                            } else {
                                match self.plan_upload(site_id, &site, &rp, &lp, e.size, Direction::Down, &settings) {
                                    Some(Ok(j)) => jobs.push(j),
                                    Some(Err(e)) => self.hub.warn("transfers", e.to_string()),
                                    None => {}
                                }
                            }
                        }
                    }
                } else {
                    continue;
                }
            } else {
                let local = remote_join(local_dir, &st.name);
                let local = crate::util::local_normalize(&local).to_string_lossy().to_string();
                if let Some(j) = self.plan_upload(site_id, &site, p, &local, st.size, Direction::Down, &settings) {
                    jobs.push(j?);
                }
            }
            drop(lease);
        }
        let n = jobs.len();
        self.transfers.add_many(jobs);
        self.hub.success("transfers", format!("{} article(s) ajouté(s) à la file.", n));
        self.transfers.notify();
        self.hub.publish(Event::dirty("queue"));
        Ok(vec![])
    }

    /// Ajoute des fichiers locaux à la file d'envoi.
    pub async fn enqueue_upload(
        &self,
        site_id: &str,
        paths: &[String],
        remote_dir: &str,
        dirs_too: bool,
    ) -> FsResult<Vec<TransferJob>> {
        let site = self.site(site_id).ok_or_else(|| FsError::Other("site inconnu".into()))?;
        let settings = self.settings();
        let pool = self.pool_for(&site, &settings).await?;
        let mut jobs = Vec::new();
        for p in paths {
            let path = PathBuf::from(p);
            let md = tokio::fs::metadata(&path)
                .await
                .map_err(|e| FsError::Io(format!("{} : {e}", path.display())))?;
            if md.is_dir() {
                if !dirs_too {
                    continue;
                }
                let base = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let target_dir = remote_join(remote_dir, &base);
                let mut stack: Vec<(PathBuf, String)> = vec![(path.clone(), target_dir)];
                while let Some((dir, rdir)) = stack.pop() {
                    // crée le dossier distant, puis les entrées
                    {
                        let lease = pool.lease(Duration::from_secs(15)).await?;
                        if !lease.fs().exists(&rdir).await {
                            let _ = lease.fs().mkdir(&rdir).await;
                        }
                        drop(lease);
                    }
                    let mut rd = match tokio::fs::read_dir(&dir).await {
                        Ok(r) => r,
                        Err(e) => {
                            self.hub.warn("file", format!("{} : {e}", dir.display()));
                            continue;
                        }
                    };
                    while let Ok(Some(de)) = rd.next_entry().await {
                        let fp = de.path();
                        let m = match tokio::fs::metadata(&fp).await {
                            Ok(m) => m,
                            Err(_) => continue,
                        };
                        let name = de.file_name().to_string_lossy().to_string();
                        let rp = remote_join(&rdir, &name);
                        if m.is_dir() {
                            stack.push((fp.clone(), rp));
                        } else {
                            if let Some(j) = self.plan_upload(site_id, &site, &rp, &fp.to_string_lossy(), m.len(), Direction::Up, &settings) {
                                jobs.push(j?);
                            }
                        }
                    }
                }
            } else {
                let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
                let rp = remote_join(remote_dir, &name);
                if let Some(j) = self.plan_upload(site_id, &site, &rp, &path.to_string_lossy(), md.len(), Direction::Up, &settings) {
                    jobs.push(j?);
                }
            }
        }
        let n = jobs.len();
        self.transfers.add_many(jobs);
        self.hub.success("transfers", format!("{} fichier(s) ajouté(s) à la file.", n));
        self.transfers.notify();
        self.hub.publish(Event::dirty("queue"));
        Ok(vec![])
    }

    /// Fabrique une ligne de transfert en respectant la politique d'écrasement.
    fn plan_upload(
        &self,
        site_id: &str,
        site: &Site,
        remote_or_local_path: &str,
        local_path: &str,
        size: u64,
        dir: Direction,
        settings: &Settings,
    ) -> Option<FsResult<TransferJob>> {
        let (remote, local) = match dir {
            Direction::Up => (remote_or_local_path.to_string(), local_path.to_string()),
            Direction::Down => (remote_or_local_path.to_string(), local_path.to_string()),
        };
        // Politique d'écrasement pour la cible.
        let target = if dir == Direction::Up {
            remote.clone()
        } else {
            local.clone()
        };
        match settings.transfers.overwrite.as_str() {
            "skip" => {
                let exists = if dir == Direction::Down {
                    Path::new(&target).exists()
                } else {
                    false
                };
                if exists {
                    self.hub.info("transfers", format!("{} existe déjà, ignoré.", target));
                    return None;
                }
            }
            "newer" => {
                // on laissera la reprise gérer ; sinon on remplace.
            }
            "rename" | "renumber" => {
                if dir == Direction::Down && Path::new(&target).exists() {
                    let mut i = settings.transfers.rename_suffix as usize;
                    let mut candidate = target.clone();
                    while Path::new(&candidate).exists() {
                        candidate = insert_suffix(&target, i as usize);
                        i += 1;
                        if i > 999 {
                            break;
                        }
                    }
                    return Some(Ok(TransferJob::new(
                        site_id.into(),
                        site.full_name(),
                        dir,
                        remote,
                        candidate,
                        size,
                    )));
                }
            }
            _ => {}
        }
        Some(Ok(TransferJob::new(
            site_id.into(),
            site.full_name(),
            dir,
            remote,
            local,
            size,
        )))
    }

    pub fn push_recent_host(&self, h: String) {
        let mut s = self.store.write().unwrap();
        s.push_recent_host(h);
    }
    pub fn push_recent_local(&self, p: String) {
        let mut s = self.store.write().unwrap();
        s.push_recent_local(p);
    }
    pub fn push_recent_remote(&self, p: String) {
        let mut s = self.store.write().unwrap();
        s.push_recent_remote(p);
    }

    pub fn add_bookmark(&self, b: Bookmark) -> Bookmark {
        let mut s = self.store.write().unwrap();
        s.add_bookmark(b)
    }

    pub fn remove_bookmark(&self, id: &str) {
        let mut s = self.store.write().unwrap();
        s.remove_bookmark(id);
        self.hub.publish(Event::dirty("bookmarks"));
    }

    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.store.read().unwrap().bookmarks.clone()
    }

    pub fn sites(&self) -> Vec<Site> {
        self.store.read().unwrap().sites.clone()
    }

    pub fn upsert_site(&self, site: Site) -> Site {
        let mut s = self.store.write().unwrap();
        let site = s.upsert_site(site);
        self.hub.publish(Event::dirty("sites"));
        site
    }

    pub fn delete_site(&self, id: &str) {
        let mut s = self.store.write().unwrap();
        s.delete_site(id);
        self.hub.publish(Event::dirty("sites"));
    }

    pub fn copy_site(&self, id: &str) -> Option<Site> {
        let mut s = self.store.write().unwrap();
        let r = s.duplicate_site(id);
        self.hub.publish(Event::dirty("sites"));
        r
    }

    pub fn logs(&self, since: i64) -> (i64, Vec<crate::models::LogLine>) {
        self.hub.snapshot(since, 800)
    }

    /// Recherche dans le disque ou le serveur.
    pub fn local_search(&self, root: &str, pattern: &str, max: usize) -> Vec<Entry> {
        let p = crate::util::local_normalize(root);
        let s = self.settings();
        crate::local::search(&p, pattern, max, s.transfers.follow_symlinks, true)
    }

    pub async fn remote_search(
        &self,
        site_id: &str,
        root: &str,
        pattern: &str,
        max: usize,
    ) -> FsResult<Vec<Entry>> {
        let site = self.site(site_id).ok_or_else(|| FsError::Other("site inconnu".into()))?;
        let settings = self.settings();
        let pool = self.pool_for(&site, &settings).await?;
        let lease = pool.lease(Duration::from_secs(20)).await?;
        let fs: Arc<dyn RemoteFs> = lease.fs().clone();
        let mut out = Vec::new();
        let mut stack = vec![root.to_string()];
        let pat = pattern.to_lowercase();
        let mut n = 0;
        while let Some(d) = stack.pop() {
            if n >= max {
                break;
            }
            let l = match fs.list(&d).await {
                Ok(l) => l,
                Err(_) => continue,
            };
            for e in l.entries {
                let rp = remote_join(&d, &e.name);
                if e.is_dir() {
                    if stack.len() < 200 {
                        stack.push(rp);
                    }
                } else if pat.is_empty() || e.name.to_lowercase().contains(&pat) {
                    let mut en = e.clone();
                    en.path = Some(rp);
                    out.push(en);
                    n += 1;
                    if n >= max {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    pub async fn site_infos(&self) -> Vec<ConnInfo> {
        let mut out = Vec::new();
        for (id, _, _) in self.pools.list_connected() {
            if let Some(p) = self.pools.get(&id) {
                if let Ok(fs) = p.browse(Duration::from_millis(30)).await {
                    let mut i = fs.info().await;
                    i.connections = p.live_count();
                    i.max_connections = p.max;
                    out.push(i);
                }
            }
        }
        out
    }

    /// Charge une archive d'en-tête d'API de sites.
    pub fn import_sites(&self, xml: &str) -> Result<usize, String> {
        let list = crate::store::import_sitemanager_xml(xml)?;
        let mut n = 0;
        {
            let mut s = self.store.write().unwrap();
            for mut site in list {
                if let Some(old) = s
                    .sites
                    .iter()
                    .find(|x| x.host == site.host && x.name == site.name)
                {
                    site.id = old.id.clone();
                }
                let up = s.upsert_site(site);
                n += 1;
                let _ = up;
            }
            let _ = s.save_sites();
        }
        self.hub.publish(Event::dirty("sites"));
        Ok(n)
    }

    pub fn export_sites(&self) -> String {
        crate::store::export_sitemanager_xml(&self.sites())
    }

    pub fn shutdown(&self) {
        self.running.store(false, Ordering::Relaxed);
        self.transfers.stop();
        self.pools.close_all();
        let jobs = self.transfers.snapshot();
        let mut s = self.store.write().unwrap();
        s.save_queue_jobs(&jobs);
    }
}

fn local_join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "/" {
        format!("/{}", name)
    } else {
        let d = dir.trim_end_matches('/');
        format!("{}/{}", d, name)
    }
}

/// `nom.txt` + 2 → `nom (2).txt`
fn insert_suffix(path: &str, n: usize) -> String {
    let p = Path::new(path);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|s| format!(".{}", s.to_string_lossy())).unwrap_or_default();
    let parent = p.parent().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let sep = if parent.is_empty() { "" } else { "/" };
    format!("{parent}{sep}{stem} ({n}){ext}")
}

/// Réglage rapide d'une file d'attente sauvegardée : les jobs interrompus
/// redeviennent « en attente ».
pub fn restore_queue(jobs: &mut Vec<TransferJob>) {
    for j in jobs.iter_mut() {
        if j.state == crate::models::JobState::Active {
            j.state = crate::models::JobState::Waiting;
        }
    }
}

pub fn default_site() -> Site {
    Site {
        id: short_id(),
        ..Default::default()
    }
}
