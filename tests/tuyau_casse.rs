//! Un envoi qu'on annule ( ou qu'on casse ) en plein vol ne doit rien laisser
//! en travers de la gorge au fil de contrôle.
//!
//! Le détail qui compte : un serveur FTP rend toujours sa réponse de fin de
//! transfert *après* avoir refermé le canal de données. Si personne ne lit
//! cette réponse — et c'est le cas quand la ligne part à la poubelle ou tombe
//! en erreur avant `finish()` — elle reste en attente dans le fil de contrôle.
//! La commande d'après lit ce reliquat au lieu de sa propre réponse : le
//! listage suivant est pourri, et le reste tant qu'on n'a pas jeté la
//! connexion.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use zilla_rs::models::{Direction, JobState, TransferJob};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Regime {
    /// le canal de données est refermé en plein envoi : le client verra « broken pipe »
    Coupe,
    /// le serveur n'encaisse rien : l'écriture s'enlise, puis la réponse de fin
    /// de transfert arrive dès que le canal est refermé par le client
    Enlissee,
}

struct Maison {
    port: u16,
    racine: PathBuf,
}

async fn une_maison(regime: Regime, lachette: usize) -> Maison {
    let racine = std::env::temp_dir().join(format!("zilla-tuyau-{:?}", regime).to_lowercase());
    let _ = std::fs::remove_dir_all(&racine);
    std::fs::create_dir_all(racine.join("coin")).unwrap();
    std::fs::write(racine.join("coin").join("veinard.txt"), "coucou".as_bytes()).unwrap();
    std::fs::write(racine.join("lu.txt"), "rien à signaler".as_bytes()).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let graine = racine.clone();
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            let root = graine.clone();
            tokio::spawn(async move {
                let _ = session(sock, root, regime, lachette).await;
            });
        }
    });
    Maison { port, racine }
}

async fn poste(regime: Regime) -> (Arc<zilla_rs::App>, String, PathBuf) {
    let maison = une_maison(regime, 64 * 1024).await;
    let reglages = std::env::temp_dir().join(format!("zilla-tuyau-cfg-{:?}", regime).to_lowercase());
    let _ = std::fs::remove_dir_all(&reglages);
    let app = zilla_rs::App::new(reglages, "test").expect("app");
    app.hub.set_verbose(true);
    let (site, _) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(maison.port), "ftp")
        .await
        .expect("connexion");
    {
        let mut s = app.store.write().unwrap();
        s.settings.transfers.max_parallel = 1;
        s.settings.transfers.retries = 0;
        s.settings.net.data_timeout = 3;
    }
    (app, site, maison.racine)
}

// -----------------------------------------------------------------------------
// le serveur
// -----------------------------------------------------------------------------

async fn session(sock: TcpStream, root: PathBuf, regime: Regime, lachette: usize) -> std::io::Result<()> {
    let _ = sock.set_nodelay(true);
    let (r, mut w) = sock.into_split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (txl, mut rxl) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    tokio::spawn(lire(r, txl));

    let mut buf = String::new();
    let mut cwd = "/".to_string();
    let mut passive: Option<TcpListener> = None;

    w.write_all(b"220 Saboteur aux ordres.\r\n").await?;
    loop {
        // la suite : soit une ligne du client, soit une réponse de fin de
        // transfert qui lui doit déjà — un serveur réel la pousse dès qu'il a
        // refermé le canal, sans attendre la commande suivante
        let ligne = tokio::select! {
            venu = rxl.recv() => {
                match venu {
                    Some(b) => {
                        buf.push_str(&String::from_utf8_lossy(&b));
                        match dequeue(&mut buf) {
                            Some(l) => l,
                            None => continue,
                        }
                    }
                    None => return Ok(()),
                }
            }
            Some(avant) = rx.recv() => {
                let _ = w.write_all(avant.as_bytes()).await;
                continue;
            }
        };
        let (c, arg) = split2(&ligne);
        let u = c.to_ascii_uppercase();
        match u.as_str() {
            "USER" | "PASS" | "ACCT" => w.write_all(b"230 Qui que tu sois, bonjour.\r\n").await?,
            "SYST" => w.write_all(b"215 UNIX Type: L8\r\n").await?,
            "FEAT" => {
                w.write_all(
                    b"211-Features:\r\n MDTM\r\n SIZE\r\n UTF8\r\n TVFS\r\n REST STREAM\r\n MLST type*;size*;modify*;\r\n211 End\r\n",
                )
                .await?
            }
            "NOOP" | "TYPE" | "STRU" | "MODE" | "OPTS" => w.write_all(b"200 OK.\r\n").await?,
            "PWD" => {
                let s = format!("257\"{cwd}\" est le courant.\r\n");
                w.write_all(s.as_bytes()).await?
            }
            "CWD" | "CDUP" => {
                let cible = if u == "CDUP" { "..".into() } else { arg.clone() };
                let p = resolve(&cwd, &cible);
                if reelle(&root, &p).is_dir() {
                    cwd = p;
                    w.write_all("250 Changé.\r\n".as_bytes()).await?
                } else {
                    w.write_all("550 Par là.\r\n".as_bytes()).await?
                }
            }
            "QUIT" => {
                let _ = w.write_all("221 À la revoyure.\r\n".as_bytes()).await;
                return Ok(());
            }
            "REST" => {
                let _ = arg.trim().parse::<u64>().unwrap_or(0);
                w.write_all(b"350 Reprise en vue.\r\n").await?
            }
            "SIZE" => {
                let p = reelle(&root, &resolve(&cwd, arg.trim()));
                match std::fs::metadata(&p) {
                    Ok(m) => {
                        let s = format!("213 {}\r\n", m.len());
                        w.write_all(s.as_bytes()).await?
                    }
                    Err(_) => w.write_all(b"550 introuvable.\r\n").await?,
                }
            }
            "MLST" => {
                let p = resolve(&cwd, arg.trim());
                match std::fs::metadata(reelle(&root, &p)) {
                    Ok(m) => {
                        let faits = if m.is_dir() {
                            "type=dir".to_string()
                        } else {
                            format!("type=file;size={}", m.len())
                        };
                        let s = format!(
                            "250-Listing {p}\r\n {};modify={};\r\n250 End\r\n",
                            faits,
                            mlsd_time(horodatage(&reelle(&root, &p)))
                        );
                        w.write_all(s.as_bytes()).await?
                    }
                    Err(_) => w.write_all(b"550 introuvable.\r\n").await?,
                }
            }
            "PASV" | "EPSV" => {
                let l = TcpListener::bind("127.0.0.1:0").await?;
                let port = l.local_addr()?.port();
                passive = Some(l);
                if u == "EPSV" {
                    w.write_all(format!("229 Mode passif (|||{port}|)\r\n").as_bytes()).await?;
                } else {
                    w.write_all(
                        format!("227 Mode passif (127,0,0,1,{},{})\r\n", port >> 8, port & 0xff).as_bytes(),
                    )
                    .await?;
                }
            }
            "LIST" | "NLST" => {
                let cible = if arg.trim().is_empty() { cwd.clone() } else { resolve(&cwd, arg.trim()) };
                let Some(mut data) = brancher(passive.take()).await? else {
                    w.write_all(b"425 Branchez-vous d'abord.\r\n").await?;
                    continue;
                };
                w.write_all(b"150 Voici la liste.\r\n").await?;
                let mut texte = String::new();
                for (nom, md) in tri(&root, &cible) {
                    if u == "NLST" {
                        texte.push_str(&format!("{nom}\r\n"));
                        continue;
                    }
                    let perm = if md.is_dir() { "drwxr-xr-x" } else { "-rw-r--r--" };
                    texte.push_str(&format!(
                        "{perm}  1 zilla zilla {:>12} {} {nom}\r\n",
                        md.len(),
                        ls_date(horodatage(&reelle(&root, &nom))),
                    ));
                }
                let _ = data.write_all(texte.as_bytes()).await;
                drop(data);
                w.write_all("226 Liste envoyée.\r\n".as_bytes()).await?;
            }
            "STOR" | "APPE" => {
                let Some(data) = brancher(passive.take()).await? else {
                    w.write_all(b"425 Branchez-vous d'abord.\r\n").await?;
                    continue;
                };
                w.write_all("150 Prêt à recevoir.\r\n".as_bytes()).await?;
                let regime_d = regime;
                let gachette = lachette;
                let rendu = format!("226 {} rangé.\r\n", resolve(&cwd, arg.trim()));
                let plume = tx.clone();
                tokio::spawn(async move {
                    enveloppe(data, regime_d, gachette, rendu, plume).await;
                });
            }
            _ => {
                let _ = w.write_all(format!("202 « {c} » n'est faite ici.\r\n").as_bytes()).await?;
            }
        }
    }
}

/// Le paquet de données : on encaisse, ou on n'encaisse pas, et on rend la
/// réponse de fin de transfert une fois le canal refermé.
async fn enveloppe(
    mut data: TcpStream,
    regime: Regime,
    lachette: usize,
    rendu: String,
    plume: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let mut tmp = vec![0u8; 65536];
    let mut recus = 0usize;
    let seuil = if regime == Regime::Coupe { lachette } else { usize::MAX / 2 };
    if regime == Regime::Enlissee {
        // le voisin ouvre le canal et ne lit pratiquement rien : l'écriture du
        // client s'enlise, la ligne reste Active. Et sitôt qu'il la referme, il
        // rend sa réponse — c'est exactement la marche à suivre d'un serveur
        // réel, et c'est bien ce qui restait dans le fil après une annulation.
        let _ = tokio::time::timeout(Duration::from_secs(30), data.read(&mut tmp)).await;
    } else {
        loop {
            match data.read(&mut tmp).await {
                Ok(0) => break,
                Ok(n) => {
                    recus += n;
                    if recus >= seuil {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }
    if regime == Regime::Coupe {
        drop(data);
        let _ = plume.send("426 Le tuyau a lâché.\r\n".to_string());
    } else {
        drop(data);
        let _ = plume.send(rendu);
    }
}

async fn lire(mut r: tokio::net::tcp::OwnedReadHalf, suite: tokio::sync::mpsc::UnboundedSender<Vec<u8>>) {
    loop {
        let mut tmp = [0u8; 4096];
        match r.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => {
                if suite.send(tmp[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = suite.send(Vec::new());
}

async fn brancher(passive: Option<TcpListener>) -> std::io::Result<Option<TcpStream>> {
    let Some(l) = passive else {
        return Ok(None);
    };
    let (s, _) = tokio::time::timeout(Duration::from_secs(20), l.accept())
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "canal non branché"))??;
    let _ = s.set_nodelay(true);
    Ok(Some(s))
}

fn dequeue(buf: &mut String) -> Option<String> {
    if let Some(nl) = buf.find('\n') {
        let ligne = buf[..nl].trim_end_matches('\r').to_string();
        buf.drain(..nl + 1);
        return Some(ligne);
    }
    None
}

fn split2(line: &str) -> (String, String) {
    let (a, b) = line.split_once(' ').unwrap_or((line, ""));
    (a.to_string(), b.trim().to_string())
}

fn reelle(racine: &Path, v: &str) -> PathBuf {
    let v = v.trim_start_matches('/');
    if v.is_empty() {
        racine.to_path_buf()
    } else {
        racine.join(v)
    }
}

fn horodatage(p: &Path) -> i64 {
    std::fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn mlsd_time(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.naive_utc().format("%Y%m%d%H%M%S").to_string())
        .unwrap_or_default()
}

fn ls_date(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.naive_utc().format("%b %e %H:%M").to_string())
        .unwrap_or_default()
}

fn tri(root: &Path, vdir: &str) -> Vec<(String, std::fs::Metadata)> {
    let réel = reelle(root, vdir);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&réel) {
        for e in rd.flatten() {
            if let Ok(m) = std::fs::metadata(e.path()) {
                let nom = format!("/{}", e.path().strip_prefix(root).unwrap_or(e.path().as_path()).display());
                out.push((nom.replace('\\', "/"), m));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn resolve(cwd: &str, cible: &str) -> String {
    if cible.is_empty() {
        return cwd.to_string();
    }
    if cible.starts_with('/') {
        return cible.to_string();
    }
    let mut part: Vec<String> = cwd
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    for p in cible.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                part.pop();
            }
            x => part.push(x.to_string()),
        }
    }
    format!("/{}", part.join("/"))
}

/// attend que le parc ait de nouveau une connexion libre : la ligne doit avoir
/// rendu son créneau, sinon ce n'est plus le même test
async fn la_place_est_rendue(app: &Arc<zilla_rs::App>, site: &str) -> bool {
    for _ in 0..500 {
        if let Some(p) = app.pools.get(&site.to_string()) {
            if p.free_count() >= p.live_count() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

async fn attendre(app: &Arc<zilla_rs::App>, id: &str, secondes: u64) -> (JobState, String) {
    let mut etat = JobState::Waiting;
    let mut erreur = String::new();
    for _ in 0..(secondes * 10) {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(j) = app.transfers.job(id) {
            etat = j.state.clone();
            erreur = j.error.clone();
        }
        if etat != JobState::Waiting && etat != JobState::Active {
            break;
        }
    }
    (etat, erreur)
}

async fn envoi(app: &Arc<zilla_rs::App>, site: &str, local: &Path, distance: &str) -> TransferJob {
    let job = TransferJob::new(
        site.to_string(),
        "voisin".into(),
        Direction::Up,
        distance.into(),
        local.to_string_lossy().to_string(),
        0,
    );
    app.transfers.add(job.clone());
    app.transfers.notify();
    job
}

// -----------------------------------------------------------------------------
// les scènes
// -----------------------------------------------------------------------------

/// Double-clic local, le voisin rend les armes en plein envoi.
#[tokio::test]
async fn une_envie_qui_casse_ne_reste_pas_actif() {
    let (app, site, racine) = poste(Regime::Coupe).await;
    let local = racine.join("a-venir.bin");
    std::fs::write(&local, vec![7u8; 6 * 1024 * 1024]).unwrap();
    let job = envoi(&app, &site, &local, "/a-venu.bin").await;

    let (etat, erreur) = attendre(&app, &job.id, 40).await;
    println!("état = {etat:?} · erreur = {erreur}");
    assert_ne!(etat, JobState::Active, "l'envoi est resté actif sans fin : {erreur}");
    assert!(matches!(etat, JobState::Error), "état attendu = erreur, eu = {etat:?} ({erreur})");
}

/// Double-clic local, le canal casse en plein envoi, et la ligne part à la
/// poubelle avant qu'on ait pu lire la réponse du serveur. C'est la scène
/// vécue : « broken pipe », puis plus de rafraîchissement des dossiers.
#[tokio::test]
async fn une_annulation_apres_la_casse_ne_empoisonne_pas_les_listages() {
    let (app, site, racine) = poste(Regime::Coupe).await;
    let local = racine.join("gosses.bin");
    std::fs::write(&local, vec![9u8; 8 * 1024 * 1024]).unwrap();
    let job = envoi(&app, &site, &local, "/gosses.bin").await;

    tokio::time::sleep(Duration::from_millis(300)).await;
    app.transfers.cancel(&job.id);
    let (etat, erreur) = attendre(&app, &job.id, 40).await;
    println!("après annulation : état = {etat:?} · erreur = {erreur}");
    assert_ne!(etat, JobState::Active, "la ligne annulée reste active : {erreur}");
    assert!(la_place_est_rendue(&app, &site).await, "la ligne annulée n'a jamais rendu son créneau");

    // le serveur a rendu sa réponse de fin de transfert dès qu'il a vu la
    // fermeture : elle est là, dans le fil, et c'est elle qui décidait du
    // sort du listage suivant
    tokio::time::sleep(Duration::from_secs(2)).await;
    la_place_est_rendue(&app, &site).await;
    let debut = std::time::Instant::now();
    let v = tokio::time::timeout(Duration::from_secs(20), app.remote_list(&site, "/")).await;
    match v {
        Ok(Ok(l)) => {
            println!("listage en {:?} · {} entrées", debut.elapsed(), l.entries.len());
            assert!(
                l.entries.iter().any(|e| e.name.ends_with("lu.txt")),
                "le listage est revenu faux : {:?}",
                l.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>()
            );
        }
        Ok(Err(e)) => panic!("plus de listage après l'annulation : {e}"),
        Err(_) => panic!("le listage est resté sourd après l'annulation"),
    }
    assert!(debut.elapsed() < Duration::from_secs(15), "listage trop lent : {:?}", debut.elapsed());
}

/// Un envoi qui s'enlise chez un voisin qui n'encaisse plus, puis qu'on met à
/// la poubelle : la ligne doit rendre sa place, et rien ne doit rester en
/// travers du fil de contrôle.
#[tokio::test]
async fn une_annulation_en_vol_ne_empoisonne_pas_les_listages() {
    let (app, site, racine) = poste(Regime::Enlissee).await;
    let local = racine.join("large.bin");
    std::fs::write(&local, vec![9u8; 8 * 1024 * 1024]).unwrap();
    let job = envoi(&app, &site, &local, "/large.bin").await;

    tokio::time::sleep(Duration::from_millis(500)).await;
    app.transfers.cancel(&job.id);
    let (etat, erreur) = attendre(&app, &job.id, 40).await;
    println!("après annulation : état = {etat:?} · erreur = {erreur}");
    assert_ne!(etat, JobState::Active, "la ligne annulée reste active : {erreur}");
    assert!(la_place_est_rendue(&app, &site).await, "la ligne annulée n'a jamais rendu son créneau");

    tokio::time::sleep(Duration::from_secs(1)).await;
    let debut = std::time::Instant::now();
    let v = tokio::time::timeout(Duration::from_secs(20), app.remote_list(&site, "/")).await;
    if v.is_err() || matches!(v, Ok(Err(_))) {
        println!("--- journal brut ---");
        for l in app.hub.dump() {
            println!("[{}] {}: {}", l.cat, l.level, l.text);
        }
    }
    match v {
        Ok(Ok(l)) => println!("listage en {:?} · {} entrées", debut.elapsed(), l.entries.len()),
        Ok(Err(e)) => panic!("plus de listage après l'annulation : {e}"),
        Err(_) => panic!("le listage est resté sourd après l'annulation"),
    }
    assert!(debut.elapsed() < Duration::from_secs(15), "listage trop lent : {:?}", debut.elapsed());
}

/// La même chose, mais la ligne tombe en erreur sans qu'on l'annule.
#[tokio::test]
async fn une_envie_cassee_laisse_encore_lister_les_dossiers() {
    let (app, site, racine) = poste(Regime::Coupe).await;
    let local = racine.join("petit.bin");
    std::fs::write(&local, vec![3u8; 512 * 1024]).unwrap();
    let job = envoi(&app, &site, &local, "/petit.bin").await;
    let (etat, erreur) = attendre(&app, &job.id, 40).await;
    println!("état = {etat:?} · erreur = {erreur}");

    let debut = std::time::Instant::now();
    let v = tokio::time::timeout(Duration::from_secs(20), app.remote_list(&site, "/")).await;
    match v {
        Ok(Ok(l)) => println!("listage en {:?} · {} entrées", debut.elapsed(), l.entries.len()),
        Ok(Err(e)) => panic!("plus de listage après la casse : {e}"),
        Err(_) => panic!("le listage est resté sourd après la casse"),
    }
}
