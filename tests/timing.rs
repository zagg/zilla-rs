//! Mesure la latence de la main cachée : connexion + transfert côté serveur.

use std::time::{Duration, Instant};
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::App;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn latence_connexion_et_transfer() {
    let root = std::env::temp_dir().join(format!("zilla-lat-{}", std::process::id()));
    let cfg = std::env::temp_dir().join(format!("zilla-latcfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), vec![7u8; 4096]).unwrap();
    let demo = DemoFtp::start(&root).await.unwrap();
    let port = demo.port();
    let app = App::new(cfg.clone(), "test").unwrap();

    let t = Instant::now();
    let (id, _info) = app.quickconnect("127.0.0.1", "anonymous", "", Some(port), "ftp").await.unwrap();
    println!("quickconnect: {:?}", t.elapsed());

    let dl = std::env::temp_dir().join(format!("zilla-latdl-{}", std::process::id()));
    let t = Instant::now();
    app.enqueue_download(&id, &["/a.txt".to_string()], &dl.to_string_lossy(), false)
        .await
        .unwrap();
    println!("enqueue: {:?}", t.elapsed());

    let t = Instant::now();
    loop {
        let j = app.transfers.snapshot();
        let st = j.first().map(|x| x.state.clone()).unwrap();
        if matches!(st, zilla_rs::models::JobState::Done | zilla_rs::models::JobState::Error) {
            break;
        }
        if t.elapsed() > Duration::from_secs(30) {
            panic!("expiration");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let j = app.transfers.snapshot().remove(0);
    println!("transfert {:?} en {:?} ({} octets)", j.state, t.elapsed(), j.transferred);

    drop(demo);
    app.shutdown();
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cfg);
}
