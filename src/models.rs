//! Modèles : sites, types de protocole, entrées de répertoire, états.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Protocole de transfert. Façon FileZilla : FTP, FTPES (explicite), FTPS (implicite), SFTP.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    #[default]
    #[serde(alias = "Ftp")]
    Ftp,
    /// FTPES : FTP sur TLS explicite (`AUTH TLS`), port 21.
    #[serde(alias = "Ftpes")]
    Ftpes,
    /// FTPS : TLS implicite, port 990.
    #[serde(alias = "Ftps")]
    Ftps,
    /// SSH File Transfer Protocol.
    #[serde(alias = "Sftp")]
    Sftp,
}

impl Proto {
    pub fn name(self) -> &'static str {
        match self {
            Proto::Ftp => "FTP",
            Proto::Ftpes => "FTPES",
            Proto::Ftps => "FTPS",
            Proto::Sftp => "SFTP",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Proto::Ftp | Proto::Ftpes => 21,
            Proto::Ftps => 990,
            Proto::Sftp => 22,
        }
    }

    pub fn is_tls(self) -> bool {
        !matches!(self, Proto::Ftp)
    }

    pub fn parse(s: &str) -> Proto {
        match s.to_ascii_lowercase().as_str() {
            "ftpes" | "ftp-ssl" | "explicit" => Proto::Ftpes,
            "ftps" | "implicit" => Proto::Ftps,
            "sftp" | "ssh" | "scp" => Proto::Sftp,
            _ => Proto::Ftp,
        }
    }

    /// Identique à `Protocol` de FileZilla dans sitemanager.xml.
    pub fn to_fz(self) -> u8 {
        match self {
            Proto::Ftp => 0,
            Proto::Ftpes => 1,
            Proto::Ftps => 4,
            Proto::Sftp => 3,
        }
    }

    pub fn from_fz(v: u8) -> Proto {
        match v {
            1 => Proto::Ftpes,
            4 => Proto::Ftps,
            3 => Proto::Sftp,
            _ => Proto::Ftp,
        }
    }

    pub fn scheme(self) -> &'static str {
        match self {
            Proto::Ftp => "ftp",
            Proto::Ftpes => "ftp",
            Proto::Ftps => "ftps",
            Proto::Sftp => "sftp",
        }
    }
}

/// Méthode d'authentification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LoginType {
    #[default]
    #[serde(alias = "Normal")]
    Normal,
    #[serde(alias = "Anonymous")]
    Anonymous,
    /// Challenge clavier (SSH) ou mot de passe demandé à la volée.
    #[serde(alias = "Interactive")]
    Interactive,
    /// Juste la clé, sans mot de passe.
    #[serde(alias = "KeyOnly", alias = "keyonly")]
    KeyOnly,
}

impl LoginType {
    pub fn parse(s: &str) -> LoginType {
        match s.to_ascii_lowercase().as_str() {
            "anonymous" => LoginType::Anonymous,
            "interactive" => LoginType::Interactive,
            "key" | "keyonly" | "cert" | "certificate" => LoginType::KeyOnly,
            _ => LoginType::Normal,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            LoginType::Normal => "Normale",
            LoginType::Anonymous => "Anonyme",
            LoginType::Interactive => "Interactive (ask-for-password)",
            LoginType::KeyOnly => "Clé (nom d'utilisateur facultatif)",
        }
    }
}

/// Niveau de sévérité TLS d'un site.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TlsLevel {
    /// Clar, sans TLS.
    #[serde(rename = "plain", alias = "Plain")]
    Plain,
    /// Utiliser le TLS si le serveur le propose (repli clair).
    #[default]
    #[serde(rename = "if", alias = "IfAvailable")]
    IfAvailable,
    /// Exiger un TLS/SSL explicite.
    #[serde(rename = "require", alias = "RequireExplicit")]
    RequireExplicit,
    /// Exiger TLS+PFS (tout refuser d'autre).
    #[serde(rename = "pfs", alias = "RequirePfs")]
    RequirePfs,
}

impl TlsLevel {
    pub fn parse(s: &str) -> TlsLevel {
        match s.to_ascii_lowercase().as_str() {
            "plain" | "none" | "clear" => TlsLevel::Plain,
            "if" | "ifavailable" | "try" => TlsLevel::IfAvailable,
            "require" | "explicit" | "requireexplicit" => TlsLevel::RequireExplicit,
            "pfs" | "requirepfs" => TlsLevel::RequirePfs,
            _ => TlsLevel::IfAvailable,
        }
    }
}

/// Un site du gestionnaire de sites.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Site {
    pub id: String,
    pub name: String,
    /// Groupe du gestionnaire de sites (chemin séparé par `/`), pour l'arbre.
    #[serde(default)]
    pub group: String,
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    pub proto: Proto,
    #[serde(default)]
    pub login: LoginType,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    /// Chemin du fichier de clé privée (SFTP).
    #[serde(default)]
    pub keyfile: String,
    /// Répertoire distant initial (`/` = auto).
    #[serde(default = "default_dir")]
    pub remote_dir: String,
    /// Dernier répertoire local utilisé pour ce site.
    #[serde(default)]
    pub local_dir: String,
    /// Nombre maximum de connexions simultanées (`None` = réglage global).
    #[serde(default)]
    pub max_connections: Option<usize>,
    /// Limiter le nombre de connexions.
    #[serde(default)]
    pub limit_connections: bool,
    #[serde(default)]
    pub tls: TlsLevel,
    /// Nom d'hôte virtuel (`HOST`) envoyé si le serveur le demande.
    #[serde(default)]
    pub virtual_host: String,
    /// Activer FTPS sur ce site même si Proto est Ftp.
    #[serde(default)]
    pub use_tls: bool,
    /// Mode passif pour ce site (`None` = réglage global).
    #[serde(default)]
    pub passive: Option<bool>,
    /// Forcer l'encodage UTF-8.
    #[serde(default)]
    pub force_utf8: bool,
    /// Ne pas envoyer de NOOP de keep-alive.
    #[serde(default)]
    pub disable_keepalive: bool,
    /// Commandes avant/après connexion.
    #[serde(default)]
    pub pre_commands: Vec<String>,
    #[serde(default)]
    pub post_commands: Vec<String>,
    /// Commandes `SITE` pour chmod/chown/utime.
    #[serde(default)]
    pub site_chmod: bool,
    #[serde(default)]
    pub site_chown: bool,
    /// Note libre.
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub disabled: bool,
    /// horodatés (millisecondes epoch)
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub modified: i64,
    /// Fichiers distants à ne jamais mettre en file d'attente.
    #[serde(default)]
    pub last_path: String,
    /// Bookmarks de chemins distants propres au site.
    #[serde(default)]
    pub bookmarks: Vec<String>,
    /// empreinte de clé hôte SSH acceptée (SFTP).
    #[serde(default)]
    pub hostkey_fp: String,
}

fn default_dir() -> String {
    "/".into()
}

impl Site {
    pub fn port(&self) -> u16 {
        self.port.unwrap_or_else(|| self.proto.default_port())
    }

    pub fn full_name(&self) -> String {
        if self.group.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.group.trim_matches('/'), self.name)
        }
    }

    /// Identité affichée dans la barre de statut / le journal.
    pub fn display(&self) -> String {
        let up = if self.user.is_empty() {
            String::new()
        } else {
            format!("{}@", self.user)
        };
        format!("{}{}:{}", up, self.host, self.port())
    }

    pub fn url(&self) -> String {
        format!("{}://{}/", self.proto.scheme(), self.display())
    }
}

impl Default for Site {
    fn default() -> Self {
        Site {
            id: String::new(),
            name: String::new(),
            group: String::new(),
            host: String::new(),
            port: None,
            proto: Proto::Ftp,
            login: LoginType::Normal,
            user: String::new(),
            password: String::new(),
            keyfile: String::new(),
            remote_dir: default_dir(),
            local_dir: String::new(),
            max_connections: None,
            limit_connections: false,
            tls: TlsLevel::IfAvailable,
            virtual_host: String::new(),
            use_tls: false,
            passive: None,
            force_utf8: false,
            disable_keepalive: false,
            pre_commands: Vec::new(),
            post_commands: Vec::new(),
            site_chmod: true,
            site_chown: false,
            comment: String::new(),
            disabled: false,
            created: 0,
            modified: 0,
            last_path: String::new(),
            bookmarks: Vec::new(),
            hostkey_fp: String::new(),
        }
    }
}

/// Types d'entrées retournés par un listing.
/// Le fil est *minuscule* sur le réseau (`"dir"`, `"file"`, …), attendu par
/// l'interface web ; les formes `"Dir"`/`"File"` restent acceptées en lecture
/// pour tolérer les réglages et caches écrits par les versions antérieures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    #[serde(alias = "File")]
    File,
    #[serde(alias = "Dir")]
    Dir,
    #[serde(alias = "Symlink")]
    Symlink,
    #[serde(alias = "Other")]
    Other,
}

impl EntryKind {
    pub fn label(self) -> &'static str {
        match self {
            EntryKind::Dir => "Répertoire",
            EntryKind::File => "Fichier",
            EntryKind::Symlink => "Lien symbolique",
            EntryKind::Other => "Spécial",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            EntryKind::Dir => "d",
            EntryKind::File => "f",
            EntryKind::Symlink => "l",
            EntryKind::Other => "?",
        }
    }
}

/// Une entrée de répertoire (locale ou distante), moulée pour les deux panneaux.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub mtime: Option<i64>,
    pub mode: Option<u32>,
    pub user: Option<String>,
    pub group: Option<String>,
    pub link: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    /// Répertoire parent affiché (utile pour les recherches).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<String>,
}

impl Entry {
    pub fn file(name: String, size: u64, mtime: Option<i64>) -> Self {
        Entry {
            name,
            kind: EntryKind::File,
            size,
            mtime,
            mode: None,
            user: None,
            group: None,
            link: None,
            hidden: false,
            path: None,
        }
    }
    pub fn dir(name: String, mtime: Option<i64>) -> Self {
        let hidden = name.starts_with('.');
        Entry {
            kind: EntryKind::Dir,
            name,
            size: 0,
            mtime,
            mode: None,
            user: None,
            group: None,
            link: None,
            hidden,
            path: None,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Dir
    }
    pub fn perm_str(&self) -> String {
        match self.mode {
            Some(m) => crate::util::mode_to_str(m, self.kind == EntryKind::Dir),
            None => String::new(),
        }
    }
}

/// Résultat complet d'un listing.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Listing {
    pub path: String,
    pub entries: Vec<Entry>,
    /// « MLSD » | « LIST » | « ssh » | « local »
    pub source: String,
    pub raw: String,
    #[serde(default)]
    pub truncated: bool,
}

/// Nature d'une différence entre le panneau local et le panneau distant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffKind {
    /// seulement présent là-bas
    RemoteOnly,
    /// seulement présent ici
    LocalOnly,
    /// les deux, mais taille ou date différente
    Different,
    /// identique
    Same,
}

impl DiffKind {
    pub fn label(self) -> &'static str {
        match self {
            DiffKind::RemoteOnly => "manquant",
            DiffKind::LocalOnly => "unique ici",
            DiffKind::Different => "différent",
            DiffKind::Same => "identique",
        }
    }
}

/// Une entrée comparée, avec les deux tailles et les deux dates.
#[derive(Clone, Debug, Serialize)]
pub struct DiffItem {
    pub path: String,
    pub kind: DiffKind,
    pub dir: bool,
    pub local_size: u64,
    pub remote_size: u64,
    pub local_mtime: i64,
    pub remote_mtime: i64,
}

/// Résultat d'une comparaison de dossiers, comme « Comparaison des dossiers »
/// de FileZilla.
#[derive(Clone, Debug, Serialize)]
pub struct CompareReport {
    pub local: String,
    pub remote: String,
    pub items: Vec<DiffItem>,
    pub same: usize,
    pub different: usize,
    pub local_only: usize,
    pub remote_only: usize,
    pub bytes_to_download: u64,
    pub bytes_to_upload: u64,
}

/// Commande de changement d'état distante.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RemoteOp {
    Mkdir(String),
    Rmdir(String),
    Remove(String),
    Rename { from: String, to: String },
    Chmod { path: String, mode: u32 },
    Chown { path: String, user: String },
    SetTimes { path: String, mtime: i64 },
    Touch(String),
}

/// Informations de connexion affichables.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConnInfo {
    pub site_id: String,
    pub proto: Proto,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub tls: bool,
    pub tls_version: String,
    pub cipher: String,
    pub local_addr: String,
    pub remote_addr: String,
    pub banner: String,
    pub server_type: String,
    pub active: bool,
    pub connections: usize,
    pub max_connections: usize,
    pub since: i64,
    #[serde(default)]
    pub hostkey_fp: String,
    #[serde(default)]
    pub features: Vec<String>,
    /// Répertoire distant où le panneau doit s'ouvrir au départ : le chemin
    /// demandé par le site quand il en demande un, sinon le chemin d'attache
    /// du serveur. Sans lui, le gestionnaire de sites n'aurait servi à rien.
    #[serde(default)]
    pub server_dir: String,
}

impl ConnInfo {
    pub fn label(&self) -> String {
        let u = if self.user.is_empty() {
            String::new()
        } else {
            format!("{}@", self.user)
        };
        format!("{}://{}{}:{}", self.proto.name().to_lowercase(), u, self.host, self.port)
    }
}

/// Un signet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    /// local | remote
    pub scope: String,
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub site_id: String,
    #[serde(default)]
    pub added: i64,
}

/// Une ligne de journal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogLine {
    pub ts: i64,
    /// server | file | transfers | general | status
    pub cat: String,
    /// info | warn | error | success
    pub level: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub conn: Option<String>,
}

impl LogLine {
    pub fn new(cat: &str, level: &str, text: impl Into<String>) -> Self {
        LogLine {
            ts: crate::util::now_ms(),
            cat: cat.to_string(),
            level: level.to_string(),
            text: text.into(),
            conn: None,
        }
    }
}

/// État d'une file de transfert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    Waiting,
    Active,
    Done,
    Error,
    Cancelled,
    Paused,
}

impl JobState {
    /// un job terminé ne demande plus aucun travail du moteur
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Done | JobState::Error | JobState::Cancelled)
    }

    pub fn label(self) -> &'static str {
        match self {
            JobState::Waiting => "En attente",
            JobState::Active => "Transfert en cours",
            JobState::Done => "Terminé",
            JobState::Error => "Erreur",
            JobState::Cancelled => "Annulé",
            JobState::Paused => "En pause",
        }
    }
}

/// Une ligne de la file de transferts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferJob {
    pub id: String,
    pub site_id: String,
    pub site_name: String,
    pub direction: Direction,
    pub remote: String,
    pub local: String,
    pub size: u64,
    pub transferred: u64,
    pub state: JobState,
    pub priority: Priority,
    pub transfer_type: String,
    pub started: i64,
    pub finished: i64,
    pub speed: f64,
    pub error: String,
    pub attempts: u32,
    pub path_chain: String,
    pub resume_from: u64,
    /// Dernier instant où le moteur a vu des octets avancer. Sert de garde-fou :
    /// un canal qui se tait sans rendre les armes laisserait sinon le créneau
    /// du parc occupé jusqu'au redémarrage du service.
    #[serde(default, skip)]
    pub last_step: i64,
    #[serde(default)]
    pub kind: EntryKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
}

impl Direction {
    pub fn label(self) -> &'static str {
        match self {
            Direction::Up => "Envoi",
            Direction::Down => "Réception",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum Priority {
    Low,
    #[default]
    Normal,
    High,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "Basse",
            Priority::Normal => "Normale",
            Priority::High => "Haute",
        }
    }
}

/// Un point sur la connection courante pour affichage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PanelInfo {
    pub site_id: String,
    pub path: String,
    pub connected: bool,
    pub info: Option<ConnInfo>,
}

/// Dossier parcouru à la demande du navigateur (picker local).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    pub has_children: bool,
}

/// Récapitulatif des capacités.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Capabilities {
    pub protos: Vec<String>,
    pub features: Vec<String>,
    pub version: String,
}

impl TransferJob {
    pub fn new(
        site_id: String,
        site_name: String,
        direction: Direction,
        remote: String,
        local: String,
        size: u64,
    ) -> Self {
        TransferJob {
            id: crate::util::short_id(),
            site_id,
            site_name,
            direction,
            remote,
            local,
            size,
            transferred: 0,
            state: JobState::Waiting,
            priority: Priority::Normal,
            transfer_type: "auto".into(),
            started: 0,
            finished: 0,
            speed: 0.0,
            error: String::new(),
            attempts: 0,
            path_chain: String::new(),
            resume_from: 0,
            last_step: 0,
            kind: EntryKind::File,
        }
    }

    pub fn elapsed(&self) -> i64 {
        if self.started == 0 {
            0
        } else {
            let end = if self.finished > 0 {
                self.finished
            } else {
                crate::util::now_ms()
            };
            (end - self.started) / 1000
        }
    }

    pub fn remaining_secs(&self) -> Option<i64> {
        if self.state != JobState::Active || self.speed <= 1.0 || self.size == 0 {
            return None;
        }
        let left = self.size.saturating_sub(self.transferred) as f64;
        Some((left / self.speed) as i64)
    }
}

/// Paramètres de la file, renvoyés à la UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueStats {
    pub pending: usize,
    pub active: usize,
    pub done: usize,
    pub failed: usize,
    pub paused: bool,
    pub bytes_per_sec: f64,
    pub total_bytes: u64,
    pub done_bytes: u64,
}

/// Une entrée « journal serveur » pour la vue texte.
pub type LogBuffer = Vec<LogLine>;

/// Attributs de chemin local.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LocalStat {
    pub path: PathBuf,
    pub entry: Entry,
}
