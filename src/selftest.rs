use crate::demosrv::DemoFtp;
use crate::ftp::FtpFs;
use crate::fs::RemoteFs;
use crate::hub::LogHub;
use crate::models::{EntryKind, LoginType, Proto, Site, TlsLevel};
use crate::settings::Settings;
use crate::util::now_ms;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// Déroule un parcours complet client/serveur et rend le verdict.
pub async fn run(version: &str) -> Value {
    let started = now_ms();
    let mut checks: Vec<Value> = Vec::new();
    let mut failures = 0usize;

    let root = std::env::temp_dir().join(format!("zilla-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("docs")).ok();
    std::fs::write(root.join("hello.txt"), b"Bonjour Zilla-RS\n").ok();
    std::fs::write(root.join("docs/liseuse.md"), b"# test\n").ok();
    std::fs::write(root.join("big.bin"), vec![7u8; 300_000]).ok();

    let demo = match DemoFtp::start(&root).await {
        Ok(d) => d,
        Err(e) => return json!({"ok": false, "error": format!("serveur de démo : {e}")}),
    };
    log(&mut checks, "serveur-démo", demo.port() > 0, &demo.url());

    let mut site = Site::default();
    site.id = "selftest".into();
    site.name = "Démo".into();
    site.host = demo.host();
    site.port = Some(demo.port());
    site.proto = Proto::Ftp;
    site.login = LoginType::Anonymous;
    site.tls = TlsLevel::Plain;

    let hub = Arc::new(LogHub::new(200));
    let settings = Settings::default();
    let fs = match FtpFs::connect(&site, &settings, &hub).await {
        Ok(f) => Arc::new(f),
        Err(e) => {
            demo.stop();
            let _ = std::fs::remove_dir_all(&root);
            return json!({"ok": false, "error": format!("connexion : {e}")});
        }
    };
    log(&mut checks, "connexion", true, "");

    // LIST
    let l = fs.list("/").await;
    let names: Vec<String> = l
        .as_ref()
        .map(|l| l.entries.iter().map(|e| e.name.clone()).collect())
        .unwrap_or_default();
    check(
        &mut checks,
        &mut failures,
        "listage-racine",
        l.is_ok() && names.contains(&"hello.txt".to_string()) && names.contains(&"docs".to_string()),
        &format!("{names:?}"),
    );

    // Entrée de type dossier.
    let docs = fs.stat("/docs").await;
    check(
        &mut checks,
        &mut failures,
        "stat-répertoire",
        docs.as_ref().map(|e| e.kind == EntryKind::Dir).unwrap_or(false),
        "",
    );

    // Taille via SIZE.
    let size = fs.size("/hello.txt").await;
    check(&mut checks, &mut failures, "taille-fichier", size == Some(17), &format!("{size:?}"));

    // Téléchargement.
    let mut src = fs.open_read("/hello.txt", 0).await.ok();
    let mut got = Vec::new();
    if let Some(s) = &mut src {
        let mut buf = vec![0u8; 4096];
        loop {
            match s.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(e) => {
                    got.clear();
                    let _ = e;
                    break;
                }
            }
        }
        let _ = s.close().await;
    }
    check(
        &mut checks,
        &mut failures,
        "téléchargement",
        got == b"Bonjour Zilla-RS\n".to_vec(),
        &format!("{} octets", got.len()),
    );

    // Téléchargement d'un gros fichier + reprise.
    let mut big = Vec::new();
    if let Ok(mut s) = fs.open_read("/big.bin", 100_000).await {
        let mut buf = vec![0u8; 65536];
        loop {
            match s.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => big.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let _ = s.close().await;
    }
    check(
        &mut checks,
        &mut failures,
        "reprise-partielle",
        big.len() == 200_000,
        &format!("{} octets", big.len()),
    );

    // Envoi.
    let up = "/up.txt";
    {
        let mut sink = fs.open_write(up, 0, false).await.expect("open_write");
        sink.write("envoyé par zilla-rs".as_bytes()).await.expect("write");
        sink.finish().await.expect("finish");
    }
    let size2 = fs.size(up).await;
    check(&mut checks, &mut failures, "téléversement", size2 == Some(20), &format!("{size2:?}"));
    let exists = root.join("up.txt").exists();
    check(&mut checks, &mut failures, "fichier-créé-sur-disque", exists, "");

    // Dossier + renommage + suppression.
    let mk = fs.mkdir("/nouveau").await;
    check(&mut checks, &mut failures, "création-dossier", mk.is_ok(), "");
    let rn = fs.rename("/up.txt", "/up2.txt").await;
    let after = fs.stat("/up2.txt").await;
    check(&mut checks, &mut failures, "rebaptiser", rn.is_ok() && after.is_ok(), "");
    let rm = fs.remove("/up2.txt").await;
    check(&mut checks, &mut failures, "supprimer", rm.is_ok(), "");
    let rmdir = fs.rmdir("/nouveau").await;
    check(&mut checks, &mut failures, "supprimer-dossier", rmdir.is_ok(), "");

    // Téléchargement d'arborescence complète par listings.
    let l2 = fs.list("/docs").await;
    check(
        &mut checks,
        &mut failures,
        "listage-sous-dossier",
        l2.as_ref().map(|l| !l.entries.is_empty()).unwrap_or(false),
        "",
    );

    // Keep-alive.
    let p = fs.ping().await;
    check(&mut checks, &mut failures, "keepalive", p.is_ok(), "");

    let info = fs.info().await;
    check(
        &mut checks,
        &mut failures,
        "métadonnées-de-connexion",
        info.port == demo.port() && !info.banner.is_empty(),
        &info.banner,
    );

    fs.shutdown().await;
    demo.stop();
    let _ = std::fs::remove_dir_all(&root);

    json!({
        "ok": failures == 0,
        "version": version,
        "passes": checks.iter().filter(|c| c["ok"].as_bool().unwrap_or(false)).count(),
        "failures": failures,
        "checks": checks,
        "ms": now_ms() - started,
    })
}

fn log(v: &mut Vec<Value>, name: &str, ok: bool, msg: &str) {
    v.push(json!({"name": name, "ok": ok, "note": msg}));
}

fn check(v: &mut Vec<Value>, f: &mut usize, name: &str, ok: bool, note: &str) {
    if !ok {
        *f += 1;
    }
    v.push(json!({"name": name, "ok": ok, "note": note}));
}

/// Chemin de démonstration des répertoires locaux.
pub fn demo_dir() -> PathBuf {
    std::env::temp_dir().join("zilla-rs-demo")
}
