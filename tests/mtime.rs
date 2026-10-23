//! L'horodatage est-il conservé lors des transferts ?
//!
//! Le serveur FTP de démonstration tourne sur le même disque : la date du
//! fichier source côté démo est donc la référence directe.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::models::JobState;

fn mtime_of(p: &Path) -> i64 {
    std::fs::metadata(p)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn wait_done(app: &Arc<zilla_rs::App>) {
    for _ in 0..120 {
        let jobs = app.transfers.snapshot();
        if jobs.iter().all(|j| matches!(j.state, JobState::Done | JobState::Error | JobState::Cancelled)) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("la file des transferts n'est pas terminée");
}

#[tokio::test]
async fn date_conservee_dans_les_deux_sens() {
    let root = std::env::temp_dir().join(format!("zilla-mtd-{}", std::process::id()));
    let cfg = std::env::temp_dir().join(format!("zilla-mtc-{}", std::process::id()));
    let dl = std::env::temp_dir().join(format!("zilla-mtdl-{}", std::process::id()));
    for d in [&root, &cfg, &dl] {
        let _ = std::fs::remove_dir_all(d);
    }
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&cfg).unwrap();
    let src = root.join("historique.txt");
    std::fs::write(&src, "referencement\n").unwrap();
    // on fige la date du fichier source : 2019-09-15T08:30:00
    let stamp = 1_568_545_800i64;
    let f = std::fs::OpenOptions::new().write(true).open(&src).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(stamp as u64))
        .unwrap();
    drop(f);

    let demo = DemoFtp::start(&root).await.expect("démo");
    let port = demo.port();
    let app = Arc::new(zilla_rs::App::new(cfg.clone(), "test").expect("app"));
    let (id, _i) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(port), "ftp")
        .await
        .expect("connexion");

    // ---- téléchargement : le fichier local doit hériter de la date distante
    app.enqueue_download(&id, &["/historique.txt".to_string()], &dl.to_string_lossy(), false)
        .await
        .expect("file");
    wait_done(&app).await;
    let got = mtime_of(&dl.join("historique.txt"));
    assert!(
        (got - stamp).abs() <= 1,
        "date locale {got} différente de la date distante {stamp}"
    );

    // ---- envoi : la date distante doit hériter de la date locale
    let up = std::env::temp_dir().join(format!("zilla-mtup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&up);
    std::fs::create_dir_all(&up).unwrap();
    let lf = up.join("recu.txt");
    std::fs::write(&lf, "depuis le disque local\n").unwrap();
    let f2 = std::fs::OpenOptions::new().write(true).open(&lf).unwrap();
    let stamp2 = stamp + 7200;
    f2.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(stamp2 as u64))
        .unwrap();
    drop(f2);
    app.enqueue_upload(&id, &[lf.to_string_lossy().to_string()], "/", false)
        .await
        .expect("file");
    wait_done(&app).await;
    let remote = PathBuf::from(&root).join("recu.txt");
    let gotr = mtime_of(&remote);
    assert!(
        (gotr - stamp2).abs() <= 1,
        "date distante {gotr} différente de la date locale {stamp2}"
    );
}
