//! Les deux modes de transfert FTP (passif et actif) doivent fonctionner.

use std::time::Duration;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::models::JobState;
use zilla_rs::App;

async fn transfert(mode: Option<bool>, app: &App, id: &str, dir: &std::path::Path) {
    let _ = app.transfers.clear_done();
    app.enqueue_download(id, &["/fichier.txt".to_string()], &dir.to_string_lossy(), false)
        .await
        .expect("file");
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let j = app.transfers.snapshot();
        match j.first().map(|x| x.state.clone()) {
            Some(JobState::Done) => return,
            Some(JobState::Error) => panic!(
                "transfert en erreur : {:?}",
                j.first().map(|x| x.error.clone())
            ),
            _ => {}
        }
    }
    panic!("transfert non terminé en 5 s");
}

async fn avec_mode(active: bool) {
    let n = std::process::id();
    let root = std::env::temp_dir().join(format!("zilla-mroot-{n}-{active}"));
    let cfg = std::env::temp_dir().join(format!("zilla-mcfg-{n}-{active}"));
    let dl = std::env::temp_dir().join(format!("zilla-mdl-{n}-{active}"));
    for p in [&root, &cfg, &dl] {
        let _ = std::fs::remove_dir_all(p);
    }
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fichier.txt"), "mode de transfert\n").unwrap();
    let demo = DemoFtp::start(&root).await.unwrap();
    let app = App::new(cfg.clone(), "test").unwrap();
    let (id, info) = app
        .quickconnect_ex(
            "127.0.0.1",
            "anonymous",
            "",
            Some(demo.port()),
            "ftp",
            None,
            Some(!active),
            None,
        )
        .await
        .unwrap();
    assert_eq!(info.active, active, "le mode annoncé ne correspond pas");
    transfert(None, &app, &id, &dl).await;
    assert_eq!(
        std::fs::read_to_string(dl.join("fichier.txt")).unwrap(),
        "mode de transfert\n"
    );
    app.shutdown();
    for p in [&root, &cfg, &dl] {
        let _ = std::fs::remove_dir_all(p);
    }
    drop(demo);
}

#[tokio::test]
async fn mode_passif() {
    avec_mode(false).await;
}

#[tokio::test]
async fn mode_actif() {
    avec_mode(true).await;
}
