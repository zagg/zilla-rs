//! Un port de service pour le harnais vivant (`tools/harnais-vivant.js`) : une
//! vraie API, un vrai FTP de démonstration, un site favori. Ce n'est pas un
//! test — c'est une prise qu'on branche quand on veut vérifier la page contre
//! autre chose que des promesses écrites à la main :
//!
//! ```text
//! cargo test --test pont -- --ignored --nocapture &   // tenu 3 min, ZILLA_PONT=<s> pour durer
//! node tools/harnais-vivant.js
//! ```

use std::time::Duration;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::App;

#[tokio::test]
#[ignore = "tient un port ouvert pour le harnais vivant — `cargo test --test pont -- --ignored`"]
async fn un_port_sert_des_verites() {
    let racine = std::env::temp_dir().join(format!("zilla-pont-{}", std::process::id()));
    let reglages = std::env::temp_dir().join(format!("zilla-pontcfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&racine);
    let _ = std::fs::remove_dir_all(&reglages);
    std::fs::create_dir_all(&racine).unwrap();
    std::fs::write(racine.join("racine.txt"), "la racine\n").unwrap();
    let demo = DemoFtp::start(&racine).await.expect("démo");
    let app = App::new(reglages, "test").expect("poste");
    app.upsert_site(zilla_rs::Site {
        id: "atelier".into(),
        name: "démo".into(),
        host: "127.0.0.1".into(),
        port: Some(demo.port()),
        proto: zilla_rs::models::Proto::Ftp,
        user: "anonymous".into(),
        remote_dir: "/".into(),
        favorite: true,
        ..Default::default()
    });

    let écoute = tokio::net::TcpListener::bind("127.0.0.1:8745").await.expect("écoute");
    let route = zilla_rs::api::router(app.clone());
    tokio::spawn(async move { let _ = axum::serve(écoute, route).await; });
    let tenu = std::env::var("ZILLA_PONT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(180);
    println!(" pont sur http://127.0.0.1:8745 (démo sur {p}) — tenu {tenu} s", p = demo.port());
    tokio::time::sleep(Duration::from_secs(tenu)).await;
    app.shutdown();
    let _ = demo.stop();
}
