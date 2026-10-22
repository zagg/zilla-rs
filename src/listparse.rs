//! Interprétation des listings `LIST`/`NLST`/`MLSD` (unix, DOS, faits).

use crate::models::{Entry, EntryKind};
use crate::util::now_secs;

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Traduit un nom de mois (fr ou en, abrégé).
fn month_to_num(m: &str) -> Option<u32> {
    let low = m.to_ascii_lowercase();
    let _table = [
        "jan", "janv", "janvier", "feb", "fev", "fév", "févr", "february", "mars", "mar", "apr",
        "avr", "avril", "may", "mai", "jun", "juin", "jun", "jul", "juil", "jul", "aug", "août",
        "aout", "august", "sep", "sept", "oct", "octobre", "nov", "nov", "dec", "déc", "déc",
    ];
    for (i, m) in MONTHS.iter().enumerate() {
        if low.starts_with(m) {
            return Some(i as u32 + 1);
        }
    }
    let fr = [
        ("jan", 1u32),
        ("janv", 1),
        ("fév", 2),
        ("fev", 2),
        ("fevr", 2),
        ("mars", 3),
        ("mar", 3),
        ("avr", 4),
        ("apr", 4),
        ("mai", 5),
        ("may", 5),
        ("juin", 6),
        ("jun", 6),
        ("juil", 7),
        ("jul", 7),
        ("août", 8),
        ("aout", 8),
        ("aug", 8),
        ("sep", 9),
        ("sept", 9),
        ("oct", 10),
        ("nov", 11),
        ("déc", 12),
        ("dec", 12),
    ];
    for (k, v) in fr {
        if low.starts_with(k) {
            return Some(v);
        }
    }
    None
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn mk_time(y: i64, mo: u32, d: u32, hh: u32, mi: u32) -> i64 {
    days_from_civil(y, mo as i64, d as i64) * 86400 + hh as i64 * 3600 + mi as i64 * 60
}

/// « Jul 12 10:20 » sans année : on prend l'année courante, sinon la précédente.
fn guess_year(mo: u32, d: u32, hh: u32, mi: u32) -> i64 {
    let now = now_secs();
    let approx_year = (now / 31_556_952) + 1970;
    let cand = mk_time(approx_year, mo, d, hh, mi);
    if cand > now + 60 * 86400 {
        mk_time(approx_year - 1, mo, d, hh, mi)
    } else {
        cand
    }
}

struct Field<'a> {
    text: &'a str,
    start: usize,
}

/// Champs d'une ligne, avec offsets (pour retrouver un nom à espaces).
fn fields(line: &str) -> Vec<Field<'_>> {
    // Découpage en caractères : les noms de fichiers peuvent être non-ASCII.
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in line.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push(Field {
                    text: &line[s..i],
                    start: s,
                });
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        out.push(Field {
            text: &line[s..],
            start: s,
        });
    }
    out
}

fn mode_from_rwx(perms: &str) -> u32 {
    // Accepte « -rwxr-xr-x » ou « rwxr-xr-x » (10 ou 9 caractères).
    let raw = perms.as_bytes();
    let b: &[u8] = if raw.len() == 10 {
        &raw[1..]
    } else {
        raw
    };
    let mut m = 0u32;
    for (i, c) in b.iter().enumerate().take(9) {
        let bit: u32 = match c {
            b'r' | b'w' | b'x' | b's' | b't' => 1,
            b'S' | b'T' => {
                if i % 3 == 2 {
                    1
                } else {
                    0
                }
            }
            _ => 0,
        };
        m = (m << 1) | bit;
    }
    if b.len() >= 9 {
        if b[2] == b's' || b[2] == b'S' {
            m |= 0o4000;
        }
        if b[5] == b's' || b[5] == b'S' {
            m |= 0o2000;
        }
        if b[8] == b't' || b[8] == b'T' {
            m |= 0o1000;
        }
    }
    m
}

fn parse_unix(line: &str) -> Option<Entry> {
    let l = line.trim_end_matches('\r');
    let l = l.trim_start();
    if l.is_empty() || l.starts_with("total") || l.starts_with("Usage:") {
        return None;
    }
    let fs = fields(l);
    if fs.len() < 8 {
        return None;
    }
    let perms = fs[0].text;
    if perms.len() < 4 {
        return None;
    }
    let k = perms.as_bytes()[0];
    let kind = match k {
        b'd' => EntryKind::Dir,
        b'l' => EntryKind::Symlink,
        b'-' => EntryKind::File,
        b'c' | b'b' | b'p' | b's' => EntryKind::Other,
        _ => return None,
    };
    if !perms[1..].chars().all(|c| "rwxstST-".contains(c)) {
        return None;
    }
    let size = fs[4].text.parse::<u64>().unwrap_or(0);
    let mo = month_to_num(fs[5].text)?;
    let day: u32 = fs[6].text.trim_end_matches(',').parse().unwrap_or(1);
    let third = fs[7].text.trim_end_matches(',');
    let (hh, mi, year) = if third.contains(':') {
        let mut sp = third.split(':');
        let hh = sp.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        let mi = sp.next().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
        (hh, mi, None)
    } else {
        (0, 0, third.parse::<i64>().ok())
    };
    let mtime = match year {
        Some(y) => mk_time(y, mo, day, hh, mi),
        None => guess_year(mo, day, hh, mi),
    };
    let name_start = fs.get(8)?.start;
    let tail = l[name_start..].to_string();
    let mut name = tail.clone();
    let hidden_name = name.starts_with('.');
    let mut link = None;
    if let Some(pos) = tail.find(" -> ") {
        name = tail[..pos].trim().to_string();
        link = Some(tail[pos + 4..].to_string());
    }
    if name == "." || name == ".." || name.is_empty() {
        return None;
    }
    Some(Entry {
        name,
        kind,
        size: if kind == EntryKind::Dir { 0 } else { size },
        mtime: Some(mtime),
        mode: Some(mode_from_rwx(&perms[1..])),
        user: Some(fs[2].text.to_string()),
        group: Some(fs[3].text.to_string()),
        link,
        hidden: hidden_name,
        path: None,
    })
}

fn parse_dos(line: &str) -> Option<Entry> {
    let l = line.trim_end();
    if !(l.contains('-') && (l.contains("AM") || l.contains("PM") || l.contains(':'))) {
        return None;
    }
    let fs = fields(l);
    if fs.len() < 3 {
        return None;
    }
    let is_dir = l.to_ascii_uppercase().contains("<DIR>");
    // La date peut être devant ou derrière la taille selon les serveurs.
    let date_i = fs
        .iter()
        .position(|f| f.text.matches('-').count() == 2 && f.text.len() <= 10)?;
    if fs[date_i].text.matches('-').count() != 2 {
        return None;
    }
    let date = fs[date_i].text.to_ascii_uppercase();
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let (mo, day, y) = (
        parts[0].trim().parse::<u32>().ok()?,
        parts[1].trim().parse::<u32>().ok()?,
        parts[2].trim().parse::<i64>().ok()?,
    );
    let year = if y < 70 {
        y + 2000
    } else if y < 100 {
        y + 1900
    } else {
        y
    };
    let time_i = date_i + 1;
    let time = fs.get(time_i)?.text.to_ascii_uppercase();
    if !time.contains(':') && !(time.contains("AM") || time.contains("PM")) {
        return None;
    }
    let mut hh = 0u32;
    let mut mi = 0u32;
    let digits: String = time
        .chars()
        .filter(|c| *c != 'A' && *c != 'P' && *c != 'M')
        .collect();
    if digits.contains(':') {
        let mut sp = digits.split(':');
        hh = sp.next()?.parse().unwrap_or(0);
        mi = sp.next()?.parse().unwrap_or(0);
        if time.contains("PM") && hh < 12 {
            hh += 12;
        }
        if time.contains("AM") && hh == 12 {
            hh = 0;
        }
    }
    let mut size = 0u64;
    let mut name_i = time_i + 1;
    if date_i > 0 {
        // « taille date heure nom »
        size = fs[0].text.parse::<u64>().unwrap_or(0);
    }
    // jeton <DIR> éventuel
    if let Some(f) = fs.get(name_i) {
        if f.text.to_ascii_uppercase().contains("<DIR>") {
            name_i += 1;
        }
    }
    if !is_dir {
        if let Some(f) = fs.get(name_i) {
            if let Ok(n) = f.text.parse::<u64>() {
                size = n;
                name_i += 1;
            }
        }
    }
    let name = l.get(fs.get(name_i)?.start..).unwrap_or("").trim().to_string();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    Some(Entry {
        name,
        kind: if is_dir {
            EntryKind::Dir
        } else {
            EntryKind::File
        },
        size,
        mtime: Some(mk_time(year, mo, day, hh, mi)),
        mode: Some(if is_dir { 0o755 } else { 0o644 }),
        user: None,
        group: None,
        link: None,
        hidden: false,
        path: None,
    })
}

/// Temps des faits MLSD : `YYYYMMDDhhmmss[.sss]` ou `seconds:123`.
pub fn parse_mlsd_time(v: &str) -> Option<i64> {
    if let Some(s) = v.strip_prefix("seconds:") {
        return s.parse::<i64>().ok();
    }
    if v.len() < 14 {
        return None;
    }
    let b = v.as_bytes();
    if !b[..14].iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let y: i64 = v[0..4].parse().ok()?;
    let mo: u32 = v[4..6].parse().ok()?;
    let d: u32 = v[6..8].parse().ok()?;
    let hh: u32 = v[8..10].parse().ok()?;
    let mi: u32 = v[10..12].parse().ok()?;
    let ss: i64 = v[12..14].parse().ok()?;
    Some(days_from_civil(y, mo as i64, d as i64) * 86400 + hh as i64 * 3600 + mi as i64 * 60 + ss)
}

/// Lit une ligne `type=..;size=..;nom`.
pub fn parse_mlsd(line: &str) -> Option<Entry> {
    let line = line.trim_end();
    let cut = line.rfind(';')?;
    let facts = &line[..cut];
    let name = line[cut + 1..].trim().to_string();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let mut kind = EntryKind::File;
    let mut size = 0u64;
    let mut mtime = None;
    let mut mode = None;
    let mut user = None;
    let mut group = None;
    let mut link = None;
    for fact in facts.split(';') {
        if fact.is_empty() {
            continue;
        }
        let mut kv = fact.splitn(2, '=');
        let k = kv.next()?.trim().to_ascii_lowercase();
        let v = kv.next().unwrap_or("").trim();
        match k.as_str() {
            "type" => {
                kind = match v.to_ascii_lowercase().as_str() {
                    "dir" | "cdir" | "pdir" => EntryKind::Dir,
                    "symlink" | "osymlink.link" | "osymlink.target" => EntryKind::Symlink,
                    _ => EntryKind::File,
                }
            }
            "size" => size = v.parse().unwrap_or(0),
            "modify" | "create" => mtime = parse_mlsd_time(v),
            "unix.mode" | "mode" => mode = u32::from_str_radix(v, 8).ok(),
            "unix.owner" => user = Some(v.to_string()),
            "unix.group" => group = Some(v.to_string()),
            "unix.uid" => user = user.or_else(|| Some(v.to_string())),
            "unix.gid" => group = group.or_else(|| Some(v.to_string())),
            "target" | "unix.target" => {
                link = Some(v.to_string());
                kind = EntryKind::Symlink;
            }
            _ => {}
        }
    }
    let hidden = name.starts_with('.');
    Some(Entry {
        name,
        kind,
        size,
        mtime,
        mode,
        user,
        group,
        link,
        hidden,
        path: None,
    })
}

fn looks_unix(line: &str) -> bool {
    let f = fields(line);
    f.len() >= 8
        && f[0].text.len() >= 9
        && "dlcps-?".contains(f[0].text.as_bytes()[0] as char)
        && f[0].text[1..].chars().all(|c| "rwxstST-".contains(c))
}

fn looks_dos(line: &str) -> bool {
    let f = fields(line);
    let up = line.to_ascii_uppercase();
    if up.contains("<DIR>") {
        return f.len() >= 3 && f[0].text.contains('-');
    }
    f.len() >= 4 && f[0].text.parse::<u64>().is_ok() && f[1].text.contains('-')
}

fn looks_mlsd(line: &str) -> bool {
    line.contains(";type=")
        || line.contains("type=file;")
        || line.contains("type=dir;")
        || (line.contains(';') && line.contains('=') && line.starts_with("type="))
}

/// Interprète une sortie de listing complète.
pub fn parse_listing(raw: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        let t = line.trim();
        if t.is_empty()
            || t.starts_with("total ")
            || t.starts_with("Usage:")
            || t.starts_with("Trying ")
            || t.eq_ignore_ascii_case("listing:")
        {
            continue;
        }
        let mut e = None;
        if looks_mlsd(line) {
            e = parse_mlsd(line);
        }
        if e.is_none() && looks_unix(line) {
            e = parse_unix(line);
        }
        if e.is_none() && looks_dos(line) {
            e = parse_dos(line);
        }
        if e.is_none() {
            let name = line.trim().to_string();
            if name == "." || name == ".." || name.contains("Permission denied") {
                continue;
            }
            e = Some(Entry {
                name,
                kind: EntryKind::File,
                size: 0,
                mtime: None,
                mode: None,
                user: None,
                group: None,
                link: None,
                hidden: false,
                path: None,
            });
        }
        let e = e.unwrap();
        if !out.iter().any(|x: &Entry| x.name == e.name) {
            out.push(e);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_lines() {
        let e = parse_unix("-rw-r--r-- 1 user group 1234 Jan  1  2020 report.pdf").unwrap();
        assert_eq!(e.name, "report.pdf");
        assert_eq!(e.size, 1234);
        assert_eq!(e.perm_str(), "-rw-r--r--");
        assert_eq!(e.mtime, Some(mk_time(2020, 1, 1, 0, 0)));

        let d = parse_unix("drwxr-xr-x  2 davy  staff       64 Jul 12 10:20 www").unwrap();
        assert_eq!(d.kind, EntryKind::Dir);
        assert_eq!(d.name, "www");

        let l = parse_unix("lrwxrwxrwx 1 root root 11 Apr  3  2023 link -> /etc/passwd").unwrap();
        assert_eq!(l.kind, EntryKind::Symlink);
        assert_eq!(l.link.as_deref(), Some("/etc/passwd"));
        assert_eq!(l.name, "link");

        // Nom avec espaces.
        let s = parse_unix("-rw-r--r-- 1 u g 5 Jan  1  2021 my file.txt").unwrap();
        assert_eq!(s.name, "my file.txt");

        // Totaux et poubelles ignorés.
        assert!(parse_unix("total 8").is_none());
    }

    #[test]
    fn dos_lines() {
        let e = parse_dos("01-01-20  10:20AM   <DIR>          Images").unwrap();
        assert_eq!(e.name, "Images");
        assert!(e.is_dir());
        let f = parse_dos("12345  06-15-19  11:11AM readme.txt").unwrap();
        assert_eq!(f.size, 12345);
        assert_eq!(f.name, "readme.txt");
    }

    #[test]
    fn mlsd_lines() {
        let e = parse_mlsd("type=file;size=1024;modify=20210314081200; notes.txt").unwrap();
        assert_eq!(e.size, 1024);
        assert_eq!(e.name, "notes.txt");
        assert_eq!(e.mtime, Some(parse_mlsd_time("20210314081200").unwrap()));
        let d = parse_mlsd("type=dir;modify=20210301120000;upload").unwrap();
        assert!(d.is_dir());
        let u = parse_mlsd("type=file;size=1;unix.mode=0644;unix.owner=root; x").unwrap();
        assert_eq!(u.mode, Some(0o644));
        assert_eq!(u.user.as_deref(), Some("root"));
    }

    #[test]
    fn mixed() {
        let raw = "total 12\ndrwxr-xr-x 3 u g 4096 Jan  1  2020 a\n-rw-rw-r-- 1 u g 10 Feb  2 10:00 b.c\ntype=file;size=3;x\n";
        let v = parse_listing(raw);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].name, "a");
        assert_eq!(v[2].size, 3);
    }
}

#[cfg(test)]
mod parse_extra_tests {
    use super::*;

    #[test]
    fn mode_simple() {
        assert_eq!(mode_from_rwx("rw-r--r--"), 0o644);
        assert_eq!(mode_from_rwx("-rwxr-xr-x"), 0o755);
        assert_eq!(mode_from_rwx("drwxrwxrwx"), 0o777);
        assert_eq!(mode_from_rwx("rwxrwxrwt"), 0o1777);
        assert_eq!(mode_from_rwx("rw-rw-rw-"), 0o666);
        assert_eq!(mode_from_rwx("----------"), 0);
    }

    #[test]
    fn unix_line_sans_espace() {
        let e = parse_unix("-rw-r--r--    1 zilla    zilla       1978740 Jan 02  2024 report.pdf").unwrap();
        assert_eq!(e.name, "report.pdf");
        assert_eq!(e.size, 1978740);
        assert_eq!(e.mode, Some(0o644));
    }

    #[test]
    fn unix_line_avec_espace_et_accent() {
        let e = parse_unix("drwxrwxr-x   2 zilla    zilla        4096 Oct 22 21:48 Le diner - Freida McFadden").unwrap();
        assert_eq!(e.name, "Le diner - Freida McFadden");
        assert!(e.is_dir());
    }

    #[test]
    fn lien_symbolique() {
        let e = parse_unix("lrwxrwxrwx   1 zilla    zilla          11 Aug  1 10:00 latest -> report.pdf").unwrap();
        assert_eq!(e.kind, EntryKind::Symlink);
        assert_eq!(e.link.as_deref(), Some("report.pdf"));
        assert_eq!(e.name, "latest");
    }

    #[test]
    fn dos_line() {
        let e = parse_dos("10-22-25  10:11PM          227297 Le diner - Freida McFadden.epub").unwrap();
        assert_eq!(e.name, "Le diner - Freida McFadden.epub");
        assert_eq!(e.size, 227297);
    }
}
