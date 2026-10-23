//! Utilitaires transverses : formats façon FileZilla, chemins distants, IDs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Millis depuis l'epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn now_secs() -> i64 {
    now_ms() / 1000
}

/// Identifiant court (pour les jobs, les sites, les ids de panel).
pub fn short_id() -> String {
    static ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    // On évite soigneusement le même identifiant dans la même milliseconde :
    // un compteur et une graine aléatoire entrent dans le calcul.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let base = *SEED.get_or_init(|| {
        let local: u32 = 0;
        let ptr = &local as *const u32 as u64;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        nanos ^ ptr.rotate_left(17) ^ ((std::process::id() as u64) << 12)
    });
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut seed =
        now_ms() as u64 ^ base ^ ((std::process::id() as u64) << 16) ^ n.rotate_left(11);
    let mut s = String::with_capacity(8);
    for _ in 0..8 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        s.push(ALPHABET[(seed >> 33) as usize % ALPHABET.len()] as char);
    }
    s
}

/// Taille lisible, style FileZilla (« Kio/Mio/Gio »).
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["octets", "Kio", "Mio", "Gio", "Tio"];
    if bytes < 1024 {
        return format!("{} octets", bytes);
    }
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let num = if v < 10.0 {
        format!("{:.2}", v)
    } else if v < 100.0 {
        format!("{:.1}", v)
    } else {
        format!("{}", v as u64)
    };
    format!("{} {}", num, UNITS[i])
}

/// Vitesse lisible, en octets/s.
pub fn human_speed(bps: f64) -> String {
    format!("{}/s", human_size(bps.max(0.0) as u64))
}

/// Temps court local (`HH:MM:SS`).
pub fn fmt_time(ts_ms: i64) -> String {
    let total = ts_ms / 1000;
    let secs = total % 60;
    let min = (total / 60) % 60;
    let h = (total / 3600) % 24;
    format!("{:02}:{:02}:{:02}", h, min, secs)
}

/// Date à la « Jan 2 15:04 » (FileZilla), puis année si ≠ année courante.
pub fn fmt_mtime(ts: Option<i64>) -> String {
    let Some(ts) = ts else { return String::new() };
    let days_since_epoch = ts.div_euclid(86400);
    let (y, m, d) = civil_from_days(days_since_epoch);
    let secs_of_day = ts.rem_euclid(86400);
    let hh = secs_of_day / 3600;
    let mm = (secs_of_day % 3600) / 60;
    const MONTHS: [&str; 12] = [
        "Jan", "Fév", "Mar", "Avr", "Mai", "Juin", "Juil", "Août", "Sep", "Oct", "Nov", "Déc",
    ];
    let mon = MONTHS.get(m.saturating_sub(1) as usize % 12).copied().unwrap_or("???");
    if hh == 0 && mm == 0 {
        format!("{} {:>2} {}", mon, d, y)
    } else {
        format!("{} {:>2} {:02}:{:02}", mon, d, hh, mm)
    }
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 4;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y } as i32, m, d)
}

/// Durée écoulée `MM:SS` / `HH:MM:SS`.
pub fn fmt_duration(secs: i64) -> String {
    let secs = secs.max(0);
    if secs >= 3600 {
        format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            (secs % 3600) / 60,
            secs % 60
        )
    } else {
        format!("{}:{}", secs / 60, secs % 60)
    }
}

/// Join propre d'un chemin distant POSIX ou Windows.
pub fn remote_join(base: &str, name: &str) -> String {
    if name.starts_with('/') || name.contains(":\\") || name.starts_with("\\\\") {
        return name.to_string();
    }
    if base == "/" || base.is_empty() {
        format!("/{}", name)
    } else if base.ends_with('/') || base.ends_with('\\') {
        format!("{}{}", base, name)
    } else {
        format!("{}/{}", base, name)
    }
}

/// Répertoire parent d'un chemin distant.
pub fn remote_parent(path: &str) -> String {
    if is_dos_root(path) {
        return path.to_string();
    }
    if is_root(path) {
        return "/".to_string();
    }
    let idx = path
        .rfind(['/', '\\'])
        .map(|i| i as isize)
        .or_else(|| path.find(':').map(|i| i as isize))
        .unwrap_or(0);
    if idx <= 0 {
        "/".to_string()
    } else {
        let mut p = path[..idx as usize].to_string();
        if p.is_empty() {
            p = "/".into();
        }
        if p.ends_with(':') {
            p.push('\\');
        }
        p
    }
}

pub fn is_dos_root(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() == 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

/// Lettre de lecteur si le chemin commence par « C:\ » ou « C:/ ».
pub fn dos_drive(path: &str) -> Option<(String, String)> {
    let b = path.as_bytes();
    if b.len() >= 3 && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') && b[0].is_ascii_alphabetic() {
        return Some((
            (b[0] as char).to_string(),
            path[3..].to_string(),
        ));
    }
    None
}

pub fn is_root(path: &str) -> bool {
    path == "/" || path.is_empty() || is_dos_root(path)
}

/// Éclate un chemin distant en composants (pour le fil d'Ariane).
pub fn remote_split(path: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some((drive, rest)) = dos_drive(path) {
        let root = format!("{}:\\", drive.to_ascii_uppercase());
        out.push((format!("{}:", drive.to_ascii_uppercase()), root.clone()));
        for part in rest.split(['/', '\\']).filter(|s| !s.is_empty()) {
            let prev = out.last().map(|p| p.1.clone()).unwrap_or_else(|| root.clone());
            let next = remote_join(&prev, part);
            out.push((part.to_string(), next));
        }
        return out;
    }
    if path.starts_with("\\\\") {
        // Unc path
        let mut cur = String::new();
        let mut acc = String::new();
        for (i, part) in path[2..].split('\\').enumerate() {
            if i < 2 {
                cur.push('\\');
                cur.push_str(part);
                acc = cur.clone();
            } else {
                let next = format!("{}\\{}", acc, part);
                out.push((part.to_string(), next.clone()));
                acc = next;
            }
        }
        let host = path[2..].split('\\').next().unwrap_or("").to_string();
        out.insert(0, (host, acc));
        return out;
    }
    let mut acc = String::new();
    if path.starts_with('/') {
        out.push(("/".to_string(), "/".to_string()));
        acc = "/".to_string();
    }
    for part in path.split('/').filter(|s| !s.is_empty()) {
        acc = if acc == "/" {
            format!("/{}", part)
        } else {
            format!("{}/{}", acc, part)
        };
        out.push((part.to_string(), acc.clone()));
    }
    if out.is_empty() {
        out.push(("/".to_string(), "/".to_string()));
    }
    out
}

pub fn remote_base(path: &str) -> String {
    let p = path.trim_end_matches(['/', '\\']);
    match p.rfind(['/', '\\']) {
        Some(i) => p[i + 1..].to_string(),
        None => p.to_string(),
    }
}

/// Normalise un chemin local (tilde, chemin relatif).
pub fn local_normalize(path: &str) -> PathBuf {
    let mut p = PathBuf::from(path);
    if path.starts_with("~/") {
        if let Some(home) = home_dir() {
            p = home.join(&path[2..]);
        }
    }
    if !p.is_absolute() {
        p = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(p);
    }
    // Composition sans résolution de symlinks.
    let mut out = PathBuf::new();
    for comp in p.components() {
        use std::path::Component::*;
        match comp {
            Prefix(p) => out.push(p.as_os_str()),
            RootDir => out.push("/"),
            CurDir => {}
            ParentDir => {
                out.pop();
            }
            Normal(c) => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Dossier de configuration par défaut.
pub fn default_config_dir() -> PathBuf {
    if let Ok(x) = std::env::var("ZILLA_CONFIG") {
        return PathBuf::from(x);
    }
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(x).join("zilla-rs");
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("zilla-rs")
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

/// Heure locale `HH:MM:SS` (pour les lignes de journal).
pub fn stamp() -> String {
    let ms = now_ms();
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", (s / 3600) % 24, (s / 60) % 60, s % 60)
}

/// `YYYYMMDDhhmmss` (commandes FTP MDTM/MFMT).
pub fn fmt_yyyy_mm_dd_hhmmss(ts: i64) -> String {
    // la version courte du format MDTM/MFMT (« 20250314081200 ») est directement
    // avalée par chrono — inutile de rejouer l'algorithme des jours civils.
    chrono::DateTime::from_timestamp(ts.max(0), 0)
        .map(|d| d.format("%Y%m%d%H%M%S").to_string())
        .unwrap_or_else(|| "19700101000000".to_string())
}

pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
}

/// Convertit une permission numérique en chaîne « -rwxr-xr-x ».
pub fn mode_to_str(mode: u32, is_dir: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    const R: u32 = 4;
    const W: u32 = 2;
    const X: u32 = 1;
    for shift in [6u32, 3, 0] {
        let b = (mode >> shift) & 7;
        s.push(if b & R != 0 { 'r' } else { '-' });
        s.push(if b & W != 0 { 'w' } else { '-' });
        let x = if b & X != 0 { 'x' } else { '-' };
        s.push(x);
    }
    s
}

/// Convertit une chaîne de droits ou un octal en u32.
pub fn parse_mode(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(m) = u32::from_str_radix(s, 8) {
        return Some(m & 0o7777);
    }
    let t = s.trim_start_matches(['d', 'l', 'c', 'b', 'p', '-']);
    if t.len() == 9 && (t.chars().all(|c| matches!(c, 'r' | 'w' | 'x' | 't' | 's' | '-' | 'S' | 'T')))
    {
        let mut m = 0u32;
        let bits = t.chars().collect::<Vec<_>>();
        let flags = [
            [('r', 4u32), ('w', 2), ('x', 1)],
            [('r', 4u32), ('w', 2), ('x', 1)],
            [('r', 4u32), ('w', 2), ('x', 1)],
        ];
        for (i, group) in flags.iter().enumerate() {
            for (j, (ch, bit)) in group.iter().enumerate() {
                if bits[i * 3 + j] == *ch {
                    m |= bit << (6 - i * 3);
                }
            }
        }
        return Some(m);
    }
    None
}

/// Échappe un nom pour un composant d'URL.
pub fn url_enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// Nom d'affichage d'un fichier distant sans chemin.
pub fn display_name(path: &str) -> String {
    let b = remote_base(path);
    if b.is_empty() {
        path.to_string()
    } else {
        b
    }
}

/// Tente de décoder une ligne venant d'un serveur FTP (latin1 par défaut).
pub fn decode_latin1_lossy(bytes: &[u8]) -> String {
    if bytes.is_ascii() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    bytes
        .iter()
        .map(|b| if *b < 0x80 { *b as char } else { char::from(*b) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 octets");
        assert_eq!(human_size(1024), "1,00 Kio".replace(',', "."));
        assert_eq!(human_size(1024 * 1024 * 3 + 4096), "3,00 Mio".replace(',', "."));
    }

    #[test]
    fn paths() {
        assert_eq!(remote_join("/a/b", "c"), "/a/b/c");
        assert_eq!(remote_join("/", "c"), "/c");
        assert_eq!(remote_parent("/a/b/c"), "/a/b");
        assert_eq!(remote_parent("/a"), "/");
        assert_eq!(remote_parent("C:\\a\\b"), "C:\\a");
        assert_eq!(remote_base("/a/b.txt"), "b.txt");
        assert_eq!(mode_to_str(0o644, false), "-rw-r--r--");
        assert_eq!(parse_mode("755"), Some(0o755));
    }

    #[test]
    fn split() {
        let s = remote_split("/home/user/www");
        assert_eq!(s.len(), 4);
        assert_eq!(s[3].1, "/home/user/www");
        let d = remote_split("C:\\www");
        assert_eq!(d[0].1, "C:\\");
    }
}

#[cfg(test)]
mod idtests {
    use super::short_id;

    #[test]
    fn chemins_relatifs_recales_sous_base() {
        assert_eq!(super::under(Some("/"), "docs/a.txt"), "/docs/a.txt");
        assert_eq!(super::under(Some("/dossier"), "a.txt"), "/dossier/a.txt");
        assert_eq!(super::under(Some("/dossier/"), "/x/a.txt"), "/x/a.txt");
        assert_eq!(super::under(None, "a.txt"), "a.txt");
        assert_eq!(super::under_local(Some("/u"), "docs/b.txt"), "/u/docs/b.txt");
        assert_eq!(super::under_local(Some("/u/"), "/tmp/b.txt"), "/tmp/b.txt");
    }

    #[test]
    fn horodatage_mdtm() {
        // le format envoyé en MDTM/MFMT doit être une date valide : le round-trip
        // est le meilleur contrôle possible.
        use crate::listparse::parse_mlsd_time;
        for ts in [0i64, 1, 100, 1700000000, 1792707353, 4102444799, 2_147_483_647] {
            let s = super::fmt_yyyy_mm_dd_hhmmss(ts);
            assert_eq!(s.len(), 14, "format court : {s}");
            assert_eq!(parse_mlsd_time(&s), Some(ts), "aller-retour raté pour {ts} → {s}");
        }
        assert_eq!(super::fmt_yyyy_mm_dd_hhmmss(-5), "19700101000000");
    }

    #[test]
    fn identifiants_uniques() {
        let mut v = Vec::new();
        for _ in 0..5000 {
            v.push(short_id());
        }
        let mut sorted = v.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), v.len(), "deux identifiants identiques ont été générés");
        assert!(v.iter().all(|s| s.len() == 8));
    }
}

/// cale `p` sous `base` si `p` est relatif (chemin distant)
pub fn under(base: Option<&str>, p: &str) -> String {
    match base {
        Some(b) if !p.starts_with('/') => remote_join(b, p),
        _ => p.to_string(),
    }
}

/// meme chose pour les chemins locaux
pub fn under_local(base: Option<&str>, p: &str) -> String {
    match base {
        Some(b) if !p.starts_with('/') && !(p.as_bytes().len() > 1 && &p[1..2] == ":") => {
            format!("{}/{}", b.trim_end_matches('/'), p)
        }
        _ => p.to_string(),
    }
}
