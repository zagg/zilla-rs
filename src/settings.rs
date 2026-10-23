//! Réglages, équivalent des préférences `recvar` de FileZilla.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub general: General,
    pub net: Net,
    pub ftp: Ftp,
    pub transfers: Transfers,
    pub sftp: Sftp,
    pub tls: Tls,
    pub ui: Ui,
    pub logs: Logs,
    pub proxy: Proxy,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: 1,
            general: General::default(),
            net: Net::default(),
            ftp: Ftp::default(),
            transfers: Transfers::default(),
            sftp: Sftp::default(),
            tls: Tls::default(),
            ui: Ui::default(),
            logs: Logs::default(),
            proxy: Proxy::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct General {
    /// Langue de l'interface (fr | en).
    pub language: String,
    /// Thème de l'interface.
    pub theme: String,
    /// Taille de police (px).
    pub font_size: u16,
    /// Afficher les fichiers cachés dans le panneau local.
    pub show_hidden_local: bool,
    /// Confirmer avant suppression.
    pub confirm_delete: bool,
    /// Confirmer à la fermeture pendant un transfert.
    pub confirm_transfers: bool,
    /// Comportement au double-clic : transfer | view | edit.
    pub double_click: String,
    /// Utiliser la corbeille pour les suppressions locales.
    pub use_trash: bool,
    /// Notifier dans le titre de la fenêtre à la fin des transferts.
    pub notify_finish: bool,
}

impl Default for General {
    fn default() -> Self {
        General {
            language: "fr".into(),
            theme: "dark".into(),
            font_size: 13,
            show_hidden_local: true,
            confirm_delete: true,
            confirm_transfers: true,
            double_click: "transfer".into(),
            use_trash: false,
            notify_finish: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Net {
    pub connect_timeout: u64,
    pub response_timeout: u64,
    pub data_timeout: u64,
    /// NOOP après N secondes d'inactivité (keep-alive). 0 = désactivé.
    pub keepalive_idle: u64,
    /// Fréquence de vérification du keep-alive.
    pub keepalive_tick: u64,
    /// Délai de reconnexion automatique.
    pub reconnect_delay: u64,
    /// Tentatives de reconnexion.
    pub reconnect_tries: u32,
    /// Taille du tampon des sockets de données (Kio).
    pub buffer_size: usize,
    /// Forcer la famille d'adresses : auto | v4 | v6.
    pub ip_family: String,
    /// Résolveur d'IP externe pour le mode actif.
    pub external_ip: String,
    pub external_resolver_url: String,
}

impl Default for Net {
    fn default() -> Self {
        Net {
            connect_timeout: 20,
            response_timeout: 20,
            data_timeout: 0,
            keepalive_idle: 15,
            keepalive_tick: 5,
            reconnect_delay: 5,
            reconnect_tries: 0,
            buffer_size: 64,
            ip_family: "auto".into(),
            external_ip: String::new(),
            external_resolver_url: String::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ftp {
    /// Mode passif (recommandé), sinon mode actif.
    pub passive: bool,
    /// Utiliser EPSV/EPRT si le serveur les accepte.
    pub use_epsv: bool,
    /// Utiliser MLSD quand disponible.
    pub use_mlsd: bool,
    /// Utiliser PRET avant LIST dans un répertoire.
    pub use_pret: bool,
    /// Optique : préférer `SIZE` à un RETR pour connaître la taille.
    pub use_size: bool,
    /// Port minimum/maximum du mode actif.
    pub active_port_min: u16,
    pub active_port_max: u16,
    /// Activer UTF8 par défaut.
    pub use_utf8: bool,
    /// Jeu de caractères : auto | utf-8 | latin1 | gbk | sjis ...
    pub charset: String,
    /// Envoyer CLNT.
    pub send_clnt: bool,
    pub clnt_string: String,
    /// Nombre max de connexions par site.
    pub max_connections: usize,
    /// Utiliser la commande HOST.
    pub use_host: bool,
    /// Utiliser `SITE` pour chmod/chown/utime.
    pub site_chmod: bool,
    pub site_chown: bool,
    pub site_utime: bool,
    /// Utiliser MDTM / MFMT.
    pub use_mdtm: bool,
    pub use_mfmt: bool,
    /// Commandes pré-connectées globales.
    pub pre_connect: Vec<String>,
    /// Suite de commandes optionnelles si `AUTH TLS` est demandé.
    pub tls_protection: bool,
}

impl Default for Ftp {
    fn default() -> Self {
        Ftp {
            passive: true,
            use_epsv: true,
            use_mlsd: true,
            use_pret: false,
            use_size: true,
            active_port_min: 0,
            active_port_max: 0,
            use_utf8: true,
            charset: "auto".into(),
            send_clnt: true,
            clnt_string: "ZillaRS-0.1".into(),
            max_connections: 10,
            use_host: false,
            site_chmod: true,
            site_chown: false,
            site_utime: false,
            use_mdtm: true,
            use_mfmt: true,
            pre_connect: Vec::new(),
            tls_protection: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Transfers {
    /// Transferts simultanés.
    pub max_parallel: usize,
    /// Mode d'écrasement : ask | overwrite | skip | newer | rename.
    pub overwrite: String,
    /// Conserver les horodatages.
    pub preserve_mtime: bool,
    /// Reprise automatique sur les erreurs.
    pub auto_resume: bool,
    /// Nombre de tentatives sur erreur réseau.
    pub retries: u32,
    /// Délai avant nouvelle tentative (secondes).
    pub retry_delay: u64,
    /// Détection automatique ASCII (selon extension).
    pub ascii_detect: bool,
    pub ascii_extensions: Vec<String>,
    /// Taille de tampon des fichiers (Kio).
    pub chunk_size: usize,
    /// Suivre les liens symboliques dans les dossiers locaux.
    pub follow_symlinks: bool,
    /// Limite globale (Kio/s), 0 = illimitée.
    pub limit_total: u64,
    pub limit_upload: u64,
    pub limit_download: u64,
    /// Limiter les connexions par site pour les transferts.
    pub limit_per_site: bool,
    /// Lancer les transferts au démarrage.
    pub autostart: bool,
    /// Effacer la liste des transferts terminés à la fermeture.
    pub clear_finished: bool,
    /// Nommer les doublons à la manière FileZilla (`file.1.ext`).
    pub rename_suffix: u32,
}

impl Default for Transfers {
    fn default() -> Self {
        Transfers {
            max_parallel: 2,
            overwrite: "ask".into(),
            preserve_mtime: true,
            auto_resume: true,
            retries: 3,
            retry_delay: 5,
            ascii_detect: true,
            ascii_extensions: default_ascii_ext(),
            chunk_size: 32,
            follow_symlinks: false,
            limit_total: 0,
            limit_upload: 0,
            limit_download: 0,
            limit_per_site: false,
            autostart: true,
            clear_finished: false,
            rename_suffix: 1,
        }
    }
}

fn default_ascii_ext() -> Vec<String> {
    [
        "asm", "c", "cc", "cmd", "conf", "cpp", "cs", "css", "dat", "for", "f90", "go", "h",
        "htm", "html", "ini", "java", "js", "jsp", "log", "lua", "m", "make", "md", "php", "pl",
        "py", "rb", "rc", "sh", "shtml", "sql", "sty", "tex", "text", "tmpl", "txt", "xml",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Sftp {
    /// Suivre les liens symboliques côté distant.
    pub follow_symlinks: bool,
    /// Demander pour les clés hôtes inconnues.
    pub ask_hostkey: bool,
    /// Chemin de la clé privée par défaut.
    pub identity_file: String,
    /// Utiliser l'agent SSH si disponible.
    pub use_agent: bool,
    /// Compression.
    pub compression: bool,
    /// Taille max des paquets de lecture (Kio).
    pub read_size: usize,
}

impl Default for Sftp {
    fn default() -> Self {
        Sftp {
            follow_symlinks: false,
            ask_hostkey: true,
            identity_file: String::new(),
            use_agent: true,
            compression: false,
            read_size: 32,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Tls {
    /// Vérifier le certificat du serveur.
    pub verify_cert: bool,
    /// Autoriser les suites sans PFS.
    pub allow_non_pfs: bool,
    /// Revenir au FTP clair si TLS échoue.
    pub fallback: bool,
    /// Version minimale (1.2 | 1.3).
    pub min_version: String,
    /// Répertoires où chercher les certificats.
    pub ca_files: Vec<String>,
}

impl Default for Tls {
    fn default() -> Self {
        Tls {
            verify_cert: false,
            allow_non_pfs: true,
            fallback: true,
            min_version: "1.2".into(),
            ca_files: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ui {
    pub show_toolbar: bool,
    pub show_message_pane: bool,
    pub show_transfer_pane: bool,
    pub show_filters: bool,
    pub show_hidden_remote: bool,
    pub show_statusbar: bool,
    /// Colonnes locales.
    pub local_columns: Vec<String>,
    /// Colonnes distantes.
    pub remote_columns: Vec<String>,
    /// Largeur par défaut des colonnes.
    pub column_widths: std::collections::HashMap<String, u16>,
    /// Raccourcis clavier personnalisés.
    pub accent: String,
    /// Densité ("comfortable" | "compact").
    pub density: String,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            show_toolbar: true,
            show_message_pane: true,
            show_transfer_pane: true,
            show_filters: true,
            show_hidden_remote: true,
            show_statusbar: true,
            local_columns: vec![
                "name".into(),
                "size".into(),
                "type".into(),
                "modified".into(),
                "perm".into(),
            ],
            remote_columns: vec![
                "name".into(),
                "size".into(),
                "type".into(),
                "modified".into(),
                "perm".into(),
                "user".into(),
                "group".into(),
            ],
            column_widths: Default::default(),
            accent: "auto".into(),
            density: "comfortable".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Logs {
    /// Niveau : normal | verbose.
    pub level: String,
    /// Écrire aussi dans un fichier.
    pub to_file: bool,
    pub file_path: String,
    /// Historique conservé en mémoire.
    pub memory_lines: usize,
    /// Journaliser les échanges de commandes (verbose).
    pub trace_commands: bool,
    /// Horodater les lignes.
    pub timestamps: bool,
}

impl Default for Logs {
    fn default() -> Self {
        Logs {
            level: "normal".into(),
            to_file: false,
            file_path: String::new(),
            memory_lines: 5000,
            trace_commands: false,
            timestamps: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Proxy {
    pub enabled: bool,
    /// none | http | socks5 | socks4
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    /// Résoudre les noms via le proxy.
    pub resolve_dns: bool,
    /// Utiliser les mêmes identifiants que le site.
    pub same_credentials: bool,
    pub tls: bool,
}

impl Default for Proxy {
    fn default() -> Self {
        Proxy {
            enabled: false,
            kind: "none".into(),
            host: String::new(),
            port: 0,
            user: String::new(),
            password: String::new(),
            resolve_dns: false,
            same_credentials: false,
            tls: false,
        }
    }
}

impl Settings {
    /// Vitesse max (octets/s) pour une direction, ou None si illimitée.
    pub fn speed_limit(&self, up: bool) -> Option<u64> {
        let v = if up {
            self.transfers.limit_upload
        } else {
            self.transfers.limit_download
        };
        let v = if v == 0 { self.transfers.limit_total } else { v };
        if v == 0 {
            None
        } else {
            Some(v * 1024)
        }
    }

    pub fn ascii_ext_set(&self) -> Vec<String> {
        self.transfers
            .ascii_extensions
            .iter()
            .map(|s| s.to_ascii_lowercase())
            .collect()
    }
}

/// Fusion profonde (`patch` dans un objet `current`) façon JSON Merge Patch étendu.
pub fn deep_merge(current: &mut serde_json::Value, patch: &serde_json::Value) {
    match (current, patch) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in b {
                let entry = a
                    .entry(k.clone())
                    .or_insert_with(|| serde_json::Value::Null);
                deep_merge(entry, v);
            }
        }
        (a, b) => {
            *a = b.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_and_roundtrip() {
        let base = serde_json::to_value(Settings::default()).unwrap();
        let patch: serde_json::Value =
            serde_json::from_str(r#"{"ftp":{"passive":false},"transfers":{"max_parallel":7}}"#)
                .unwrap();
        let mut cur = base;
        deep_merge(&mut cur, &patch);
        let s: Settings = serde_json::from_value(cur).unwrap();
        assert!(!s.ftp.passive);
        assert_eq!(s.transfers.max_parallel, 7);
        assert_eq!(s.net.connect_timeout, 20);
    }

    #[test]
    fn speeds() {
        let mut s = Settings::default();
        assert_eq!(s.speed_limit(false), None);
        s.transfers.limit_total = 100;
        assert_eq!(s.speed_limit(true), Some(100 * 1024));
    }
}
