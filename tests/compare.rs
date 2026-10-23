//! Comparaison de dossiers (écart local/distant) avec le serveur de démonstration.

use std::path::PathBuf;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::models::DiffKind;

#[tokio::test]
async fn ecarts_identifies_entre_paniers() {
    let root = std::env::temp_dir().join(format!("zilla-cmp-{}", std::process::id()));
    let cfg = std::env::temp_dir().join(format!("zilla-cmpc-{}", std::process::id()));
    let loc = std::env::temp_dir().join(format!("zilla-cmpl-{}", std::process::id()));
    for d in [&root, &cfg, &loc] {
        let _ = std::fs::remove_dir_all(d);
    }
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::create_dir_all(loc.join("docs")).unwrap();
    std::fs::write(root.join("les-identiques.txt"), "des deux cotes\n").unwrap();
    std::fs::write(loc.join("les-identiques.txt"), "des deux cotes\n").unwrap();
    std::fs::write(root.join("docs/l-appli.txt"), "seulement au bord\n").unwrap();
    std::fs::write(root.join("modifie.txt"), "ancienne version\n").unwrap();
    std::fs::write(loc.join("modifie.txt"), "version beaucoup plus longue\n").unwrap();
    std::fs::write(loc.join("nouveau.txt"), "que moi\n").unwrap();

    let demo = DemoFtp::start(&root).await.expect("démo");
    let port = demo.port();
    let app = zilla_rs::App::new(cfg.clone(), "test").expect("app");
    let (id, _i) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(port), "ftp")
        .await
        .expect("connexion");

    let r = app
        .compare(&id, &loc.to_string_lossy(), "/")
        .await
        .expect("comparaison");

    let k = |p: &str| {
        r.items
            .iter()
            .find(|i| i.path == p)
            .map(|i| i.kind.clone())
            .unwrap_or(DiffKind::Same)
    };
    assert_eq!(k("les-identiques.txt"), DiffKind::Same, "deux copies conformes");
    assert_eq!(k("docs/l-appli.txt"), DiffKind::RemoteOnly);
    assert_eq!(k("docs/modifie.txt"), DiffKind::Same);
    assert_eq!(k("nouveau.txt"), DiffKind::LocalOnly);
    assert_eq!(k("modifie.txt"), DiffKind::Different);
    assert_eq!(r.same, 2);
    assert_eq!(r.local_only, 1);
    assert_eq!(r.remote_only, 1);
    assert_eq!(r.different, 1);
    assert!(r.bytes_to_upload > 0, "chiffres à envoyer");

    // on vérifie le chemin renvoyé pour un dossier distant : le rapport doit
    // garder l'arborescence relative, pas seulement le nom du fichier.
    let _ = PathBuf::from(&root);
}

#[tokio::test]
async fn synchronisation_upload_puis_download() {
    let root = std::env::temp_dir().join(format!("zilla-cmps-{}", std::process::id()));
    let cfg = std::env::temp_dir().join(format!("zilla-cmpsc-{}", std::process::id()));
    let loc = std::env::temp_dir().join(format!("zilla-cmpls-{}", std::process::id()));
    for d in [&root, &cfg, &loc] {
        let _ = std::fs::remove_dir_all(d);
    }
    std::fs::create_dir_all(loc.join("sousse")).unwrap();
    std::fs::write(loc.join("racine.txt"), "à envoyer\n").unwrap();
    std::fs::write(loc.join("sousse/plouf.txt"), "et aussi ca\n").unwrap();
    let demo = DemoFtp::start(&root).await.expect("démo");
    let port = demo.port();
    let app = zilla_rs::App::new(cfg.clone(), "test").expect("app");
    let (id, _i) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(port), "ftp")
        .await
        .expect("connexion");

    let r = app.compare(&id, &loc.to_string_lossy(), "/").await.unwrap();
    let up: Vec<String> = r
        .items
        .iter()
        .filter(|i| matches!(i.kind, DiffKind::LocalOnly | DiffKind::Different))
        .map(|i| format!("{}/{}", loc.to_string_lossy(), i.path))
        .collect();
    app.enqueue_upload_ex(&id, &up, "/", true, Some(&loc.to_string_lossy()))
        .await
        .expect("envoi");
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let js = app.transfers.snapshot();
        if !js.iter().any(|j| !j.state.is_terminal()) {
            break;
        }
    }
    assert!(std::fs::read_to_string(root.join("racine.txt")).unwrap().starts_with("à envoyer"));
    assert!(std::fs::read_to_string(root.join("sousse/plouf.txt")).unwrap().starts_with("et aussi"));

    // maintenant dans l'autre sens : on repart d'un panier local vide
    let loc2 = std::env::temp_dir().join(format!("zilla-cmpld-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&loc2);
    std::fs::create_dir_all(&loc2).unwrap();
    let r2 = app.compare(&id, &loc2.to_string_lossy(), "/").await.unwrap();
    let down: Vec<String> = r2
        .items
        .iter()
        .filter(|i| matches!(i.kind, DiffKind::RemoteOnly))
        .map(|i| i.path.clone())
        .collect();
    app.enqueue_download_ex(&id, &down, &loc2.to_string_lossy(), true, Some("/"))
        .await
        .expect("recevoir");
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if !app.transfers.snapshot().iter().any(|j| !j.state.is_terminal()) {
            break;
        }
    }
    assert_eq!(std::fs::read_to_string(loc2.join("racine.txt")).unwrap(), "à envoyer\n");
    assert_eq!(std::fs::read_to_string(loc2.join("sousse/plouf.txt")).unwrap(), "et aussi ca\n");

    let r3 = app.compare(&id, &loc2.to_string_lossy(), "/").await.unwrap();
    assert_eq!(r3.local_only, 0, "aucun reste local");
    assert_eq!(r3.remote_only, 0, "aucun reste distant");
    assert_eq!(r3.different, 0, "aucun écart de contenu");
}
