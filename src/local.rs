//! Panneau local : accès au disque de la machine qui héberge Zilla-RS.

use crate::fs::{FsError, FsResult};
#[allow(unused_imports)]
use tokio::io::AsyncWriteExt;
use crate::models::{Entry, EntryKind, Listing};
use crate::util::{local_normalize, now_ms};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::collections::HashMap;

fn passwd_map() -> &'static HashMap<u32, String> {
    static MAP: OnceLock<HashMap<u32, String>> = OnceLock::new();
    MAP.get_or_init(|| parse_etc("/etc/passwd", 0))
}

fn group_map() -> &'static HashMap<u32, String> {
    static MAP: OnceLock<HashMap<u32, String>> = OnceLock::new();
    MAP.get_or_init(|| parse_etc("/etc/group", 3))
}

fn parse_etc(path: &str, field: usize) -> HashMap<u32, String> {
    let mut m = HashMap::new();
    if let Ok(s) = std::fs::read_to_string(path) {
        for line in s.lines() {
            let p: Vec<&str> = line.split(':').collect();
            if p.len() > field + 1 {
                if let Ok(id) = p[field + 1].parse::<u32>() {
                    m.insert(id, p[field].to_string());
                }
            }
        }
    }
    m
}

/// Nom d'utilisateur pour un uid.
pub fn user_name(uid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        return passwd_map().get(&uid).cloned();
    }
    #[cfg(not(unix))]
    {
        let _ = uid;
        None
    }
}

pub fn group_name(gid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        return group_map().get(&gid).cloned();
    }
    #[cfg(not(unix))]
    {
        let _ = gid;
        None
    }
}

#[cfg(unix)]
fn ids(md: &std::fs::Metadata) -> (Option<u32>, Option<u32>) {
    use std::os::unix::fs::MetadataExt;
    (Some(md.uid()), Some(md.gid()))
}

#[cfg(not(unix))]
fn ids(_md: &std::fs::Metadata) -> (Option<u32>, Option<u32>) {
    (None, None)
}

#[cfg(unix)]
fn mode_of(md: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(md.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_of(_md: &std::fs::Metadata) -> Option<u32> {
    None
}

/// Liste un répertoire local.
pub fn list(path: &Path, show_hidden: bool, follow_symlinks: bool) -> FsResult<Listing> {
    let p = local_normalize(&path.to_string_lossy());
    let rd = std::fs::read_dir(&p).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            FsError::NotFound(p.display().to_string())
        } else {
            FsError::Denied(format!("{} — {e}", p.display()))
        }
    })?;
    let mut entries = Vec::new();
    let root = p.to_string_lossy().to_string();
    for de in rd {
        let de = match de {
            Ok(d) => d,
            Err(_) => continue,
        };
        let name = de.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let full = de.path();
        let md = if follow_symlinks {
            std::fs::metadata(&full)
        } else {
            std::fs::symlink_metadata(&full)
        };
        let md = match md {
            Ok(m) => m,
            Err(_) => continue,
        };
        let kind = if md.is_dir() {
            EntryKind::Dir
        } else if md.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let (uid, gid) = ids(&md);
        let link = if kind == EntryKind::Other || is_symlink(&full) {
            std::fs::read_link(&full).ok().map(|l| l.to_string_lossy().to_string())
        } else {
            None
        };
        entries.push(Entry {
            name: name.clone(),
            kind,
            size: if kind == EntryKind::Dir { 0 } else { md.len() },
            mtime,
            mode: mode_of(&md),
            user: uid.and_then(user_name),
            group: gid.and_then(group_name),
            link,
            hidden: name.starts_with('.'),
            path: Some(if root == "/" {
                format!("/{name}")
            } else {
                format!("{root}/{name}")
            }),
        });
    }
    entries.sort_by(|a, b| match (a.is_dir(), b.is_dir()) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(Listing {
        path: root,
        entries,
        source: "local".into(),
        raw: String::new(),
        truncated: false,
    })
}

#[cfg(unix)]
fn is_symlink(p: &Path) -> bool {
    p.is_symlink()
}

#[cfg(not(unix))]
fn is_symlink(_p: &Path) -> bool {
    false
}

pub fn stat(path: &Path) -> FsResult<Entry> {
    let p = local_normalize(&path.to_string_lossy());
    let md = std::fs::metadata(&p).map_err(|e| FsError::NotFound(format!("{} — {e}", p.display())))?;
    let kind = if md.is_dir() {
        EntryKind::Dir
    } else if md.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    let (uid, gid) = ids(&md);
    Ok(Entry {
        name: crate::util::remote_base(&p.to_string_lossy()),
        kind,
        size: md.len(),
        mtime,
        mode: mode_of(&md),
        user: uid.and_then(user_name),
        group: gid.and_then(group_name),
        link: std::fs::read_link(&p).ok().map(|l| l.to_string_lossy().to_string()),
        hidden: p.to_string_lossy().rsplit('/').next().unwrap_or("").starts_with('.'),
        path: Some(p.to_string_lossy().to_string()),
    })
}

pub async fn mkdir(path: &Path, recursive: bool) -> FsResult<()> {
    let p = local_normalize(&path.to_string_lossy());
    if recursive {
        tokio::fs::create_dir_all(&p)
            .await
            .map_err(|e| FsError::Io(format!("{} — {e}", p.display())))
    } else {
        tokio::fs::create_dir(&p)
            .await
            .map_err(|e| FsError::Io(format!("{} — {e}", p.display())))
    }
}

/// Supprime fichier ou dossier (récursif si nécessaire).
pub async fn remove(path: &Path, recursive: bool) -> FsResult<()> {
    let p = local_normalize(&path.to_string_lossy());
    let md = tokio::fs::symlink_metadata(&p)
        .await
        .map_err(|e| FsError::NotFound(format!("{} — {e}", p.display())))?;
    if md.is_dir() {
        if recursive {
            tokio::fs::remove_dir_all(&p)
                .await
                .map_err(|e| FsError::Io(e.to_string()))?;
        } else {
            tokio::fs::remove_dir(&p)
                .await
                .map_err(|e| FsError::Io(e.to_string()))?;
        }
    } else {
        tokio::fs::remove_file(&p)
            .await
            .map_err(|e| FsError::Io(e.to_string()))?;
    }
    Ok(())
}

pub async fn rename(from: &Path, to: &Path) -> FsResult<()> {
    let a = local_normalize(&from.to_string_lossy());
    let b = local_normalize(&to.to_string_lossy());
    tokio::fs::rename(&a, &b)
        .await
        .map_err(|e| FsError::Io(format!("{} → {} : {e}", a.display(), b.display())))
}

pub async fn chmod(path: &Path, mode: u32) -> FsResult<()> {
    let p = local_normalize(&path.to_string_lossy());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode))
            .await
            .map_err(|e| FsError::Io(format!("{} — {e}", p.display())))
    }
    #[cfg(not(unix))]
    {
        let _ = (p, mode);
        Ok(())
    }
}

/// Applique un `mtime` (en secondes epoch) sur un fichier local.
pub fn set_mtime(path: &Path, ts: i64) -> FsResult<()> {
    let p = local_normalize(&path.to_string_lossy());
    let f = std::fs::OpenOptions::new().write(true).open(&p);
    match f {
        Ok(f) => {
            let when = file_time(ts)?;
            f.set_modified(when).map_err(|e| FsError::Io(e.to_string()))
        }
        Err(e) => Err(FsError::Io(e.to_string())),
    }
}

fn file_time(ts: i64) -> FsResult<std::time::SystemTime> {
    let base = std::time::UNIX_EPOCH;
    if ts >= 0 {
        Ok(base + std::time::Duration::from_secs(ts as u64))
    } else {
        Ok(base - std::time::Duration::from_secs((-ts) as u64))
    }
}

/// Touche un fichier : le crée si absent, sinon met à jour l'horodatage.
pub async fn touch(path: &Path) -> FsResult<()> {
    let p = local_normalize(&path.to_string_lossy());
    if p.exists() {
        set_mtime(&p, now_ms() / 1000)
    } else {
        tokio::fs::File::create(&p)
            .await
            .map_err(|e| FsError::Io(format!("{} — {e}", p.display())))?
            .flush()
            .await
            .map_err(|e| e.into())
    }
}

/// Copie locale complète (changement de disque).
pub async fn copy_file(from: &Path, to: &Path) -> FsResult<u64> {
    let a = local_normalize(&from.to_string_lossy());
    let b = local_normalize(&to.to_string_lossy());
    if let Some(p) = b.parent() {
        let _ = tokio::fs::create_dir_all(p).await;
    }
    let mut ri = tokio::fs::File::open(&a).await.map_err(|e| FsError::Io(e.to_string()))?;
    let mut wo = tokio::fs::File::create(&b)
        .await
        .map_err(|e| FsError::Io(format!("{} : {e}", b.display())))?;
    let n = tokio::io::copy(&mut ri, &mut wo)
        .await
        .map_err(|e| FsError::Io(e.to_string()))?;
    Ok(n)
}

/// Recherche dans un arbre local.
pub fn search(
    root: &Path,
    pattern: &str,
    max: usize,
    follow_links: bool,
    files_only: bool,
) -> Vec<Entry> {
    let pat = pattern.to_lowercase();
    let mut out = Vec::new();
    let mut walker = walkdir::WalkDir::new(root)
        .follow_links(follow_links)
        .max_depth(12)
        .into_iter();
    while let Some(Ok(e)) = walker.next() {
        if e.depth() == 0 {
            continue;
        }
        if e.file_type().is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if pat.is_empty() || name.to_lowercase().contains(&pat) {
            let md = e.metadata().ok();
            if files_only && md.as_ref().map(|m| m.is_dir()).unwrap_or(false) {
                continue;
            }
            if !files_only && e.file_type().is_dir() {
                continue;
            }
            let (size, mtime) = match md {
                Some(m) => (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64)),
                None => (0, None),
            };
            let mut en = Entry::file(name, size, mtime);
            en.path = Some(e.path().to_string_lossy().to_string());
            out.push(en);
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Arborescence pour le sélecteur de dossier.
pub fn tree(path: &Path, depth: usize) -> Vec<crate::models::DirNode> {
    fn walk(p: &Path, out: &mut Vec<crate::models::DirNode>, depth: usize) {
        if depth == 0 {
            return;
        }
        let rd = match std::fs::read_dir(p) {
            Ok(r) => r,
            Err(_) => return,
        };
        let mut dirs: Vec<(String, PathBuf)> = Vec::new();
        for e in rd.flatten() {
            if let Ok(ft) = e.file_type() {
                if ft.is_dir() && !e.file_name().to_string_lossy().starts_with('.') {
                    dirs.push((e.file_name().to_string_lossy().to_string(), e.path()));
                }
            }
        }
        dirs.sort();
        for (name, path) in dirs {
            let has = std::fs::read_dir(&path)
                .map(|mut r| r.next().is_some())
                .unwrap_or(false);
            out.push(crate::models::DirNode {
                name: name.clone(),
                path: path.to_string_lossy().to_string(),
                has_children: has,
            });
            if has {
                walk(&path, out, depth - 1);
            }
        }
    }
    let mut out = Vec::new();
    let p = local_normalize(&path.to_string_lossy());
    out.push(crate::models::DirNode {
        name: p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string()),
        path: p.to_string_lossy().to_string(),
        has_children: true,
    });
    walk(&p, &mut out, depth);
    out
}

/// Racines disponibles (Linux : /, les points de montage ; sinon home).
pub fn roots() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(s) = std::fs::read_to_string("/proc/mounts") {
        let mut seen = std::collections::HashSet::new();
        for line in s.lines() {
            if let Some(mnt) = line.split_whitespace().nth(1) {
                if mnt.starts_with('/') && !mnt.contains("proc") && !mnt.contains("sys") && !mnt.contains("dev/pts") {
                    if seen.insert(mnt.to_string()) {
                        v.push(mnt.to_string());
                    }
                }
            }
        }
        v.sort();
    }
    if v.is_empty() {
        v.push("/".into());
    }
    if v.len() > 40 {
        v.truncate(40);
    }
    v
}

/// Espace disque libre, via `df` (pas de dépendance externe supplémentaire).
pub fn disk_info(path: &Path) -> Option<(u64, u64)> {
    let out = std::process::Command::new("df")
        .arg("-Pk")
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().nth(1)?;
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() >= 4 {
        let total = f[1].parse::<u64>().ok()? * 1024;
        let free = f[3].parse::<u64>().ok()? * 1024;
        return Some((total, free));
    }
    None
}

/// Chemin absolu propre pour une saisie utilisateur.
pub fn expand(input: &str) -> String {
    local_normalize(input).to_string_lossy().to_string()
}

/// Le chemin est-il un sous-chemin sûr de `base` ?
pub fn inside(base: &Path, candidate: &Path) -> bool {
    let b = local_normalize(&base.to_string_lossy());
    let c = local_normalize(&candidate.to_string_lossy());
    match c.strip_prefix(&b) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Petit utilitaire pour les chemins relatifs d'affichage.
pub fn relative(base: &Path, p: &Path) -> String {
    match p.strip_prefix(base) {
        Ok(r) => r.to_string_lossy().to_string(),
        Err(_) => p.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_and_ops() {
        let d = std::env::temp_dir().join(format!("zilla-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sd")).unwrap();
        std::fs::write(d.join("f.txt"), b"hello").unwrap();
        let l = list(&d, true, false).unwrap();
        assert_eq!(l.entries.len(), 2);
        assert_eq!(l.entries[0].kind, EntryKind::Dir);
        assert!(search(&d, "f.txt", 10, false, true).iter().any(|e| e.name == "f.txt"));
        assert_eq!(stat(&d.join("f.txt")).unwrap().size, 5);
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&d.join("f.txt"), std::fs::Permissions::from_mode(0o600));
        }
        assert!(tree(&d, 2).len() >= 1);
        let _ = std::fs::remove_dir_all(&d);
    }
}
