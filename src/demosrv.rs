//! Mini-serveur FTP embarqué : sert une arborescence locale sur 127.0.0.1.
//! Sert à l'auto-test, au banc d'essai et à la démo hors-ligne.

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Serveur FTP de démonstration (localhost uniquement).
pub struct DemoFtp {
    pub addr: SocketAddr,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl DemoFtp {
    pub async fn start(root: &Path) -> std::io::Result<DemoFtp> {
        DemoFtp::start_on(root, 2121).await
    }

    /// Essaie `port`, sinon un port libre.
    pub async fn start_on(root: &Path, port: u16) -> std::io::Result<DemoFtp> {
        std::fs::create_dir_all(root)?;
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(_) => TcpListener::bind(("127.0.0.1", 0)).await?,
        };
        let addr = listener.local_addr()?;
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let root = root.to_path_buf();
        tokio::spawn(async move {
            let root = root;
            loop {
                tokio::select! {
                    acc = listener.accept() => {
                        if let Ok((sock, _peer)) = acc {
                            let root = root.clone();
                            tokio::spawn(async move {
                                if let Err(_e) = session(sock, root).await {}
                            });
                        }
                    }
                    _ = &mut rx => break,
                }
            }
        });
        Ok(DemoFtp {
            addr,
            shutdown: Some(tx),
        })
    }

    pub fn host(&self) -> String {
        self.addr.ip().to_string()
    }
    pub fn port(&self) -> u16 {
        self.addr.port()
    }
    pub fn url(&self) -> String {
        format!("ftp://{}:{}", self.addr.ip(), self.addr.port())
    }

    pub fn stop(mut self) {
        if let Some(s) = self.shutdown.take() {
            let _ = s.send(());
        }
    }
}

async fn session(sock: TcpStream, root: PathBuf) -> std::io::Result<()> {
    sock.set_nodelay(true)?;
    let (mut r, mut w) = sock.into_split();
    let mut buf: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    let mut cwd = "/".to_string();
    let mut rest: u64 = 0;
    let mut passive: Option<TcpListener> = None;
    let mut active: Option<SocketAddr> = None;
    let mut ascii = false;
    let mut rename_from: Option<String> = None;

    w.write_all(b"220 (Zilla-RS demo FTP) Ready.\r\n").await?;

    loop {
        let line = match read_line(&mut r, &mut buf, &mut pos).await {
            Ok(l) => l,
            Err(_) => break,
        };
        let (cmd, arg) = split2(&line);
        let cu = cmd.to_ascii_uppercase();
        match cu.as_str() {
            "USER" => {
                if arg.eq_ignore_ascii_case("anonymous") || arg.is_empty() {
                    w.write_all(b"230 Anonymous user logged in.\r\n").await?;
                } else {
                    w.write_all(b"331 Password required.\r\n").await?;
                }
            }
            "PASS" => {
                w.write_all(b"230 Login successful.\r\n").await?;
            }
            "ACCT" => {
                w.write_all(b"230 OK.\r\n").await?;
            }
            "SYST" => {
                w.write_all(b"215 UNIX Type: L8\r\n").await?;
            }
            "FEAT" => {
                let body = "211-Features:\r\n MDTM\r\n SIZE\r\n UTF8\r\n TVFS\r\n REST STREAM\r\n EPSV\r\n MFMT\r\n211 End\r\n";
                w.write_all(body.as_bytes()).await?;
            }
            "NOOP" => {
                w.write_all(b"250 OK\r\n").await?;
            }
            "PWD" => {
                let s = format!("257\"{cwd}\" is the current directory\r\n");
                w.write_all(s.as_bytes()).await?;
            }
            "TYPE" => {
                ascii = arg.to_uppercase().starts_with('A');
                w.write_all(b"200 Type set.\r\n").await?;
            }
            "STRU" | "MODE" => {
                w.write_all(b"200 OK.\r\n").await?;
            }
            "OPTS" => {
                w.write_all(b"200 OK.\r\n").await?;
            }
            "CWD" | "CDUP" => {
                let target = if cu == "CDUP" { "..".to_string() } else { arg.clone() };
                match chdir(&cwd, &target) {
                    Some(p) if is_dir_real(&root, &p) => {
                        cwd = p;
                        w.write_all(b"250 Directory changed.\r\n").await?;
                    }
                    _ => {
                        w.write_all(b"550 No such directory.\r\n").await?;
                    }
                }
            }
            "MKD" => {
                let p = resolve(&cwd, &arg);
                let real = real_path(&root, &p);
                match std::fs::create_dir(&real) {
                    Ok(()) => {
                        let s = format!("257\"{p}\" created\r\n");
                        w.write_all(s.as_bytes()).await?;
                    }
                    Err(e) => {
                        let s = format!("550 {e}\r\n");
                        w.write_all(s.as_bytes()).await?;
                    }
                }
            }
            "RMD" => {
                let p = resolve(&cwd, &arg);
                match std::fs::remove_dir(real_path(&root, &p)) {
                    Ok(()) => w.write_all(b"250 Directory removed.\r\n").await?,
                    Err(e) => {
                        let s = format!("550 {e}\r\n");
                        w.write_all(s.as_bytes()).await?;
                    }
                }
            }
            "DELE" => {
                let p = resolve(&cwd, &arg);
                match std::fs::remove_file(real_path(&root, &p)) {
                    Ok(()) => w.write_all(b"250 File deleted.\r\n").await?,
                    Err(e) => {
                        let s = format!("550 {e}\r\n");
                        w.write_all(s.as_bytes()).await?;
                    }
                }
            }
            "RNFR" => {
                rename_from = Some(resolve(&cwd, &arg));
                w.write_all(b"350 Ready for RNTO.\r\n").await?;
            }
            "RNTO" => {
                if let Some(from) = rename_from.take() {
                    let to = resolve(&cwd, &arg);
                    match std::fs::rename(real_path(&root, &from), real_path(&root, &to)) {
                        Ok(()) => w.write_all(b"250 Rename OK.\r\n").await?,
                        Err(e) => {
                            let s = format!("550 {e}\r\n");
                            w.write_all(s.as_bytes()).await?;
                        }
                    }
                } else {
                    w.write_all(b"503 Rename sequence error.\r\n").await?;
                }
            }
            "SIZE" => {
                let p = resolve(&cwd, &arg);
                let real = real_path(&root, &p);
                match tokio::fs::metadata(&real).await {
                    Ok(m) if m.is_file() => {
                        let s = format!("213 {}\r\n", m.len());
                        w.write_all(s.as_bytes()).await?;
                    }
                    _ => w.write_all(b"550 Not a regular file.\r\n").await?,
                }
            }
            "MDTM" => {
                let p = resolve(&cwd, &arg);
                let real = real_path(&root, &p);
                match tokio::fs::metadata(&real).await {
                    Ok(m) => {
                        let ts = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        w.write_all(format!("213 {}\r\n", mlsd_time(ts)).as_bytes()).await?;
                    }
                    Err(_) => w.write_all(b"550 unavailable.\r\n").await?,
                }
            }
            "MFMT" => {
                // « MFMT 20210314081200 fichier » : règlement de la date du fichier.
                let (tsp, path) = split2(&arg);
                let ts = chrono::NaiveDateTime::parse_from_str(tsp.trim(), "%Y%m%d%H%M%S")
                    .map(|d| d.and_utc().timestamp())
                    .unwrap_or(0);
                let p = resolve(&cwd, &path);
                let real = real_path(&root, &p);
                match std::fs::OpenOptions::new().write(true).open(&real) {
                    Ok(f) if ts > 0 => {
                        let mut ft = std::fs::FileTimes::new();
                        ft = ft.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64));
                        match f.set_times(ft) {
                            Ok(()) => {
                                w.write_all(format!("250 Modify OK; mtime={}\r\n", mlsd_time(ts)).as_bytes())
                                    .await?
                            }
                            Err(e) => w.write_all(format!("550 {e}\r\n").as_bytes()).await?,
                        }
                    }
                    _ => w.write_all(b"550 cannot set time.\r\n").await?,
                }
            }
            "SITE" => {
                w.write_all(b"200 Site ok.\r\n").await?;
            }
            "PASV" | "EPSV" => {
                let l = TcpListener::bind("127.0.0.1:0").await?;
                let port = l.local_addr()?.port();
                passive = Some(l);
                active = None;
                if cu == "EPSV" {
                    w.write_all(format!("229 Entering Extended Passive Mode (|||{port}|)\r\n").as_bytes())
                        .await?;
                } else {
                    w.write_all(
                        format!(
                            "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                            port >> 8,
                            port & 0xff
                        )
                        .as_bytes(),
                    )
                    .await?;
                }
            }
            "PORT" | "EPRT" => {
                let a = parse_port_cmd(&arg);
                match a {
                    Some(a) => {
                        active = Some(a);
                        passive = None;
                        w.write_all(b"200 PORT command successful.\r\n").await?;
                    }
                    None => w.write_all(b"501 Bad address.\r\n").await?,
                }
            }
            "LIST" | "NLST" => {
                let path = if arg.trim().is_empty() {
                    cwd.clone()
                } else {
                    resolve(&cwd, arg.trim())
                };
                if let Some(data) = open_data(&mut r, &mut w, passive.take(), active).await? {
                    w.write_all(b"150 Here comes the directory listing.\r\n").await?;
                    let _ = list_into(&root, &path, data, cu == "NLST").await;
                    w.write_all(b"226 Directory send OK.\r\n").await?;
                }
            }
            "RETR" => {
                let p = resolve(&cwd, arg.trim());
                let real = real_path(&root, &p);
                if !real.is_file() {
                    w.write_all(b"550 File not found.\r\n").await?;
                } else if let Some(mut data) = open_data(&mut r, &mut w, passive.take(), active).await? {
                    w.write_all(b"150 Opening data connection.\r\n").await?;
                    let mut f = tokio::fs::File::open(&real).await?;
                    if rest > 0 {
                        use tokio::io::AsyncSeekExt;
                        let _ = f.seek(std::io::SeekFrom::Start(rest)).await;
                    }
                    rest = 0;
                    let mut tmp = vec![0u8; 32 * 1024];
                    loop {
                        let n = f.read(&mut tmp).await?;
                        if n == 0 {
                            break;
                        }
                        if data.write_all(&tmp[..n]).await.is_err() {
                            break;
                        }
                    }
                    let _ = data.shutdown().await;
                    drop(data);
                    w.write_all(b"226 Transfer complete.\r\n").await?;
                }
            }
            "STOR" | "APPE" => {
                let p = resolve(&cwd, arg.trim());
                let real = real_path(&root, &p);
                if let Some(parent) = real.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Some(mut data) = open_data(&mut r, &mut w, passive.take(), active).await? {
                    w.write_all(b"150 Ready to receive.\r\n").await?;
                    rest = 0;
                    let mut f = if cu == "APPE" {
                        tokio::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(&real)
                            .await?
                    } else {
                        tokio::fs::File::create(&real).await?
                    };
                    let mut tmp = vec![0u8; 32 * 1024];
                    loop {
                        match data.read(&mut tmp).await {
                            Ok(0) => break,
                            Ok(n) => {
                                f.write_all(&tmp[..n]).await?;
                            }
                            Err(_) => break,
                        }
                    }
                    f.flush().await?;
                    drop(f);
                    drop(data);
                    w.write_all(b"226 Transfer complete.\r\n").await?;
                }
            }
            "REST" => {
                rest = arg.trim().parse::<u64>().unwrap_or(0);
                w.write_all(b"350 Ready to restart.\r\n").await?;
            }
            "ABOR" => {
                w.write_all(b"226 ABOR ok.\r\n").await?;
            }
            "QUIT" => {
                w.write_all(b"221 Goodbye.\r\n").await?;
                break;
            }
            "HELP" | "CLNT" | "LANG" | "HOST" | "AUTH" | "PROT" | "PBSZ" => {
                w.write_all(b"200 OK.\r\n").await?;
            }
            "" => {}
            other => {
                let s = format!("502 Unknown command {other}.\r\n");
                w.write_all(s.as_bytes()).await?;
            }
        }
        let _ = ascii;
    }
    Ok(())
}

async fn read_line(
    r: &mut tokio::net::tcp::OwnedReadHalf,
    buf: &mut Vec<u8>,
    pos: &mut usize,
) -> std::io::Result<String> {
    loop {
        if let Some(i) = buf[*pos..].iter().position(|b| *b == b'\n') {
            let end = *pos + i;
            let mut s = String::from_utf8_lossy(&buf[*pos..end]).into_owned();
            if s.ends_with('\r') {
                s.pop();
            }
            *pos = end + 1;
            if *pos > 64 * 1024 {
                buf.drain(0..*pos);
                *pos = 0;
            }
            return Ok(s);
        }
        let mut tmp = [0u8; 1024];
        let n = r.read(&mut tmp).await?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof"));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

async fn open_data(
    _r: &mut tokio::net::tcp::OwnedReadHalf,
    _w: &mut tokio::net::tcp::OwnedWriteHalf,
    passive: Option<TcpListener>,
    active: Option<SocketAddr>,
) -> std::io::Result<Option<TcpStream>> {
    if let Some(l) = passive {
        let (s, _) = tokio::time::timeout(std::time::Duration::from_secs(15), l.accept())
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "no passive conn"))??;
        s.set_nodelay(true)?;
        Ok(Some(s))
    } else if let Some(a) = active {
        let s = tokio::time::timeout(std::time::Duration::from_secs(15), TcpStream::connect(a))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "no active conn"))??;
        s.set_nodelay(true)?;
        Ok(Some(s))
    } else {
        _w.write_all(b"425 No data connection.\r\n").await?;
        Ok(None)
    }
}

async fn list_into(root: &Path, vdir: &str, mut data: TcpStream, names_only: bool) -> std::io::Result<()> {
    let real = real_path(root, vdir);
    let mut lines = String::new();
    let rd = std::fs::read_dir(&real);
    match rd {
        Ok(rd) => {
            let mut items: Vec<(String, std::fs::DirEntry)> = Vec::new();
            for e in rd.flatten() {
                items.push((e.file_name().to_string_lossy().to_string(), e));
            }
            items.sort_by(|a, b| b.0.cmp(&a.0));
            items.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, de) in items {
                if names_only {
                    lines.push_str(&format!("{name}\r\n"));
                    continue;
                }
                let md = match std::fs::metadata(de.path()) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                let is_dir = md.is_dir();
                let mode = if is_dir { 0o040755 } else { 0o100644 };
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                lines.push_str(&format!(
                    "{perm} {links:>3} {user:<8} {group:<8} {size:>12} {stamp} {name}\r\n",
                    perm = rwx(mode, is_dir),
                    links = if is_dir { 2 } else { 1 },
                    user = "zilla",
                    group = "zilla",
                    size = md.len(),
                    stamp = ls_time(mtime),
                    name = name
                ));
            }
        }
        Err(e) => {
            let _ = e;
            lines.push_str("550 cannot open directory\r\n");
        }
    }
    data.write_all(lines.as_bytes()).await?;
    data.flush().await?;
    let _ = data.shutdown().await;
    Ok(())
}

fn rwx(mode: u32, dir: bool) -> String {
    let mut s = String::new();
    s.push(if dir { 'd' } else { '-' });
    let table = ['r', 'w', 'x'];
    for g in 0..3 {
        for (i, t) in table.iter().enumerate() {
            s.push(if mode & (1 << (8 - g * 3 - i)) != 0 { *t } else { '-' });
        }
    }
    s
}

fn ls_time(ts: i64) -> String {
    // format façon « ls -l » : « Oct 22 14:03 » ou « Oct 22  2024 » selon l'ancienneté.
    let names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let dt = chrono::DateTime::from_timestamp(ts, 0);
    let (mon, d, y) = match dt {
        Some(d) => (
            names[d.format("%m").to_string().parse::<usize>().unwrap_or(1) - 1],
            d.format("%d").to_string().parse::<u32>().unwrap_or(1),
            d.format("%Y").to_string().parse::<i32>().unwrap_or(1970),
        ),
        None => return "Jan  1  1970".to_string(),
    };
    let hh = ts.rem_euclid(86400) / 3600;
    let mi = ts.rem_euclid(3600) / 60;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|x| x.as_secs() as i64).unwrap_or(0);
    if now - ts > 365 * 86400 {
        format!("{mon} {d:>2} {y}")
    } else {
        format!("{mon} {d:>2} {hh:02}:{mi:02}")
    }
}

fn mlsd_time(ts: i64) -> String {
    // même format court que MDTM/MFMT ; chrono s'en charge sans算法 maison.
    chrono::DateTime::from_timestamp(ts.max(0), 0)
        .map(|d| d.format("%Y%m%d%H%M%S").to_string())
        .unwrap_or_else(|| "19700101000000".to_string())
}

fn split2(line: &str) -> (String, String) {
    let mut it = line.splitn(2, ' ');
    let c = it.next().unwrap_or("").to_string();
    let a = it.next().unwrap_or("").to_string();
    (c, a)
}

fn parse_port_cmd(arg: &str) -> Option<SocketAddr> {
    if arg.starts_with('|') {
        let mut p = arg.trim_matches('|').split('|');
        let _proto = p.next()?;
        let ip = p.next()?.parse::<std::net::IpAddr>().ok()?;
        let port = p.next()?.parse::<u16>().ok()?;
        return Some(SocketAddr::new(ip, port));
    }
    let f: Vec<u16> = arg.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    if f.len() < 6 {
        return None;
    }
    let ip = format!("{}.{}.{}.{}", f[0], f[1], f[2], f[3]);
    let ip: std::net::IpAddr = ip.parse().ok()?;
    Some(SocketAddr::new(ip, f[4] * 256 + f[5]))
}

fn chdir(current: &str, target: &str) -> Option<String> {
    if target == ".." {
        if current == "/" {
            return Some("/".to_string());
        }
        let p = Path::new(current);
        return Some(
            p.parent()
                .map(|x| if x.as_os_str().is_empty() { "/".to_string() } else { x.to_string_lossy().to_string() })
                .unwrap_or_else(|| "/".to_string()),
        );
    }
    Some(resolve(current, target))
}

fn resolve(current: &str, target: &str) -> String {
    if target.is_empty() {
        return current.to_string();
    }
    let mut p = if target.starts_with('/') {
        PathBuf::from(target)
    } else {
        Path::new(current).join(target)
    };
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::RootDir => {
                out.push("/");
            }
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(x) => out.push(x),
            _ => {}
        }
    }
    p = out;
    let s = p.to_string_lossy().to_string();
    if s.is_empty() {
        "/".to_string()
    } else {
        s
    }
}

fn real_path(root: &Path, vpath: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for c in Path::new(vpath).components() {
        if let Component::Normal(x) = c {
            p.push(x);
        }
    }
    p
}

fn is_dir_real(root: &Path, vpath: &str) -> bool {
    real_path(root, vpath).is_dir()
}
