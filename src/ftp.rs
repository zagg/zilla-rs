//! Client FTP / FTPES / FTPS explicite ou implicite.
//!
//! Le comportement visé est celui de FileZilla : journal de dialogue
//! `Command:` / `Response:`, mode passif par défaut (EPSV puis PASV), repli
//! explicite en mode actif, MLSD puis LIST, reprise par `REST`, `ABORT` etc.

use crate::fs::{FsError, FsResult, RemoteFs, Sink, Src};
use crate::hub::LogHub;
use crate::listparse::{self, parse_listing};
use crate::models::{ConnInfo, Entry, EntryKind, Listing, LoginType, Proto, Site, TlsLevel};
use crate::net::{self, bind_active, Sock};
use crate::settings::Settings;
use crate::util::{now_ms, remote_join, short_id};
use async_trait::async_trait;
use std::collections::HashSet;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;

/// Réponse FTP (une ou plusieurs lignes).
#[derive(Clone, Debug)]
pub struct Reply {
    pub code: u16,
    pub text: String,
}

impl Reply {
    pub fn ok(&self) -> FsResult<&Reply> {
        if self.code >= 400 {
            Err(FsError::from_reply(self.code, &self.text))
        } else {
            Ok(self)
        }
    }
    pub fn is_positive(&self) -> bool {
        self.code < 400
    }
}

/// Paramètres figés d'une connexion.
pub struct Cfg {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub proto: Proto,
    pub settings: Settings,
    pub site: Site,
}

impl Cfg {
    pub fn new(site: &Site, settings: &Settings) -> Cfg {
        let mut proto = site.proto;
        if site.use_tls && proto == Proto::Ftp {
            proto = Proto::Ftpes;
        }
        let (user, pass) = match site.login {
            LoginType::Anonymous => ("anonymous".to_string(), "zilla-rs@localhost".to_string()),
            LoginType::Interactive => (site.user.clone(), String::new()),
            _ => (site.user.clone(), site.password.clone()),
        };
        Cfg {
            host: site.host.clone(),
            port: site.port(),
            user,
            pass,
            proto,
            settings: settings.clone(),
            site: site.clone(),
        }
    }

    fn wants_tls(&self) -> bool {
        match self.site.tls {
            TlsLevel::Plain => false,
            TlsLevel::IfAvailable => self.proto.is_tls(),
            TlsLevel::RequireExplicit | TlsLevel::RequirePfs => true,
        }
    }

    fn passive(&self) -> bool {
        self.site.passive.unwrap_or(self.settings.ftp.passive)
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.settings.net.response_timeout.max(2))
    }

}

/// Le fil de contrôle.
pub struct Ctrl {
    pub sock: Sock,
    pub buf: Vec<u8>,
    pub pos: usize,
    pub cfg: Arc<Cfg>,
    pub hub: Arc<LogHub>,
    pub cwd: Option<String>,
    pub features: HashSet<String>,
    pub sys: String,
    pub server_kind: u8,
    pub tls: bool,
    pub utf8: bool,
    pub epsv: bool,
    pub banner: String,
    pub mlsd_types: bool,
}

impl Ctrl {
    async fn fill(&mut self) -> FsResult<()> {
        if self.pos > 4096 {
            self.buf.drain(0..self.pos);
            self.pos = 0;
        }
        let mut tmp = [0u8; 1024];
        let n = match tokio::time::timeout(self.cfg.timeout(), self.sock.read(&mut tmp)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(FsError::Io(e.to_string())),
            Err(_) => {
                return Err(FsError::Timeout(
                    "aucune réponse du serveur sur le fil de contrôle".into(),
                ))
            }
        };
        if n == 0 {
            return Err(FsError::Closed);
        }
        self.buf.extend_from_slice(&tmp[..n]);
        Ok(())
    }

    async fn line(&mut self) -> FsResult<String> {
        loop {
            if let Some(i) = self.buf[self.pos..]
                .iter()
                .position(|b| *b == b'\n')
                .map(|i| i + self.pos)
            {
                let mut s = String::from_utf8_lossy(&self.buf[self.pos..i]).into_owned();
                self.pos = i + 1;
                if s.ends_with('\r') {
                    s.pop();
                }
                return Ok(s);
            }
            self.fill().await?;
        }
    }

    async fn raw_reply(&mut self) -> FsResult<Reply> {
        let mut first = String::new();
        // Quelques serveurs envoient des lignes vides ou sans code au début.
        for _ in 0..32 {
            let l = self.line().await?;
            if l.trim().is_empty() {
                continue;
            }
            if l.len() < 4 || !l[..3].bytes().all(|c| c.is_ascii_digit()) {
                return Err(FsError::Protocol(l));
            }
            first = l;
            break;
        }
        if first.is_empty() {
            return Err(FsError::Protocol("réponse vide".into()));
        }
        let code: u16 = first[..3].parse().unwrap();
        let cont = first.as_bytes().get(3).copied() == Some(b'-');
        // Le texte démarre juste après le code ; le séparateur (espace, ou le
        // tiret du multi-ligne) est parfois absent : on le saute sans manger
        // le premier caractère utile de la réponse.
        let mut text = match first.get(3..) {
            Some(rest) => rest.trim_start_matches([' ', '-']).to_string(),
            None => String::new(),
        };
        if cont {
            loop {
                let l = self.line().await?;
                // Fin de bloc : « 211 End » (les serveurs tolèrent un espace
                // résiduel devant la ligne de clôture).
                let t = l.trim_start();
                if t.starts_with(&first[..3]) && t[3..].starts_with(' ') {
                    break;
                }
                if l.len() >= 4 && l[..3] == first[..3] && l.as_bytes()[3] == b' ' {
                    break;
                }
                text.push('\n');
                text.push_str(&l);
            }
        }
        let verbose = self.hub.verbose();
        if verbose {
            self.hub.trace("server", format!("Response: {code} {text}"));
        }
        Ok(Reply { code, text })
    }

    async fn reply(&mut self) -> FsResult<Reply> {
        let r = self.raw_reply().await;
        match &r {
            Ok(rep) if rep.code >= 400 => {
                self.hub.trace("server", format!("Response: {} {}", rep.code, rep.text));
            }
            Ok(rep) => {
                self.hub.trace("server", format!("Response: {} {}", rep.code, rep.text));
            }
            Err(_) => {}
        }
        r
    }

    pub async fn cmd(&mut self, c: &str) -> FsResult<Reply> {
        let logc = if c.to_uppercase().starts_with("PASS") {
            "PASS ***"
        } else {
            c
        };
        self.hub.trace("server", format!("Command: {logc}"));
        let mut data = String::with_capacity(c.len() + 2);
        data.push_str(c);
        data.push_str("\r\n");
        match tokio::time::timeout(self.cfg.timeout(), async {
            self.sock.write_all(data.as_bytes()).await?;
            self.sock.flush().await
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(FsError::Io(e.to_string())),
            Err(_) => return Err(FsError::Timeout("envoi de commande bloqué".into())),
        }
        self.reply().await
    }

    pub async fn ok(&mut self, c: &str) -> FsResult<Reply> {
        let r = self.cmd(c).await?;
        match r.ok() {
            Ok(_) => Ok(r),
            Err(e) => Err(e),
        }
    }

    pub async fn quit(&mut self) {
        let _ = self.cmd("QUIT").await;
        let _ = self.sock.close().await;
    }

    /// Hôte vers lequel ouvrir un canal de données.
    fn data_host(&self, from_reply: Option<&str>) -> String {
        if let Some(h) = from_reply {
            return h.to_string();
        }
        let peer = self.sock.peer_addr();
        match peer.rsplit_once(':') {
            Some((h, _)) => h.to_string(),
            None => self.cfg.host.clone(),
        }
    }

    async fn pasv(&mut self) -> FsResult<(String, u16)> {
        let use_epsv = self.epsv
            && self.cfg.settings.ftp.use_epsv
            && self.features.contains("EPSV");
        if use_epsv {
            match self.cmd("EPSV").await {
                Ok(r) if r.code == 229 => {
                    let port = r
                        .text
                        .split("|||")
                        .nth(1)
                        .and_then(|s| s.split('|').next())
                        .and_then(|s| s.parse::<u16>().ok())
                        .unwrap_or(0);
                    if port > 0 {
                        return Ok((self.data_host(None), port));
                    }
                }
                _ => {
                    self.epsv = false;
                }
            }
        }
        let r = self.cmd("PASV").await?;
        if r.code != 227 {
            return Err(FsError::Protocol(format!("PASV : {r:?}")));
        }
        let digits: Vec<u16> = r
            .text
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == ',')
            .collect::<String>()
            .split(',')
            .filter_map(|p| p.parse().ok())
            .collect();
        if digits.len() < 6 {
            return Err(FsError::Protocol("227 PASV malformed".into()));
        }
        let host = digits[..4]
            .iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(".");
        let port = digits[4] * 256 + digits[5];
        Ok((host, port))
    }

    async fn active(&mut self) -> FsResult<(tokio::net::TcpListener, u16)> {
        let (l, port) = bind_active(&self.cfg.settings).await?;
        let (ip, is_v6) = match l.local_addr() {
            Ok(a) => (a.ip(), a.ip().is_ipv6()),
            Err(_) => (std::net::IpAddr::from([127, 0, 0, 1]), false),
        };
        if is_v6 && self.features.contains("EPRT") {
            let c = format!("EPRT |2|{ip}|{port}|");
            self.ok(&c).await?;
        } else if is_v6 {
            return Err(FsError::Protocol(
                "IPv6 en mode actif nécessite EPRT".into(),
            ));
        } else {
            let oct = if let std::net::IpAddr::V4(v4) = ip {
                v4.octets()
            } else {
                [127, 0, 0, 1]
            };
            let c = format!(
                "PORT {}.{}.{}.{},{},{}",
                oct[0],
                oct[1],
                oct[2],
                oct[3],
                (port >> 8) as u8,
                (port & 0xff) as u8
            );
            self.ok(&c).await?;
        }
        Ok((l, port))
    }
}

/// Canal de données en préparation.
enum DataStart {
    Passive(Sock),
    Active(tokio::net::TcpListener),
}

impl DataStart {
    async fn accept(self, cfg: &Cfg, _hub: &LogHub) -> FsResult<Sock> {
        match self {
            DataStart::Passive(s) => Ok(s),
            DataStart::Active(l) => {
                let t = Duration::from_secs(cfg.settings.net.connect_timeout.max(5));
                match tokio::time::timeout(t, l.accept()).await {
                    Ok(Ok((s, _))) => {
                        let _ = s.set_nodelay(true);
                        Ok(Sock::Plain(s))
                    }
                    Ok(Err(e)) => Err(FsError::Io(format!("mode actif : {e}"))),
                    Err(_) => Err(FsError::Timeout(
                        "le serveur n'a pas ouvert de connexion de données (mode actif)".into(),
                    )),
                }
            }
        }
    }
}

/// La connexion FTP vue comme système de fichiers distant.
pub struct FtpFs {
    pub ctrl: Arc<AsyncMutex<Ctrl>>,
    pub hub: Arc<LogHub>,
    id: String,
    closed: AtomicBool,
    last_used: AtomicI64,
    #[allow(dead_code)]
    created: i64,
    #[allow(dead_code)]
    tls: AtomicBool,
    #[allow(dead_code)]
    server_kind: AtomicU8,
    pub info: RwLock<ConnInfo>,
}

impl FtpFs {
    pub fn id(&self) -> String {
        self.id.clone()
    }

    fn touch(&self) {
        self.last_used.store(now_ms(), Ordering::Relaxed);
    }


    /// Ouvre une connexion FTP complète.
    pub async fn connect(site: &Site, settings: &Settings, hub: &Arc<LogHub>) -> FsResult<FtpFs> {
        let cfg = Arc::new(Cfg::new(site, settings));
        let id = short_id();
        hub.status(format!(
            "Connexion à {} ({} {})…",
            cfg.host,
            cfg.proto.name(),
            cfg.port
        ));
        hub.status("Connection established, waiting for welcome message…");
        let mut sock = net::connect(&cfg.host, cfg.port, settings, hub).await?;
        sock.nodelay();

        let mut ctrl = Ctrl {
            sock,
            buf: Vec::new(),
            pos: 0,
            cfg: cfg.clone(),
            hub: hub.clone(),
            cwd: None,
            features: HashSet::new(),
            sys: String::new(),
            server_kind: 0,
            tls: false,
            utf8: false,
            epsv: false,
            banner: String::new(),
            mlsd_types: false,
        };

        // Bande de bienvenue.
        let banner = ctrl.reply().await?;
        if banner.code != 220 {
            return Err(FsError::Auth(format!(
                "le serveur n'a pas accepté la connexion : {}",
                banner.text
            )));
        }
        ctrl.banner = banner.text.clone();

        // CLNT, façon FileZilla.
        if settings.ftp.send_clnt {
            let _ = ctrl.cmd(&format!("CLNT {}", settings.ftp.clnt_string)).await;
        }

        // FTPS implicite.
        if cfg.proto == Proto::Ftps {
            ctrl.sock = net::upgrade_tls(ctrl.sock, &cfg.host, settings, hub).await?;
            ctrl.tls = true;
        }

        // FEAT pour connaître les extensions.
        if let Ok(f) = ctrl.cmd("FEAT").await {
            if f.code == 211 {
                for l in f.text.lines() {
                    let t = l.trim();
                    if t.is_empty() {
                        continue;
                    }
                    let feat = t.split_whitespace().next().unwrap_or("").to_uppercase();
                    if feat.is_empty() || feat.ends_with(':') {
                        continue;
                    }
                    ctrl.features.insert(feat);
                }
            }
        }
        ctrl.epsv = ctrl.features.contains("EPSV") || ctrl.features.contains("EPSV ALL");
        ctrl.mlsd_types = ctrl.features.iter().any(|f| f.starts_with("MLSD"));
        if let Ok(s) = ctrl.cmd("SYST").await {
            if s.code == 215 {
                ctrl.sys = s.text.clone();
                if s.text.to_uppercase().contains("WIN") || s.text.to_uppercase().contains("DOS") {
                    ctrl.server_kind = 2;
                } else {
                    ctrl.server_kind = 1;
                }
            }
        }

        // TLS explicite.
        let want_tls = cfg.wants_tls() && !ctrl.tls;
        if want_tls {
            hub.status("Initializing TLS…");
            let auth = match ctrl.cmd("AUTH TLS").await {
                Ok(r) => r,
                Err(e) => {
                    hub.warn("general", format!("AUTH TLS n'est pas accepté : {e}"));
                    Reply {
                        code: 530,
                        text: "no tls".into(),
                    }
                }
            };
            if auth.code == 234 || auth.code == 334 {
                ctrl.sock = net::upgrade_tls(ctrl.sock, &cfg.host, settings, hub).await?;
                ctrl.tls = true;
                hub.success("general", "TLS connection established");
                if settings.ftp.tls_protection {
                    let _ = ctrl.cmd("PBSZ 0").await;
                    let p = ctrl.cmd("PROT P").await?;
                    if !p.is_positive() {
                        return Err(FsError::Tls(
                            "le serveur refuse PROT P sur la session TLS".into(),
                        ));
                    }
                }
            } else {
                match cfg.site.tls {
                    TlsLevel::RequireExplicit | TlsLevel::RequirePfs => {
                        return Err(FsError::Tls(format!(
                            "le serveur ne prend pas en charge TLS : {}",
                            auth.text
                        )));
                    }
                    _ => {
                        hub.warn(
                            "general",
                            "Le serveur ne prend pas en charge TLS, suite en clair.",
                        );
                    }
                }
            }
        }

        // Identifiants.
        let user = ctrl.cmd(&format!("USER {}", cfg.user)).await?;
        match user.code {
            230 => {}
            331 | 332 => {
                let pass = if cfg.pass.is_empty() {
                    match ctrl.cfg.site.login {
                        LoginType::Anonymous => "zilla-rs@localhost",
                        _ => "",
                    }
                } else {
                    &cfg.pass
                };
                let rp = ctrl.cmd(&format!("PASS {}", pass)).await?;
                if rp.code == 332 {
                    let _ = ctrl.cmd("ACCT none").await;
                } else if rp.code != 230 {
                    return Err(FsError::Auth(rp.text.clone()));
                }
            }
            336 => {
                ctrl.sock = net::upgrade_tls(ctrl.sock, &cfg.host, settings, hub).await?;
                ctrl.tls = true;
                let rp = ctrl.cmd(&format!("PASS {}", cfg.pass)).await?;
                if rp.code != 230 {
                    return Err(FsError::Auth(rp.text));
                }
            }
            202 => {}
            other => {
                return Err(FsError::Auth(format!("USER refusé ({other}) : {}", user.text)));
            }
        }

        // UTF-8.
        if settings.ftp.use_utf8 || site.force_utf8 {
            if ctrl.features.contains("UTF8") || ctrl.features.contains("UTF8 ON") {
                match ctrl.cmd("OPTS UTF8 ON").await {
                    Ok(r) if r.is_positive() => ctrl.utf8 = true,
                    _ => {}
                }
            }
        }

        if settings.ftp.use_host && !site.virtual_host.is_empty() {
            let _ = ctrl.cmd(&format!("HOST {}", site.virtual_host)).await;
        }

        // Repérage du répertoire courant.
        match ctrl.cmd("PWD").await {
            Ok(r) if r.code == 257 => {
                if let Some(p) = r.text.split('\"').nth(1) {
                    ctrl.cwd = Some(p.to_string());
                }
            }
            _ => {}
        }

        for c in &site.pre_commands {
            let _ = ctrl.cmd(c).await;
        }
        for c in &settings.ftp.pre_connect {
            let _ = ctrl.cmd(c).await;
        }

        hub.success(
            "general",
            format!("Connecté à {}:{}", cfg.host, cfg.port),
        );

        let (ver, cipher) = ctrl.sock.tls_info();
        let local_addr = ctrl.sock.local_addr();
        let remote_addr = ctrl.sock.peer_addr();
        let info = ConnInfo {
            site_id: site.id.clone(),
            proto: cfg.proto,
            host: cfg.host.clone(),
            port: cfg.port,
            user: cfg.user.clone(),
            tls: ctrl.tls,
            tls_version: ver,
            cipher,
            local_addr,
            remote_addr,
            banner: ctrl.banner.clone(),
            server_type: ctrl.sys.clone(),
            active: !cfg.passive(),
            connections: 1,
            max_connections: settings.ftp.max_connections,
            since: now_ms(),
            hostkey_fp: String::new(),
            features: ctrl.features.iter().cloned().collect(),
        };
        let tls_on = ctrl.tls;
        let fs = FtpFs {
            ctrl: Arc::new(AsyncMutex::new(ctrl)),
            hub: hub.clone(),
            id,
            closed: AtomicBool::new(false),
            last_used: AtomicI64::new(now_ms()),
            created: now_ms(),
            tls: AtomicBool::new(tls_on),
            server_kind: AtomicU8::new(if fs_kind(&info) { 1 } else { 0 }),
            info: RwLock::new(info),
        };
        Ok(fs)
    }

    /// Statistiques de connexion pour l'interface.
    pub async fn conn_info(&self) -> ConnInfo {
        let mut i = self.info.read().unwrap().clone();
        let c = self.ctrl.lock().await;
        i.tls = c.tls;
        i.banner = c.banner.clone();
        i.server_type = c.sys.clone();
        i.features = c.features.iter().cloned().collect();
        i.active = !c.cfg.passive();
        let (v, s) = c.sock.tls_info();
        i.tls_version = v;
        i.cipher = s;
        i.connections = 1;
        i
    }

    async fn ensure_type(&self, c: &mut Ctrl, path: &str) {
        let t = if self.needs_ascii(path) { "A" } else { "I" };
        let _ = c.cmd(&format!("TYPE {t}")).await;
    }

    fn needs_ascii(&self, path: &str) -> bool {
        let s = self.ctrl_try_settings();
        if !s.transfers.ascii_detect {
            return false;
        }
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        s.transfers
            .ascii_extensions
            .iter()
            .any(|e| e.to_lowercase() == ext)
    }

    fn ctrl_try_settings(&self) -> Settings {
        // sans lock : les réglages sont figés à la connexion
        self.cfg_snapshot()
    }

    pub fn cfg_snapshot(&self) -> Settings {
        // Le Cfg est dans le ctrl, mais une copie est trop coûteuse sous lock.
        // On garde une valeur par dérogation : on lit via try_lock.
        match self.ctrl.try_lock() {
            Ok(c) => c.cfg.settings.clone(),
            Err(_) => Settings::default(),
        }
    }

    /// Ouvre le canal de données selon le mode (passif/actif).
    async fn begin_data(&self, c: &mut Ctrl) -> FsResult<DataStart> {
        if c.cfg.passive() {
            match c.pasv().await {
                Ok((host, port)) => {
                    let _t = Duration::from_secs(c.cfg.settings.net.connect_timeout.max(5));
                    let s = net::connect(&host, port, &c.cfg.settings, &self.hub)
                        .await
                        .map_err(|e| {
                            FsError::Io(format!("connexion de données passive {host}:{port} — {e}"))
                        })?;
                    let mut s = s;
                    s.nodelay();
                    if c.tls && c.cfg.settings.ftp.tls_protection {
                        s = net::upgrade_tls(s, &c.cfg.host, &c.cfg.settings, &self.hub).await?;
                    }
                    Ok(DataStart::Passive(s))
                }
                Err(e) => {
                    if c.cfg.settings.tls.fallback || true {
                        self.hub
                            .warn("general", format!("Mode passif : {e} — essai en mode actif."));
                        let (l, _) = c.active().await?;
                        Ok(DataStart::Active(l))
                    } else {
                        Err(e)
                    }
                }
            }
        } else {
            match c.active().await {
                Ok((l, _)) => Ok(DataStart::Active(l)),
                Err(e) => {
                    self.hub
                        .warn("general", format!("Mode actif : {e} — repli en passif."));
                    let (host, port) = c.pasv().await?;
                    let s = net::connect(&host, port, &c.cfg.settings, &self.hub).await?;
                    let mut s = s;
                    s.nodelay();
                    if c.tls && c.cfg.settings.ftp.tls_protection {
                        s = net::upgrade_tls(s, &c.cfg.host, &c.cfg.settings, &self.hub).await?;
                    }
                    Ok(DataStart::Passive(s))
                }
            }
        }
    }

    /// Envoie `cmd`, attend la réponse préliminaire, récupère le flux de données.
    async fn begin_transfer(
        &self,
        c: &mut Ctrl,
        cmd: &str,
    ) -> FsResult<Sock> {
        let start = self.begin_data(c).await?;
        let r = match c.cmd(cmd).await {
            Ok(r) => r,
            Err(e) => {
                match start {
                    DataStart::Passive(mut s) => {
                        let _ = s.close().await;
                    }
                    DataStart::Active(l) => drop(l),
                }
                return Err(e);
            }
        };
        if r.code >= 400 {
            match start {
                DataStart::Passive(mut s) => {
                    let _ = s.close().await;
                }
                DataStart::Active(l) => drop(l),
            }
            return Err(FsError::from_reply(r.code, &r.text));
        }
        match start.accept(&c.cfg, &self.hub).await {
            Ok(s) => {
                let mut s = s;
                s.nodelay();
                Ok(s)
            }
            Err(e) => Err(e),
        }
    }

    /// Lit la réponse de fin de transfert.
    async fn finish(c: &mut Ctrl) -> FsResult<()> {
        match tokio::time::timeout(c.cfg.timeout(), c.reply()).await {
            Ok(Ok(r)) => {
                if r.code >= 400 {
                    return Err(FsError::from_reply(r.code, &r.text));
                }
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(FsError::Timeout(
                "pas de fin de transfert (225/226/250) reçue".into(),
            )),
        }
    }

    async fn mlstat(&self, c: &mut Ctrl, path: &str) -> FsResult<Entry> {
        if !c.features.contains("MLST") && !c.features.contains("MLST ") {
            return Err(FsError::Other("MLST non pris en charge".into()));
        }
        let r = c.cmd(&format!("MLST {path}")).await?;
        if r.code != 250 {
            return Err(FsError::from_reply(r.code, &r.text));
        }
        let line = r
            .text
            .lines()
            .find(|l| l.starts_with(' ') && l.contains(';'))
            .unwrap_or("");
        match listparse::parse_mlsd(line.trim()) {
            // parse_mlsd sait déjà séparer les faits du chemin, y compris quand
            // le serveur renvoie le chemin intégral : on garde ce nom-ci, qui
            // est le seul cohérent avec ce qu'affiche le panneau.
            Some(e) => Ok(e),
            None => Err(FsError::Protocol("MLST non interprétable".into())),
        }
    }

    /// Chemin absolu canonique.
    async fn real(&self, c: &mut Ctrl, path: &str) -> FsResult<String> {
        if path == "." {
            if let Some(cwd) = c.cwd.clone() {
                return Ok(cwd);
            }
        }
        match self.mlstat(c, path).await {
            Ok(e) => {
                if e.kind == EntryKind::Dir {
                    return Ok(remote_join(&c.cwd.clone().unwrap_or_else(|| "/".into()), &e.name));
                }
            }
            Err(_) => {}
        }
        let abs = if path.starts_with('/') {
            path.to_string()
        } else {
            remote_join(&c.cwd.clone().unwrap_or_else(|| "/".into()), path)
        };
        if path.starts_with('/') {
            return Ok(abs);
        }
        let save = c.cwd.clone();
        match c.cmd(&format!("CWD {path}")).await {
            Ok(r) if r.code == 250 => {
                let p = self.pwd(c).await.unwrap_or(abs);
                if let Some(s) = save {
                    let _ = c.cmd(&format!("CWD {}", quote(&s))).await;
                }
                Ok(p)
            }
            _ => Ok(abs),
        }
    }

    async fn pwd(&self, c: &mut Ctrl) -> FsResult<String> {
        let r = c.cmd("PWD").await?;
        if r.code != 257 {
            return Err(FsError::Protocol(format!("PWD : {}", r.text)));
        }
        let p = r
            .text
            .split('\"')
            .nth(1)
            .unwrap_or("/")
            .to_string();
        c.cwd = Some(p.clone());
        Ok(p)
    }

    /// Se déplace, renvoie le nouveau chemin absolu.
    async fn chdir(&self, c: &mut Ctrl, path: &str) -> FsResult<String> {
        let target = if path.is_empty() || path == "/" {
            "/".to_string()
        } else {
            path.to_string()
        };
        if c.cwd.as_deref() == Some(target.as_str()) {
            return Ok(target);
        }
        let q = quote(&target);
        match c.cmd(&format!("CWD {q}")).await {
            Ok(r) if r.code == 250 || r.code == 202 => {}
            Ok(r) if r.code == 257 => {}
            Ok(r) => return Err(FsError::from_reply(r.code, &r.text)),
            Err(e) => return Err(e),
        }
        Ok(self.pwd(c).await?)
    }

    pub async fn keepalive(&self) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        c.cmd("NOOP").await?;
        self.touch();
        Ok(())
    }
}

fn fs_kind(i: &ConnInfo) -> bool {
    i.tls
}

/// Les commandes FTP prennent le reste de la ligne comme argument :
/// pas d’échappement, sinon vsFTPd (et d’autres) répondent 550.
fn quote(path: &str) -> String {
    path.trim().to_string()
}

#[async_trait]
impl RemoteFs for FtpFs {
    async fn list(&self, path: &str) -> FsResult<Listing> {
        let mut c = self.ctrl.lock().await;
        if c.cfg.settings.ftp.use_pret {
            let _ = c.cmd("PRET").await;
        }
        let dir = self.chdir(&mut c, path).await?;
        // MLSD, sinon LIST.
        let mut entries: Vec<Entry> = Vec::new();
        let mut source = String::new();
        let mut raw = String::new();
        if self.cfg_snapshot().ftp.use_mlsd
            && (c.features.contains("MLSD") || c.mlsd_types)
        {
            let cmd = if dir == "/" { "MLSD" } else { "MLSD" };
            match self.begin_transfer(&mut c, cmd).await {
                Ok(mut sock) => {
                    let mut buf: Vec<u8> = Vec::new();
                    let mut tmp = vec![0u8; 16384];
                    loop {
                        match tokio::time::timeout(
                            Duration::from_secs(self.cfg_snapshot().net.data_timeout.max(1)),
                            sock.read(&mut tmp),
                        )
                        .await
                        {
                            Ok(Ok(0)) | Err(_) => break,
                            Ok(Ok(n)) => buf.extend_from_slice(&tmp[..n]),
                            Ok(Err(e)) => return Err(FsError::Io(e.to_string())),
                        }
                    }
                    let _ = sock.close().await;
                    let _ = FtpFs::finish(&mut c).await;
                    raw = String::from_utf8_lossy(&buf).into_owned();
                    entries = parse_listing(&raw);
                    source = "mlsd".into();
                }
                Err(e) => {
                    self.hub.trace("file", format!("MLSD indisponible ({e}), on utilise LIST."));
                }
            }
        }

        if entries.is_empty() {
            match self.begin_transfer(&mut c, "LIST").await {
                Ok(mut sock) => {
                    let mut buf: Vec<u8> = Vec::new();
                    let mut tmp = vec![0u8; 16384];
                    loop {
                        match sock.read(&mut tmp).await {
                            Ok(0) => break,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                            Err(e) => {
                                let _ = sock.close().await;
                                return Err(FsError::Io(e.to_string()));
                            }
                        }
                    }
                    let _ = sock.close().await;
                    let fin = FtpFs::finish(&mut c).await;
                    raw = String::from_utf8_lossy(&buf).into_owned();
                    entries = parse_listing(&raw);
                    source = "list".into();
                    if let Err(e) = fin {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }

        // On complète ce qui manque avec SIZE/MDTM (certains serveurs).
        let need_time = entries
            .iter()
            .all(|e| e.kind == EntryKind::File && e.mtime.is_none())
            && !entries.is_empty();
        if need_time && c.cfg.settings.ftp.use_mdtm {
            let names: Vec<String> = entries.iter().map(|e| e.name.clone()).take(80).collect();
            for n in names {
                if let Ok(r) = c.cmd(&format!("MDTM {n}")).await {
                    if r.code == 213 {
                        if let Some(ts) = listparse::parse_mlsd_time(r.text.trim()) {
                            if let Some(e) = entries.iter_mut().find(|e| e.name == n) {
                                e.mtime = Some(ts);
                            }
                        }
                    }
                }
            }
        }
        self.touch();
        Ok(Listing {
            path: dir,
            entries,
            source: if source.is_empty() { "list".into() } else { source },
            raw,
            truncated: false,
        })
    }

    async fn stat(&self, path: &str) -> FsResult<Entry> {
        let mut c = self.ctrl.lock().await;
        match self.mlstat(&mut c, path).await {
            Ok(e) => {
                self.touch();
                return Ok(e);
            }
            Err(_) => {}
        }
        let name = crate::util::remote_base(path);
        let is_dir_hint = path.ends_with('/') || path.ends_with('\\') || path == "." || path == "/";
        if is_dir_hint {
            if self.chdir(&mut c, path).await.is_ok() {
                let back = c.cwd.clone().unwrap_or_else(|| "/".into());
                let _ = self.chdir(&mut c, &back).await;
                return Ok(Entry::dir(name, None));
            }
            return Err(FsError::NotFound(path.to_string()));
        }
        let mut size = None;
        if c.cfg.settings.ftp.use_size {
            if let Ok(r) = c.cmd(&format!("SIZE {}", quote(path))).await {
                if r.code == 213 {
                    size = r.text.trim().parse::<u64>().ok();
                }
            }
        }
        let mut mtime = None;
        if c.cfg.settings.ftp.use_mdtm {
            if let Ok(r) = c.cmd(&format!("MDTM {}", quote(path))).await {
                if r.code == 213 {
                    mtime = listparse::parse_mlsd_time(r.text.trim());
                }
            }
        }
        if size.is_none() {
            // Peut-être un répertoire.
            let save = c.cwd.clone();
            if self.chdir(&mut c, path).await.is_ok() {
                if let Some(s) = save {
                    let _ = self.chdir(&mut c, &s).await;
                }
                return Ok(Entry::dir(name, mtime));
            }
            return Err(FsError::NotFound(path.to_string()));
        }
        let hidden = name.starts_with('.');
        self.touch();
        Ok(Entry {
            name,
            kind: EntryKind::File,
            size: size.unwrap_or(0),
            mtime,
            mode: None,
            user: None,
            group: None,
            link: None,
            hidden,
            path: None,
        })
    }

    async fn realpath(&self, path: &str) -> FsResult<String> {
        let mut c = self.ctrl.lock().await;
        self.real(&mut c, path).await
    }

    async fn mkdir(&self, path: &str) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        match c.cmd(&format!("MKD {}", quote(path))).await {
            Ok(r) if r.code == 257 || r.code == 250 => {}
            Ok(r) => return Err(FsError::from_reply(r.code, &r.text)),
            Err(e) => return Err(e),
        }
        self.touch();
        Ok(())
    }

    async fn rmdir(&self, path: &str) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        match c.cmd(&format!("RMD {}", quote(path))).await {
            Ok(r) if r.code == 250 || r.code == 257 => {}
            Ok(r) if r.code == 550 => {
                // Peut-être un fichier ordinaire.
                if let Ok(r2) = c.cmd(&format!("DELE {}", quote(path))).await {
                    if r2.code == 250 {
                        return Ok(());
                    }
                }
                return Err(FsError::from_reply(r.code, &r.text));
            }
            Ok(r) => return Err(FsError::from_reply(r.code, &r.text)),
            Err(e) => return Err(e),
        }
        self.touch();
        Ok(())
    }

    async fn remove(&self, path: &str) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        match c.cmd(&format!("DELE {}", quote(path))).await {
            Ok(r) if r.code == 250 || r.code == 226 => {}
            Ok(r) if r.code == 550 || r.code == 551 => {
                // Répertoire ?
                if let Ok(r2) = c.cmd(&format!("RMD {}", quote(path))).await {
                    if r2.code == 250 || r2.code == 257 {
                        return Ok(());
                    }
                }
                return Err(FsError::from_reply(r.code, &r.text));
            }
            Ok(r) => return Err(FsError::from_reply(r.code, &r.text)),
            Err(e) => return Err(e),
        }
        self.touch();
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        let r = c.cmd(&format!("RNFR {}", quote(from))).await?;
        if r.code != 350 {
            return Err(FsError::from_reply(r.code, &r.text));
        }
        let r = c.cmd(&format!("RNTO {}", quote(to))).await?;
        if r.code != 250 {
            return Err(FsError::from_reply(r.code, &r.text));
        }
        self.touch();
        Ok(())
    }

    async fn chmod(&self, path: &str, mode: u32) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        let m = format!("{mode:04o}");
        if c.cfg.settings.ftp.site_chmod {
            let r = c.cmd(&format!("SITE CHMOD {m} {}", quote(path))).await?;
            if r.code == 200 || r.code == 250 {
                return Ok(());
            }
        }
        let r = c.cmd(&format!("CHMOD {m} {}", quote(path))).await?;
        if r.code >= 400 {
            return Err(FsError::Other(
                "ce serveur ne permet pas de modifier les permissions".into(),
            ));
        }
        let _ = r;
        Ok(())
    }

    async fn chown(&self, path: &str, user: &str) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        let r = c.cmd(&format!("SITE CHOWN {user} {}", quote(path))).await?;
        if r.code >= 400 {
            return Err(FsError::Other(
                "la commande de propriété est refusée par ce serveur".into(),
            ));
        }
        Ok(())
    }

    async fn set_mtime(&self, path: &str, ts: i64) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        if !c.cfg.settings.ftp.use_mfmt {
            return Ok(());
        }
        let dt = crate::util::fmt_yyyy_mm_dd_hhmmss(ts);
        let r = c.cmd(&format!("MFMT {dt} {}", quote(path))).await?;
        if r.code == 200 || r.code == 250 {
            return Ok(());
        }
        let _ = c
            .cmd(&format!("SITE UTIME {} {dt}", quote(path)))
            .await;
        Ok(())
    }

    async fn exists(&self, path: &str) -> bool {
        self.stat(path).await.is_ok()
    }

    async fn open_read(&self, path: &str, offset: u64) -> FsResult<Box<dyn Src + Send>> {
        let mut c = self.ctrl.lock().await;
        self.ensure_type(&mut c, path).await;
        if offset > 0 {
            let r = c.cmd(&format!("REST {offset}")).await?;
            if r.code != 350 {
                return Err(FsError::Other(format!(
                    "reprise refusée par le serveur : {}",
                    r.text
                )));
            }
        }
        let sock = self
            .begin_transfer(&mut c, &format!("RETR {}", quote(path)))
            .await?;
        self.touch();
        Ok(Box::new(FtpSrc {
            data: Some(sock),
            ctrl: self.ctrl.clone(),
            done: false,
        }))
    }

    async fn open_write(&self, path: &str, offset: u64, append: bool) -> FsResult<Box<dyn Sink + Send>> {
        let mut c = self.ctrl.lock().await;
        self.ensure_type(&mut c, path).await;
        if append && offset == 0 {
            let sock = self
                .begin_transfer(&mut c, &format!("APPE {}", quote(path)))
                .await?;
            return Ok(Box::new(FtpSink {
                data: Some(sock),
                ctrl: self.ctrl.clone(),
            }));
        }
        if offset > 0 {
            let r = c.cmd(&format!("REST {offset}")).await?;
            if r.code != 350 {
                // Repli : on reprend en ajout.
                let sock = self
                    .begin_transfer(&mut c, &format!("APPE {}", quote(path)))
                    .await?;
                return Ok(Box::new(FtpSink {
                    data: Some(sock),
                    ctrl: self.ctrl.clone(),
                }));
            }
        }
        let sock = self
            .begin_transfer(&mut c, &format!("STOR {}", quote(path)))
            .await?;
        self.touch();
        Ok(Box::new(FtpSink {
            data: Some(sock),
            ctrl: self.ctrl.clone(),
        }))
    }

    async fn ping(&self) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        if self.closed.load(Ordering::Relaxed) {
            return Err(FsError::Closed);
        }
        c.cmd("NOOP").await?;
        self.touch();
        Ok(())
    }

    async fn abort(&self) -> FsResult<()> {
        let mut c = self.ctrl.lock().await;
        let _ = c.cmd("ABOR").await;
        Ok(())
    }

    async fn shutdown(&self) {
        if self.closed.swap(true, Ordering::Relaxed) {
            return;
        }
        let mut c = self.ctrl.lock().await;
        c.quit().await;
    }

    async fn info(&self) -> ConnInfo {
        self.conn_info().await
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
        Proto::Ftp
    }

    fn features(&self) -> Vec<String> {
        match self.ctrl.try_lock() {
            Ok(c) => c.features.iter().cloned().collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Source issue d'un canal `RETR`.
struct FtpSrc {
    data: Option<Sock>,
    ctrl: Arc<AsyncMutex<Ctrl>>,
    done: bool,
}

#[async_trait]
impl Src for FtpSrc {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(sock) = self.data.as_mut() else {
            return Ok(0);
        };
        match sock.read(buf).await {
            Ok(0) => {
                let _ = sock.close().await;
                self.data = None;
                Ok(0)
            }
            Ok(n) => Ok(n),
            Err(e) => Err(e),
        }
    }

    async fn close(&mut self) -> io::Result<()> {
        if let Some(mut s) = self.data.take() {
            let _ = s.close().await;
        }
        if self.done {
            return Ok(());
        }
        self.done = true;
        let mut c = self.ctrl.lock().await;
        match FtpFs::finish(&mut c).await {
            Ok(()) => Ok(()),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
        }
    }
}

/// Destination d'un `STOR`/`APPE`.
struct FtpSink {
    data: Option<Sock>,
    ctrl: Arc<AsyncMutex<Ctrl>>,
}

#[async_trait]
impl Sink for FtpSink {
    async fn write(&mut self, data: &[u8]) -> io::Result<()> {
        match self.data.as_mut() {
            Some(s) => s.write_all(data).await,
            None => Err(io::Error::new(io::ErrorKind::BrokenPipe, "canal de données fermé")),
        }
    }

    async fn finish(&mut self) -> io::Result<()> {
        if let Some(mut s) = self.data.take() {
            s.shutdown().await?;
            drop(s);
        }
        let mut c = self.ctrl.lock().await;
        match FtpFs::finish(&mut c).await {
            Ok(()) => Ok(()),
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
        }
    }
}

/// Fabrique de connexions FTP pour le parc de sites.
pub async fn make_fs(
    site: &Site,
    settings: &Settings,
    hub: &Arc<LogHub>,
) -> FsResult<Arc<dyn RemoteFs>> {
    let fs = FtpFs::connect(site, settings, hub).await?;
    Ok(Arc::new(fs))
}
