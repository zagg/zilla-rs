//! Persistance : réglages, sites, historiques, signets, file d'attente.
//! Le layout est proche de FileZilla (`recvar.xml`, `sitemanager.xml`,
//! `transferqueue.xml`) mais stocké en JSON + un import/export XML compatible.

use crate::models::{Bookmark, Priority, Site, TransferJob};
use crate::settings::{deep_merge, Settings};
use crate::util::{now_ms, short_id};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Store {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub sites: Vec<Site>,
    /// Hôtes récents pour la connexion rapide.
    #[serde(default)]
    pub recent_hosts: Vec<String>,
    /// Derniers répertoires (locaux et distants).
    #[serde(default)]
    pub recent_local: Vec<String>,
    #[serde(default)]
    pub recent_remote: Vec<String>,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    pub queue: Vec<TransferJob>,
    /// Ignoré côté persistance.
    #[serde(skip, default)]
    pub dir: PathBuf,
}

impl Store {
    pub fn path_for(dir: &Path, name: &str) -> PathBuf {
        dir.join(name)
    }

    pub fn load(dir: &Path) -> std::io::Result<Store> {
        fs::create_dir_all(dir)?;
        fs::create_dir_all(dir.join("logs"))?;
        let mut store = Store {
            dir: dir.to_path_buf(),
            ..Default::default()
        };
        if let Some(v) = read_json(&dir.join("settings.json"))? {
            if let Ok(s) = serde_json::from_value::<Settings>(v) {
                store.settings = s;
            }
        }
        if let Some(v) = read_json(&dir.join("sitemanager.json"))? {
            if let Ok(s) = serde_json::from_value::<Vec<Site>>(v) {
                store.sites = s;
            }
        }
        if let Some(v) = read_json(&dir.join("ui.json"))? {
            let m = v.as_object().cloned().unwrap_or_default();
            let mut m2 = m.clone();
            store.recent_hosts = take_strs(&mut m2, "recent_hosts");
            store.recent_local = take_strs(&mut m2, "recent_local");
            store.recent_remote = take_strs(&mut m2, "recent_remote");
            if let Some(bm) = m.get("bookmarks") {
                store.bookmarks = serde_json::from_value(bm.clone()).unwrap_or_default();
            }
        }
        if let Some(v) = read_json(&dir.join("transferqueue.json"))? {
            if let Ok(q) = serde_json::from_value::<Vec<TransferJob>>(v) {
                store.queue = q;
            }
        }
        // Normalisation : ids manquants.
        for s in &mut store.sites {
            if s.id.is_empty() {
                s.id = short_id();
            }
        }
        Ok(store)
    }

    pub fn save_settings(&self) -> std::io::Result<()> {
        atomic_write(
            &self.dir.join("settings.json"),
            &serde_json::to_string_pretty(&self.settings)?,
        )
    }

    pub fn save_sites(&self) -> std::io::Result<()> {
        atomic_write(
            &self.dir.join("sitemanager.json"),
            &serde_json::to_string_pretty(&self.sites)?,
        )
    }

    pub fn save_ui(&self) -> std::io::Result<()> {
        let mut m = serde_json::Map::new();
        m.insert("recent_hosts".into(), serde_json::json!(self.recent_hosts));
        m.insert("recent_local".into(), serde_json::json!(self.recent_local));
        m.insert("recent_remote".into(), serde_json::json!(self.recent_remote));
        m.insert("bookmarks".into(), serde_json::json!(self.bookmarks));
        atomic_write(
            &self.dir.join("ui.json"),
            &serde_json::to_string_pretty(&Value::Object(m))?,
        )
    }

    pub fn save_queue(&self) -> std::io::Result<()> {
        atomic_write(
            &self.dir.join("transferqueue.json"),
            &serde_json::to_string_pretty(&self.queue)?,
        )
    }

    pub fn update_settings(&mut self, patch: &Value) -> Result<(), String> {
        let mut cur = serde_json::to_value(&self.settings).map_err(|e| e.to_string())?;
        deep_merge(&mut cur, patch);
        let s: Settings = serde_json::from_value(cur).map_err(|e| format!("réglages invalides : {e}"))?;
        self.settings = s;
        self.save_settings().map_err(|e| e.to_string())
    }

    pub fn push_recent_host(&mut self, host: String) {
        if host.trim().is_empty() {
            return;
        }
        self.recent_hosts.retain(|h| h != &host);
        self.recent_hosts.insert(0, host);
        self.recent_hosts.truncate(15);
        let _ = self.save_ui();
    }

    pub fn push_recent_local(&mut self, p: String) {
        if p.is_empty() {
            return;
        }
        self.recent_local.retain(|h| h != &p);
        self.recent_local.insert(0, p);
        self.recent_local.truncate(15);
        let _ = self.save_ui();
    }

    pub fn push_recent_remote(&mut self, p: String) {
        if p.is_empty() {
            return;
        }
        self.recent_remote.retain(|h| h != &p);
        self.recent_remote.insert(0, p);
        self.recent_remote.truncate(15);
        let _ = self.save_ui();
    }

    pub fn find_site(&self, id: &str) -> Option<&Site> {
        self.sites.iter().find(|s| s.id == id)
    }

    pub fn find_site_mut(&mut self, id: &str) -> Option<&mut Site> {
        self.sites.iter_mut().find(|s| s.id == id)
    }

    pub fn upsert_site(&mut self, mut site: Site) -> Site {
        if site.id.is_empty() {
            site.id = short_id();
        }
        if site.created == 0 {
            site.created = now_ms();
        }
        site.modified = now_ms();
        let replaced = self.sites.iter().position(|s| s.id == site.id);
        let out = site.clone();
        match replaced {
            Some(i) => self.sites[i] = site,
            None => self.sites.push(site),
        }
        let _ = self.save_sites();
        out
    }

    pub fn delete_site(&mut self, id: &str) {
        self.sites.retain(|s| s.id != id);
        for b in self.bookmarks.clone() {
            if b.site_id == id {
                self.bookmarks.retain(|x| x.id != b.id);
            }
        }
        let _ = self.save_sites();
        let _ = self.save_ui();
    }

    pub fn duplicate_site(&mut self, id: &str) -> Option<Site> {
        let mut s = self.find_site(id)?.clone();
        s.id = short_id();
        s.name = format!("{} (copie)", s.name);
        s.created = now_ms();
        s.modified = now_ms();
        let s = s.clone();
        self.sites.push(s.clone());
        let _ = self.save_sites();
        Some(s)
    }

    pub fn add_bookmark(&mut self, mut b: Bookmark) -> Bookmark {
        if b.id.is_empty() {
            b.id = short_id();
        }
        b.added = now_ms();
        self.bookmarks.push(b.clone());
        let _ = self.save_ui();
        b
    }

    pub fn remove_bookmark(&mut self, id: &str) {
        self.bookmarks.retain(|b| b.id != id);
        let _ = self.save_ui();
    }

    /// File d'attente persistée.
    pub fn save_queue_jobs(&mut self, jobs: &[TransferJob]) {
        self.queue = jobs
            .iter()
            .filter(|j| j.state != crate::models::JobState::Done)
            .cloned()
            .collect();
        let _ = self.save_queue();
    }
}

pub fn take_strs(m: &mut serde_json::Map<String, Value>, k: &str) -> Vec<String> {
    m.remove(k)
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Lecture JSON : `Ok(None)` si absent.
pub fn read_json(p: &Path) -> std::io::Result<Option<Value>> {
    match fs::read_to_string(p) {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v) => Ok(Some(v)),
            Err(e) => {
                let _ = fs::write(p.with_extension("corrupt"), &s);
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{} : {e}", p.display()),
                ))
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Écriture atomique.
pub fn atomic_write(p: &Path, data: &str) -> std::io::Result<()> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = p.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data.as_bytes())?;
        f.flush()?;
        let _ = f.sync_all();
    }
    fs::rename(&tmp, p)?;
    Ok(())
}

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s.as_bytes())
}

fn b64d(s: &str) -> String {
    match base64::engine::general_purpose::STANDARD.decode(s) {
        Ok(v) => String::from_utf8_lossy(&v).into_owned(),
        Err(_) => s.to_string(),
    }
}

/// Cherche un élément portant ce nom, en profondeur (borné à 6 niveaux).
fn find_element<'a, 'b>(node: roxmltree::Node<'a, 'b>, name: &str, depth: usize) -> Option<roxmltree::Node<'a, 'b>> {
    if depth > 6 {
        return None;
    }
    for c in node
        .children()
        .filter(|c| c.node_type() == roxmltree::NodeType::Element)
    {
        if c.has_tag_name(name) {
            return Some(c);
        }
        if let Some(f) = find_element(c, name, depth + 1) {
            return Some(f);
        }
    }
    None
}

/// Import du format `sitemanager.xml` de FileZilla.
pub fn import_sitemanager_xml(xml: &str) -> Result<Vec<Site>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("XML invalide : {e}"))?;
    let root = doc.root();
    let mut out = Vec::new();
    // <FileZilla3><Servers>…</Servers></FileZilla3> — on cherche « Servers » en profondeur.
    let servers = find_element(root, "Servers", 0);
    let container = match servers {
        Some(s) => s,
        None => root,
    };
    fn walk(node: roxmltree::Node, group: &str, out: &mut Vec<Site>) {
        for n in node
            .children()
            .filter(|c| c.has_tag_name("Server") && c.node_type() == roxmltree::NodeType::Element)
        {
            let mut s = Site::default();
            s.id = short_id();
            s.group = group.to_string();
            for c in n.children() {
                if !c.is_element() {
                    continue;
                }
                let tag = c.tag_name().name();
                let txt = c.text().unwrap_or("").to_string();
                match tag {
                    "Name" => s.name = txt.trim().to_string(),
                    "Host" => s.host = txt.trim().to_string(),
                    "Port" => s.port = txt.trim().parse::<u16>().ok(),
                    "Protocol" => s.proto = crate::models::Proto::from_fz(txt.trim().parse().unwrap_or(0)),
                    "User" => s.user = txt,
                    "Pass" => {
                        let enc = c.attribute("encoding").unwrap_or("");
                        s.password = if enc == "base64" { b64d(&txt) } else { txt };
                    }
                    "RemoteDir" | "RemoteDir2" => {
                        if !txt.trim().is_empty() {
                            s.remote_dir = txt.trim().to_string();
                        }
                    }
                    "LocalDir" | "LocalDir2" => {
                        if !txt.trim().is_empty() {
                            s.local_dir = txt.clone();
                        }
                    }
                    "LoginType" => {
                        s.login = match txt.trim() {
                            "1" => crate::models::LoginType::Anonymous,
                            "2" => crate::models::LoginType::Interactive,
                            "4" | "5" => crate::models::LoginType::KeyOnly,
                            _ => crate::models::LoginType::Normal,
                        }
                    }
                    "MaxConnections" => s.max_connections = txt.trim().parse().ok(),
                    "LimitNumberOfConnections" => s.limit_connections = txt.trim() == "1",
                    "Comment" => s.comment = txt,
                    "TLS" => {
                        s.use_tls = txt.trim() == "1";
                        if s.use_tls && s.proto == crate::models::Proto::Ftp {
                            s.proto = crate::models::Proto::Ftpes;
                        }
                    }
                    "FtpPlainFallback" => {
                        if txt.trim() == "1" {
                            s.tls = crate::models::TlsLevel::IfAvailable;
                        }
                    }
                    "PrivateKey" => s.keyfile = txt,
                    "HostkeyAlgo" | "NameVirtualHost" => {
                        if tag == "NameVirtualHost" {
                            s.virtual_host = txt;
                        }
                    }
                    "FTE-PRE-Commands" | "precommand" => {
                        if !txt.trim().is_empty() {
                            s.pre_commands = txt.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                        }
                    }
                    "FTE-POST-Commands" | "postcommand" => {
                        if !txt.trim().is_empty() {
                            s.post_commands = txt.split(';').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                        }
                    }
                    _ => {}
                }
            }
            let nested_group = n
                .children()
                .any(|c| c.has_tag_name("Server"));
            if nested_group {
                let g = if group.is_empty() {
                    s.name.clone()
                } else {
                    format!("{}/{}", group, s.name)
                };
                walk(n, &g, out);
            } else {
                if s.proto == crate::models::Proto::Sftp {
                    if let Some(_p) = s.port.filter(|p| *p == 0) {
                        s.port = Some(22);
                    }
                }
                if s.name.is_empty() {
                    s.name = s.host.clone();
                }
                out.push(s);
            }
        }
    }
    walk(container, "", &mut out);
    if out.is_empty() {
        return Err("aucun serveur défini dans ce fichier".into());
    }
    Ok(out)
}

/// Export `sitemanager.xml` (compatible FileZilla).
pub fn export_sitemanager_xml(sites: &[Site]) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<FileZilla3 version=\"3.68.1\" platform=\"*nix\">\n\t<Servers>\n");
    // Groupes d'abord, pour l'arbre.
    let mut groups: Vec<String> = Vec::new();
    for site in sites {
        if !site.group.is_empty() && !groups.contains(&site.group) {
            groups.push(site.group.clone());
        }
    }
    groups.sort();
    for g in &groups {
        let top = g.split('/').next().unwrap_or(g).to_string();
        let mut sub: Vec<String> = groups
            .iter()
            .filter(|x| **x != *top && x.starts_with(&format!("{top}/")))
            .cloned()
            .collect();
        sub.sort();
        let direct: Vec<&Site> = sites
            .iter()
            .filter(|s| s.group == top)
            .collect();
        s.push_str(&format!("\t\t<Server CompactItem=\"1\"><Name>{}</Name>\n", esc(&top)));
        for subg in sub {
            let name = subg.trim_start_matches(&format!("{top}/")).to_string();
            let inner: Vec<&Site> = sites
                .iter()
                .filter(|s| {
                    s.group == subg || s.group.starts_with(&format!("{subg}/"))
                })
                .collect();
            s.push_str(&format!("\t\t\t<Server CompactItem=\"1\"><Name>{}</Name>\n", esc(&name)));
            for x in inner {
                write_server(&mut s, x, 4);
            }
            s.push_str("\t\t\t</Server>\n");
        }
        for d in direct {
            write_server(&mut s, d, 3);
        }
        s.push_str("\t\t</Server>\n");
    }
    for site in sites.iter().filter(|s| s.group.is_empty()) {
        write_server(&mut s, site, 2);
    }
    s.push_str("\t</Servers>\n</FileZilla3>\n");
    s
}

fn tag(s: &mut String, indent: &str, name: &str, value: &str) {
    s.push_str(&format!(
        "{ind}<{name}>{val}</{name}>\n",
        ind = indent,
        name = name,
        val = esc(value)
    ));
}

fn write_server(s: &mut String, site: &Site, indent: usize) {
    let i = "\t".repeat(indent);
    s.push_str(&format!(
        "{ind}<Server Type=\"{t}\" TransferType=\"0\" LogType=\"0\" PathCaseOverride=\"0\" DirTimeOffset=\"1\">\n",
        ind = i,
        t = site.proto.to_fz()
    ));
    let j = "\t".repeat(indent + 1);
    let kv = |s: &mut String, k: &str, v: &str| {
        let mut buf = String::new();
        tag(&mut buf, &j, k, v);
        s.push_str(&buf);
    };
    kv(s, "SftpAlternativePath", "1");
    kv(s, "Name", &site.name);
    kv(s, "Host", &site.host);
    kv(s, "Port", &site.port().to_string());
    kv(s, "Protocol", &site.proto.to_fz().to_string());
    s.push_str(&format!(
        "{j}<Proxy host=\"0\" port=\"0\" username=\"\" password=\"\" type=\"0\" resolve_dns=\"1\" tls=\"1\" pasv=\"1\" same_user=\"1\"/>\n",
        j = j
    ));
    kv(s, "LogType", "0");
    kv(s, "DisableKeepAlive", if site.disable_keepalive { "1" } else { "0" });
    kv(s, "WaitContinue", "1");
    kv(s, "PreserveTimestamp", "1");
    kv(s, "RemoteSys", "1");
    kv(s, "NatType", "31");
    kv(s, "TLS", if site.proto.is_tls() { "1" } else { "0" });
    kv(s, "FtpPlainFallback", if site.proto == crate::models::Proto::Ftp { "1" } else { "1" });
    kv(s, "FtpUseMlst", "1");
    kv(s, "FtpUsePret", if crate::settings::Settings::default().ftp.use_pret { "1" } else { "0" });
    kv(s, "MaxConnections", &site.max_connections.unwrap_or(10).to_string());
    kv(s, "LimitNumberOfConnections", if site.limit_connections { "1" } else { "0" });
    kv(s, "Comment", &site.comment);
    kv(s, "Timeout", "0");
    if !site.pre_commands.is_empty() {
        kv(s, "FTE-PRE-Commands", &site.pre_commands.join("; "));
    }
    if !site.post_commands.is_empty() {
        kv(s, "FTE-POST-Commands", &site.post_commands.join("; "));
    }
    if !site.virtual_host.is_empty() {
        kv(s, "NameVirtualHost", &site.virtual_host);
    }
    let login = match site.login {
        crate::models::LoginType::Normal => 0,
        crate::models::LoginType::Anonymous => 1,
        crate::models::LoginType::Interactive => 2,
        crate::models::LoginType::KeyOnly => 4,
    };
    kv(s, "LoginType", &login.to_string());
    kv(s, "User", &site.user);
    s.push_str(&format!(
        "{j}<Pass encoding=\"base64\">{}</Pass>\n",
        esc(&b64(&site.password))
    ));
    if !site.keyfile.is_empty() {
        kv(s, "PrivateKey", &site.keyfile);
    }
    kv(s, "RemoteDir", &site.remote_dir);
    if !site.local_dir.is_empty() {
        kv(s, "LocalDir", &site.local_dir);
    }
    kv(s, "Encoding", "0");
    kv(s, "Created", &site.created.to_string());
    kv(s, "Modified", &site.modified.to_string());
    if !site.bookmarks.is_empty() {
        s.push_str(&format!("{j}<Bookmarks>\n", j = j));
        for b in &site.bookmarks {
            s.push_str(&format!(
                "{j}\t<Name>{n}</Name><Folder>2</Folder>\n",
                j = j,
                n = esc(b)
            ));
        }
        s.push_str(&format!("{j}</Bookmarks>\n", j = j));
    }
    if !site.hostkey_fp.is_empty() {
        tag(s, &j, "HostkeyFingerprint", &site.hostkey_fp);
    }
    s.push_str(&format!("{ind}</Server>\n", ind = i));
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Format `transferqueue.xml` simplifié (compat partiel).
pub fn import_queue_xml(xml: &str) -> Result<Vec<TransferJob>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let mut jobs = Vec::new();
    for node in doc.root().descendants().filter(|n| n.has_tag_name("File")) {
        let local = node.children().find(|c| c.has_tag_name("LocalName")).and_then(|c| c.text().map(|t| t.to_string()));
        let remote = node
            .children()
            .find(|c| c.has_tag_name("RemoteName"))
            .and_then(|c| c.text().map(|t| t.to_string()))
            .unwrap_or_default();
        let Some(local) = local else { continue };
        let dir = node
            .children()
            .find(|c| c.has_tag_name("Direction"))
            .and_then(|c| c.text().map(|t| t.trim().to_string()))
            .unwrap_or_else(|| "1".into());
        let mut j = TransferJob::new(
            String::new(),
            String::new(),
            if dir == "0" { crate::models::Direction::Up } else { crate::models::Direction::Down },
            remote,
            local,
            node.children()
                .find(|c| c.has_tag_name("Size"))
                .and_then(|c| c.text().and_then(|t| t.trim().parse().ok()))
                .unwrap_or(0),
        );
        j.state = crate::models::JobState::Waiting;
        j.priority = Priority::Normal;
        jobs.push(j);
    }
    if jobs.is_empty() {
        return Err("aucun transfert dans ce fichier".into());
    }
    Ok(jobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_roundtrip() {
        let mut s = Site::default();
        s.id = "a1".into();
        s.name = "Serveur <dossier> & co".into();
        s.host = "ftp.example.org".into();
        s.port = Some(2121);
        s.user = "davy".into();
        s.password = "secret/".into();
        s.proto = crate::models::Proto::Ftpes;
        s.group = "Perso".into();
        let xml = export_sitemanager_xml(std::slice::from_ref(&s));
        assert!(xml.contains("<Host>ftp.example.org</Host>"));
        let back = import_sitemanager_xml(&xml).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].host, "ftp.example.org");
        assert_eq!(back[0].password, "secret/");
        assert_eq!(back[0].name, "Serveur <dossier> & co");
        assert_eq!(back[0].proto, crate::models::Proto::Ftpes);
        assert_eq!(back[0].group, "Perso");
    }

    #[test]
    fn nested_groups() {
        let xml = r#"<?xml version="1.0"?><FileZilla3 version="3.68"><Servers>
          <Server CompactItem="1"><Name>Pro</Name>
            <Server><Name>alpha</Name><Host>a.example</Host><Port>21</Port><Protocol>0</Protocol></Server>
          </Server>
          <Server><Name>dev</Name><Host>1.2.3.4</Host><Port>22</Port><Protocol>3</Protocol><User>r</User><Pass encoding="base64">cGFzcw==</Pass></Server>
        </Servers></FileZilla3>"#;
        let v = import_sitemanager_xml(xml).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].group, "Pro");
        assert_eq!(v[0].host, "a.example");
        assert_eq!(v[1].password, "pass");
        assert_eq!(v[1].proto, crate::models::Proto::Sftp);
    }
}
