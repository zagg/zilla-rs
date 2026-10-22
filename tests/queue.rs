//! Test de bout en bout : mini-serveur FTP + moteur de transferts.

use std::sync::Arc;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::models::JobState;

#[tokio::test]
async fn download_puis_upload_via_queue() {
    let root = std::env::temp_dir().join(format!("zilla-q-{}", std::process::id()));
    let cfg = std::env::temp_dir().join(format!("zilla-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fichier.txt"), "contenu de test\n").unwrap();
    std::fs::create_dir_all(root.join("sous")).unwrap();
    std::fs::write(root.join("sous/lourd.bin"), vec![9u8; 90_000]).unwrap();

    let demo = DemoFtp::start(&root).await.expect("démo");
    let port = demo.port();

    let app = Arc::new(zilla_rs::App::new(cfg.clone(), "test").expect("app"));

    let (id, _info) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(port), "ftp")
        .await
        .expect("connexion");

    // --- téléchargement d'un fichier simple
    let dl = std::env::temp_dir().join(format!("zilla-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dl);
    app.enqueue_download(&id, &["/fichier.txt".to_string()], &dl.to_string_lossy(), false)
        .await
        .expect("file");

    let mut st = JobState::Waiting;
    let mut size_seen = 0u64;
    for _ in 0..80 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let jobs = app.transfers.snapshot();
        if let Some(j) = jobs.first() {
            st = j.state.clone();
            size_seen = j.transferred;
            if j.state == JobState::Done || j.state == JobState::Error {
                break;
            }
        }
    }
    assert!(
        st == JobState::Done,
        "téléchargement pas terminé ({:?}) — {}",
        st,
        app.transfers
            .snapshot()
            .first()
            .map(|j| j.error.clone())
            .unwrap_or_default()
    );
    assert_eq!(size_seen, 16, "octets comptabilisés");
    assert_eq!(
        std::fs::read_to_string(dl.join("fichier.txt")).unwrap(),
        "contenu de test\n"
    );

    // --- un dossier entier, en mode « récursif »
    app.enqueue_download(&id, &["/sous".to_string()], &dl.to_string_lossy(), true)
        .await
        .expect("file");
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if std::fs::metadata(dl.join("sous/lourd.bin")).is_ok() {
            break;
        }
    }
    assert_eq!(std::fs::metadata(dl.join("sous/lourd.bin")).unwrap().len(), 90_000);

    // --- envoi d'un fichier local
    let up = std::env::temp_dir().join(format!("zilla-up-{}", std::process::id()));
    std::fs::create_dir_all(&up).unwrap();
    let src = up.join("envoyé.txt");
    std::fs::write(&src, "téléversé\n").unwrap();
    app.transfers.clear_done();
    app.enqueue_upload(&id, &[src.to_string_lossy().to_string()], "/", false)
        .await
        .expect("file");
    for _ in 0..80 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if root.join("envoyé.txt").exists() {
            break;
        }
    }
    assert!(root.join("envoyé.txt").exists());
    let done = app
        .transfers
        .snapshot()
        .iter()
        .any(|j| j.state == JobState::Done);
    assert!(done, "l'envoi n'est pas terminé");

    app.shutdown();
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cfg);
    let _ = std::fs::remove_dir_all(&dl);
    let _ = std::fs::remove_dir_all(&up);
    drop(demo);
}
