//! Client SFTP (SSH) basé sur russh + russh-sftp.

use crate::fs::{FsError, FsResult, RemoteFs, Sink, Src};
use crate::hub::LogHub;
use crate::models::{ConnInfo, Entry, EntryKind, Listing, LoginType, Proto, Site};
use crate::settings::Settings;
use crate::util::{now_ms, remote_join, short_id};
use async_trait::async_trait;
use russh::keys::PublicKeyOrCertificate;

impl From<russh::Error> for FsError {
    fn from(e: russh::Error) -> Self {
        FsError::Io(format!("SSH : {e}"))
    }
}
use russh::client as sshc;
use russh::keys::{load_secret_key, HashAlg};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[derive(Debug)]
pub struct SshError(pub String);

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SshError {}

impl From<russh::Error> for SshError {
    fn from(e: russh::Error) -> SshError {
        SshError(e.to_string())
    }
}

/// Fiches d'hôtes connues, au format JSON (chemin `known_hosts.json`).
pub struct KnownHosts {
    path: PathBuf,
    map: Mutex<std::collections::HashMap<String, String>>,
}

impl KnownHosts {
    pub fn open(dir: &std::path::Path) -> KnownHosts {
        let path = dir.join("known_hosts.json");
        let map = crate::store::read_json(&path)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        KnownHosts {
            path,
            map: Mutex::new(map),
        }
    }

    fn key(host: &str, port: u16, algo: &str) -> String {
        if port == 22 {
            format!("{host}|{algo}")
        } else {
            format!("{host}:{port}|{algo}")
        }
    }

    pub fn check(&self, host: &str, port: u16, algo: &str, fp: &str) -> (bool, Option<String>) {
        let m = self.map.lock().unwrap();
        let k = Self::key(host, port, algo);
        match m.get(&k) {
            Some(known) if known == fp => (true, None),
            Some(known) => (false, Some(known.clone())),
            None => (false, None),
        }
    }

    pub fn trust(&self, host: &str, port: u16, algo: &str, fp: &str) {
        let mut m = self.map.lock().unwrap();
        m.insert(Self::key(host, port, algo), fp.to_string());
        let dump = serde_json::to_string_pretty(&*m).unwrap_or_default();
        let _ = crate::store::atomic_write(&self.path, &dump);
    }
}

/// Handler SSH : journalise la clé hôte et la bannière.
struct Handler {
    #[allow(dead_code)]
    hub: Arc<LogHub>,
    host: String,
    port: u16,
    known: Arc<KnownHosts>,
    ask: bool,
    fp: Arc<Mutex<String>>,
}

impl sshc::Handler for Handler {
    type Error = SshError;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, SshError> {
        let (algo, fp) = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => (
                key.algorithm().as_str().to_string(),
                format!("{}", key.fingerprint(HashAlg::Sha256)),
            ),
            PublicKeyOrCertificate::Certificate(c) => {
                (c.public_key().algorithm().as_str().to_string(), format!("{c:?}"))
            }
        };
        *self.fp.lock().unwrap() = fp.clone();
        self.hub
            .status(format!("Clé hôte du serveur : {algo} {fp}"));
        match self.known.check(&self.host, self.port, &algo, &fp) {
            (true, _) => {
                self.hub.trace("general", "Clé hôte connue et validée.");
                Ok(true)
            }
            (false, Some(previous)) => {
                self.hub.err(
                    "general",
                    format!(
                        "Attention : la clé hôte a changé ! Enregistrée {previous}, reçue {fp}. Connexion interrompue."
                    ),
                );
                Ok(false)
            }
            (false, None) => {
                if self.ask {
                    // On accepte pour la première fois, comme FileZilla qui
                    // demande confirmation ; la clé est désormais mémorisée.
                    self.hub.warn(
                        "general",
                        "Clé hôte inconnue, acceptée pour cette session et mémorisée.",
                    );
                } else {
                    self.hub.warn("general", "Clé hôte inconnue, acceptée (vérification désactivée).");
                }
                self.known.trust(&self.host, self.port, &algo, &fp);
                Ok(true)
            }
        }
    }

    async fn auth_banner(&mut self, banner: &str, _s: &mut sshc::Session) -> Result<(), SshError> {
        if !banner.trim().is_empty() {
            self.hub.info("server", format!("Bannière : {}", banner.trim()));
        }
        Ok(())
    }
}

/// Système de fichiers SFTP.
pub struct SftpFs {
    sftp: Arc<SftpSession>,
    handle: tokio::sync::Mutex<Option<sshc::Handle<Handler>>>,
    info: RwLock<ConnInfo>,
    #[allow(dead_code)]
    hub: Arc<LogHub>,
    id: String,
    closed: AtomicBool,
    last_used: AtomicI64,
    #[allow(dead_code)]
    root: RwLock<String>,
}

fn attr_to_entry(name: String, a: &FileAttributes) -> Entry {
    let kind = match a.permissions.unwrap_or(0) & 0o170000 {
        0o040000 => EntryKind::Dir,
        0o120000 => EntryKind::Symlink,
        _ if a.permissions.is_none() => EntryKind::File,
        _ => EntryKind::File,
    };
    Entry {
        name: name.clone(),
        kind,
        size: a.size.unwrap_or(0),
        mtime: a.mtime.map(|t| t as i64),
        mode: a.permissions.map(|p| p & 0o7777),
        user: a.user.clone().or_else(|| a.uid.map(|u| u.to_string())),
        group: a.group.clone().or_else(|| a.gid.map(|g| g.to_string())),
        link: None,
        hidden: name.starts_with('.'),
        path: None,
    }
}

impl SftpFs {
    pub async fn connect(
        site: &Site,
        settings: &Settings,
        hub: &Arc<LogHub>,
        known: Arc<KnownHosts>,
    ) -> FsResult<SftpFs> {
        let host = site.host.clone();
        let port = site.port();
        hub.status(format!("Connexion SFTP à {host}:{port}…"));
        let cfg = Arc::new(sshc::Config {
            ..Default::default()
        });
        let _keepalive = if site.disable_keepalive { 0 } else { settings.net.keepalive_idle.max(5) };
        let handler = Handler {
            hub: hub.clone(),
            host: host.clone(),
            port,
            known: known.clone(),
            ask: settings.sftp.ask_hostkey,
            fp: Arc::new(Mutex::new(String::new())),
        };
        let addrs = (host.as_str(), port);
        let mut handle = match sshc::connect(cfg, addrs, handler).await {
            Ok(h) => h,
            Err(e) => {
                hub.err("general", format!("Échec de la connexion SSH : {e}"));
                return Err(FsError::Io(format!("SSH : {e}")));
            }
        };
        let fp = String::new();
        let user = if site.user.is_empty() {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".into())
        } else {
            site.user.clone()
        };

        let mut authed = false;
        // Méthode par clé.
        let keypath = if !site.keyfile.is_empty() {
            Some(site.keyfile.clone())
        } else if !settings.sftp.identity_file.is_empty() {
            Some(settings.sftp.identity_file.clone())
        } else {
            None
        };
        if let Some(p) = keypath {
            match load_secret_key(&p, None) {
                Ok(key) => {
                    let hash = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
                    match handle.authenticate_publickey(&user, russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await {
                        Ok(r) if r.success() => {
                            authed = true;
                            hub.success("general", "Authentification par clé réussie.");
                        }
                        Ok(_) => {
                            hub.warn("general", "Clé refusée par le serveur.");
                        }
                        Err(e) => {
                            hub.warn("general", format!("Clé inutilisable ({p}) : {e}"));
                        }
                    }
                }
                Err(e) => hub.warn("general", format!("Impossible de lire la clé {p} : {e}")),
            }
        }
        if !authed && site.login != LoginType::KeyOnly {
            match handle
                .authenticate_password(&user, site.password.clone())
                .await
            {
                Ok(r) if r.success() => {
                    authed = true;
                    hub.success("general", "Authentification réussie.");
                }
                Ok(_) => {
                    hub.err("general", "Mot de passe refusé.");
                }
                Err(e) => hub.err("general", format!("Échec d'authentification : {e}")),
            }
        }
        if !authed {
            return Err(FsError::Auth("toutes les méthodes d'authentification ont échoué".into()));
        }

        let channel = handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let stream = channel.into_stream();
        let sftp = Arc::new(
            SftpSession::new(stream)
                .await
                .map_err(|e| FsError::Protocol(format!("SFTP : {e}")))?,
        );
        let cwd = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into());
        hub.success("general", format!("Connecté à {host}:{port} (SFTP), répertoire : {cwd}"));

        let info = ConnInfo {
            site_id: site.id.clone(),
            proto: Proto::Sftp,
            host: host.clone(),
            port,
            user: user.clone(),
            tls: true,
            tls_version: "SSH".into(),
            cipher: String::new(),
            local_addr: String::new(),
            remote_addr: String::new(),
            banner: String::new(),
            server_type: "SFTP".into(),
            active: false,
            connections: 1,
            max_connections: 1,
            since: now_ms(),
            hostkey_fp: fp,
            features: vec!["SFTP".into()],
        };
        Ok(SftpFs {
            sftp,
            handle: tokio::sync::Mutex::new(Some(handle)),
            info: RwLock::new(info),
            hub: hub.clone(),
            id: short_id(),
            closed: AtomicBool::new(false),
            last_used: AtomicI64::new(now_ms()),
            root: RwLock::new(cwd),
        })
    }

    fn meta_entry(&self, name: String, m: &FileAttributes) -> Entry {
        let mut e = attr_to_entry(name, m);
        if e.kind == EntryKind::File && e.size == 0 {
            e.size = 0;
        }
        e
    }

    async fn attrs(&self, path: &str) -> FsResult<FileAttributes> {
        match self.sftp.metadata(path).await {
            Ok(m) => Ok(m),
            Err(e) => Err(FsError::Other(format!("{} : {e}", path))),
        }
    }
}

/// Source SFTP (lecture séquentielle avec position).
struct SftpSrc {
    file: Option<russh_sftp::client::fs::File>,
    eof: bool,
}

#[async_trait]
impl Src for SftpSrc {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.eof {
            return Ok(0);
        }
        let Some(f) = self.file.as_mut() else {
            return Ok(0);
        };
        match f.read(buf).await {
            Ok(0) => {
                self.eof = true;
                Ok(0)
            }
            Ok(n) => Ok(n),
            Err(e) => Err(e),
        }
    }

    async fn close(&mut self) -> io::Result<()> {
        self.eof = true;
        if let Some(f) = self.file.take() {
            let _ = f.close().await;
        }
        Ok(())
    }
}

/// Destination SFTP.
struct SftpSink {
    file: Option<russh_sftp::client::fs::File>,
}

#[async_trait]
impl Sink for SftpSink {
    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        if let Some(f) = self.file.as_mut() {
            f.write_all(data).await
        } else {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "fichier SFTP fermé"))
        }
    }

    async fn finish(&mut self) -> io::Result<()> {
        if let Some(f) = self.file.take() {
            let _ = f.close().await;
        }
        Ok(())
    }
}

#[async_trait]
impl RemoteFs for SftpFs {
    async fn list(&self, path: &str) -> FsResult<Listing> {
        let dir = if path.is_empty() { "." } else { path };
        let abs = match self.sftp.canonicalize(dir).await {
            Ok(p) => p,
            Err(e) => return Err(FsError::Other(format!("{} : {e}", path))),
        };
        let entries = match self.sftp.read_dir(&abs).await {
            Ok(r) => r,
            Err(e) => return Err(FsError::Other(format!("{path} : {e}"))),
        };
        let mut out = Vec::new();
        for de in entries {
            let name = de.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let md = de.metadata();
            let mut e = self.meta_entry(name, &md);
            // Lien symbolique : on lit la cible.
            if matches!(md.permissions.unwrap_or(0) & 0o170000, 0o120000) {
                if let Ok(target) = self.sftp.read_link(&remote_join(&abs, &e.name)).await {
                    e.link = Some(target);
                }
            }
            e.path = Some(remote_join(&abs, &e.name));
            out.push(e);
        }
        self.last_used.store(now_ms(), Ordering::Relaxed);
        Ok(Listing {
            path: abs,
            entries: out,
            source: "sftp".into(),
            raw: String::new(),
            truncated: false,
        })
    }

    async fn stat(&self, path: &str) -> FsResult<Entry> {
        let name = crate::util::remote_base(path);
        let m = self.attrs(path).await?;
        let mut e = self.meta_entry(name, &m);
        if e.kind == EntryKind::Symlink {
            e.link = self.sftp.read_link(path).await.ok();
        }
        Ok(e)
    }

    async fn realpath(&self, path: &str) -> FsResult<String> {
        match self.sftp.canonicalize(path).await {
            Ok(p) => Ok(p),
            Err(e) => Err(FsError::Other(format!("{path} : {e}"))),
        }
    }

    async fn mkdir(&self, path: &str) -> FsResult<()> {
        match self.sftp.create_dir(path).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("{path} : {e}"))),
        }
    }

    async fn rmdir(&self, path: &str) -> FsResult<()> {
        match self.sftp.remove_dir(path).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("{path} : {e}"))),
        }
    }

    async fn remove(&self, path: &str) -> FsResult<()> {
        match self.sftp.remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("{path} : {e}"))),
        }
    }

    async fn rename(&self, from: &str, to: &str) -> FsResult<()> {
        match self.sftp.rename(from, to).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("{from} → {to} : {e}"))),
        }
    }

    async fn chmod(&self, path: &str, mode: u32) -> FsResult<()> {
        let mut a = FileAttributes::default();
        // On conserve le type déjà présent si possible.
        if let Ok(cur) = self.sftp.metadata(path).await {
            a.permissions = Some((cur.permissions.unwrap_or(0) & 0o170000) | mode);
        } else {
            a.permissions = Some(mode);
        }
        match self.sftp.set_metadata(path, a).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("chmod {path} : {e}"))),
        }
    }

    async fn chown(&self, path: &str, user: &str) -> FsResult<()> {
        let mut a = FileAttributes::default();
        let (u, g) = if let Some((a, b)) = user.split_once(':') {
            (a.parse::<u32>().ok(), Some(b.parse::<u32>().unwrap_or(0)))
        } else {
            (user.parse::<u32>().ok(), None)
        };
        if u.is_none() {
            return Err(FsError::Other(
                "il faut un uid numérique (ou uid:gid)".into(),
            ));
        }
        a.uid = u;
        a.gid = g;
        match self.sftp.set_metadata(path, a).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("chown {path} : {e}"))),
        }
    }

    async fn set_mtime(&self, path: &str, ts: i64) -> FsResult<()> {
        let mut a = FileAttributes::default();
        if let Ok(cur) = self.sftp.metadata(path).await {
            a.atime = cur.atime;
            a.permissions = cur.permissions;
            a.size = cur.size;
        }
        a.mtime = Some(ts as u32);
        match self.sftp.set_metadata(path, a).await {
            Ok(()) => Ok(()),
            Err(e) => Err(FsError::Other(format!("horodatage {path} : {e}"))),
        }
    }

    async fn exists(&self, path: &str) -> bool {
        self.sftp.try_exists(path).await.unwrap_or(false)
    }

    async fn size(&self, path: &str) -> Option<u64> {
        self.attrs(path).await.ok().and_then(|a| a.size)
    }

    async fn open_read(&self, path: &str, offset: u64) -> FsResult<Box<dyn Src + Send>> {
        use russh_sftp::protocol::OpenFlags;
        let mut f = self
            .sftp
            .open_with_flags(path, OpenFlags::READ)
            .await
            .map_err(|e| FsError::Other(format!("RETR {path} : {e}")))?;
        if offset > 0 {
            f.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| FsError::Io(e.to_string()))?;
        }
        self.last_used.store(now_ms(), Ordering::Relaxed);
        Ok(Box::new(SftpSrc { file: Some(f), eof: false }))
    }

    async fn open_write(&self, path: &str, offset: u64, append: bool) -> FsResult<Box<dyn Sink + Send>> {
        use russh_sftp::protocol::OpenFlags;
        let mut flags = OpenFlags::WRITE | OpenFlags::CREATE;
        if append {
            flags |= OpenFlags::APPEND;
        } else if offset == 0 {
            flags |= OpenFlags::TRUNCATE;
        }
        let mut f = self
            .sftp
            .open_with_flags(path, flags)
            .await
            .map_err(|e| FsError::Other(format!("STOR {path} : {e}")))?;
        if offset > 0 && !append {
            f.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|e| FsError::Io(e.to_string()))?;
        }
        self.last_used.store(now_ms(), Ordering::Relaxed);
        Ok(Box::new(SftpSink { file: Some(f) }))
    }

    async fn ping(&self) -> FsResult<()> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(FsError::Closed);
        }
        let mut g = self.handle.lock().await;
        match g.as_mut() {
            Some(h) => h
                .send_keepalive(true)
                .await
                .map_err(|e| FsError::Io(format!("keepalive : {e}")))?,
            None => return Err(FsError::Closed),
        }
        Ok(())
    }

    async fn shutdown(&self) {
        if self.closed.swap(true, Ordering::Relaxed) {
            return;
        }
        let _ = self.sftp.close().await;
        if let Some(h) = self.handle.lock().await.take() {
            let _ = h.disconnect(russh::Disconnect::ByApplication, "", "en-US").await;
        }
    }

    async fn info(&self) -> ConnInfo {
        self.info.read().unwrap().clone()
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn last_used(&self) -> i64 {
        self.last_used.load(Ordering::Relaxed)
    }

    fn touch(&self) {
        self.last_used.store(now_ms(), Ordering::Relaxed);
    }

    fn proto(&self) -> Proto {
        Proto::Sftp
    }

    fn features(&self) -> Vec<String> {
        vec!["SFTP".into()]
    }
}

/// Fabrique de connexions SFTP.
pub async fn make_fs(
    site: &Site,
    settings: &Settings,
    hub: &Arc<LogHub>,
    dir: &std::path::Path,
) -> FsResult<Arc<dyn RemoteFs>> {
    let known = Arc::new(KnownHosts::open(dir));
    let fs = SftpFs::connect(site, settings, hub, known).await?;
    Ok(Arc::new(fs))
}


