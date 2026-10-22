//! Le pool de connexions réutilise les liaisons, comme FileZilla.

use std::time::Duration;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::models::{EntryKind, Proto};
use zilla_rs::App;

fn site(port: u16) -> zilla_rs::Site {
    zilla_rs::Site {
        id: "pool-test".into(),
        name: "démo".into(),
        host: "127.0.0.1".into(),
        port: Some(port),
        proto: Proto::Ftp,
        user: "anonymous".into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn pool_reutilie_les_connexions() {
    let root = std::env::temp_dir().join(format!("zilla-pool-{}", std::process::id()));
    let cfg = std::env::temp_dir().join(format!("zilla-poolcfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cfg);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "aaaa").unwrap();
    std::fs::create_dir_all(root.join("d")) .unwrap();
    let demo = DemoFtp::start(&root).await.unwrap();
    let port = demo.port();

    let app = App::new(cfg.clone(), "test").expect("app");
    let s = site(port);
    let settings = app.settings();

    let pool = app.pool_for(&s, &settings).await.expect("pool");
    let l1 = pool.lease(Duration::from_secs(8)).await.expect("lease1");
    assert_eq!(pool.live_count(), 1);
    let l = l1.fs().list("/").await.expect("list");
    assert!(l.entries.iter().any(|e| e.kind == EntryKind::File));
    drop(l1);

    let l2 = pool.lease(Duration::from_secs(8)).await.expect("lease2");
    assert_eq!(pool.live_count(), 1, "la connexion doit être réutilisée");
    l2.fs().list("/d").await.expect("list2");

    // Deux locations simultanées : deux connexions.
    let l3 = pool.lease(Duration::from_secs(8)).await.expect("lease3");
    assert_eq!(pool.live_count(), 2);

    let st2 = settings.clone();
    pool.close_all();
    drop((l2, l3, st2));
    app.shutdown();

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&cfg);
    drop(demo);
}
