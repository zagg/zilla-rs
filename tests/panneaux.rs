//! Les deux promesses tenues par le poste de travail : le dossier indiqué
//! par le site sert vraiment de point de départ, et une ligne qui se termine
//! réveille les panneaux sans qu'il faille attendre la sonde de la page.

use std::time::Duration;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::models::JobState;
use zilla_rs::App;

async fn poste(remarquette: &str) -> (DemoFtp, std::path::PathBuf, std::path::PathBuf) {
    let racine = std::env::temp_dir().join(format!("zilla-pan-{remarquette}-{}", std::process::id()));
    let reglages = std::env::temp_dir().join(format!("zilla-pancfg-{remarquette}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&racine);
    let _ = std::fs::remove_dir_all(&reglages);
    std::fs::create_dir_all(racine.join("atelier")).unwrap();
    std::fs::write(racine.join("atelier/fiche.txt"), "une fiche\n").unwrap();
    std::fs::write(racine.join("racine.txt"), "la racine\n").unwrap();
    let demo = DemoFtp::start(&racine).await.expect("démo");
    (demo, racine, reglages)
}

fn site(port: u16, id: &str, remote_dir: &str) -> zilla_rs::Site {
    zilla_rs::Site {
        id: id.into(),
        name: "démo".into(),
        host: "127.0.0.1".into(),
        port: Some(port),
        proto: zilla_rs::models::Proto::Ftp,
        user: "anonymous".into(),
        remote_dir: remote_dir.into(),
        ..Default::default()
    }
}

/// Le répertoire demandé au gestionnaire de sites ouvre le panneau, et n'est
/// pas une notice décorative.
#[tokio::test]
async fn le_dossier_du_site_ouvre_le_panneau() {
    let (demo, _racine, reglages) = poste("dir").await;
    let app = App::new(reglages, "test").expect("app");
    let s = app.upsert_site(site(demo.port(), "atelier", "/atelier/"));
    let info = app.connect(&s).await.expect("connexion");
    assert_eq!(info.server_dir, "/atelier", "le panneau aurait dû démarrer dans le dossier du site");

    // le chemin du milieu de la racine, lui, doit rester la racine
    let ailleurs = app.upsert_site(site(demo.port(), "partout", "/"));
    let info = app.connect(&ailleurs).await.expect("connexion2");
    assert_eq!(info.server_dir, "/");

    // un nom sans racine s'appuie sur le répertoire courant du serveur
    let sous = app.upsert_site(site(demo.port(), "sous", "atelier"));
    let info = app.connect(&sous).await.expect("connexion3");
    assert!(
        info.server_dir.ends_with("atelier"),
        "un chemin relatif aurait dû s'accoler au répertoire du serveur : {}",
        info.server_dir
    );
}

/// Un chemin introuvable ne doit pas empêcher de démarrer : on tombe à la
/// racine et le journal le raconte.
#[tokio::test]
async fn un_dossier_du_site_introuvable_ne_refuse_pas_laconnexion() {
    let (demo, _racine, reglages) = poste("raté").await;
    let app = App::new(reglages, "test").expect("app");
    let s = app.upsert_site(site(demo.port(), "fantome", "/il-n-y-a-pas"));
    let info = app.connect(&s).await.expect("connexion");
    assert_eq!(info.server_dir, "/", "faute de mieux, on démarre à la racine");
}

/// L'entente tenue entre la page et le serveur au moment de la connexion :
/// le poste rend son dossier d'attache et lui rappelle celui du site.
#[tokio::test]
async fn la_connect_api_annonce_les_deux_dossiers() {
    let (demo, _racine, reglages) = poste("api").await;
    let app = App::new(reglages, "test").expect("app");
    let mut s = site(demo.port(), "atelier", "/atelier");
    s.local_dir = "/tmp".into();
    let s = app.upsert_site(s);

    let écoute = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("écoute");
    let pied = écoute.local_addr().expect("adresse");
    let route = zilla_rs::api::router(app.clone());
    tokio::spawn(async move {
        let _ = axum::serve(écoute, route).await;
    });

    let corps = format!("{{\"site_id\":\"{}\"}}", s.id);
    let reponse = mander(pied, "POST", "/api/connect", &corps).await;
    let i = serde_json::from_str::<serde_json::Value>(&reponse).expect("json");
    let donnees = &i["data"];
    assert_eq!(donnees["server_dir"], "/atelier", "le dossier distant du site");
    assert_eq!(donnees["local_dir"], "/tmp", "le dossier local du site");
    assert_eq!(donnees["host"], "127.0.0.1", "la page compte encore là-dessus");
}

/// Une requête HTTP à la main, sans détours ni dépendance de plus.
async fn mander(addr: std::net::SocketAddr, methode: &str, chemin: &str, corps: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut c = tokio::net::TcpStream::connect(addr).await.expect("connexion TCP");
    let requete = format!(
        "{methode} {chemin} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{corps}",
        corps.len()
    );
    c.write_all(requete.as_bytes()).await.expect("envoi");
    let mut tout = Vec::new();
    let _ = c.read_to_end(&mut tout).await;
    let texte = String::from_utf8_lossy(&tout).to_string();
    texte
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string()
}

/// Une ligne qui se termine publie bien son panneau : sinon le dossier reste
/// figé sur ce qu'il croyait savoir, et l'utilisateur croit que rien est arrivé.
#[tokio::test]
async fn la_fin_dun_download_reveille_le_panneau_local() {
    let (demo, _racine, reglages) = poste("fin").await;
    let app = App::new(reglages, "test").expect("app");
    let destination = std::env::temp_dir().join(format!("zilla-pan-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&destination);

    let (id, _info) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(demo.port()), "ftp")
        .await
        .expect("connexion");
    let mut rx = app.hub.subscribe();
    app.enqueue_download(&id, &["/racine.txt".to_string()], &destination.to_string_lossy(), false)
        .await
        .expect("file");

    let mut reveil_local = 0;
    let mut reveil_file = 0;
    let mut fini = false;
    for _ in 0..100 {
        while let Ok(ev) = rx.try_recv() {
            if ev.kind == "dirty" {
                let what = ev.data.get("what").and_then(|v| v.as_str()).unwrap_or("");
                if what == "local" {
                    reveil_local += 1;
                }
                if what == "queue" {
                    reveil_file += 1;
                }
            }
        }
        if let Some(j) = app.transfers.snapshot().first() {
            if j.state == JobState::Done {
                fini = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(fini, "le téléchargement ne s'est pas terminé");
    assert!(
        reveil_local > 0,
        "le panneau local n'a jamais été prévenu que le dossier venait de changer"
    );
    assert!(reveil_file > 0, "la file n'a jamais été prévenue de l'état final");
    app.transfers.stop();
}

/// La même chose côté envoi : c'est le panneau distant qui prend le choc.
#[tokio::test]
async fn la_fin_dun_envoi_reveille_le_panneau_distant() {
    let (demo, _racine, reglages) = poste("envoi").await;
    let app = App::new(reglages, "test").expect("app");
    let source = std::env::temp_dir().join(format!("zilla-pan-up-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&source);
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("pousse.txt"), "poussé\n").unwrap();

    let (id, _info) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(demo.port()), "ftp")
        .await
        .expect("connexion");
    let mut rx = app.hub.subscribe();
    app.enqueue_upload(
        &id,
        &[source.join("pousse.txt").to_string_lossy().to_string()],
        "/atelier",
        false,
    )
    .await
    .expect("file");

    let mut reveil_remote = 0;
    let mut fini = false;
    for _ in 0..100 {
        while let Ok(ev) = rx.try_recv() {
            if ev.kind == "dirty"
                && ev.data.get("what").and_then(|v| v.as_str()) == Some("remote")
            {
                reveil_remote += 1;
            }
        }
        if let Some(j) = app.transfers.snapshot().first() {
            if j.state == JobState::Done {
                fini = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(fini, "l'envoi ne s'est pas terminé");
    assert!(
        reveil_remote > 0,
        "le panneau distant n'a jamais été prévenu que le dossier venait de changer"
    );
    app.transfers.stop();
}
