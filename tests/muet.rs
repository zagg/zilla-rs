//! Un voisin qui se tait en plein envoi ne doit rien figer.
//!
//! Avant, une écriture sur un canal abandonné attendait sans fin : la ligne
//! restait Active, le créneau du parc avec elle, et ni les listages ni les
//! lignes suivantes ne respiraient plus — seul un redémarrage du service s'en
//! dépêtrait. Ici le canal est borné dans le temps, la ligne tombe en erreur
//! et la place est rendue.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use zilla_rs::fs::{FsResult, Pool, RemoteFs, Sink, Src};
use zilla_rs::models::{ConnInfo, Entry, JobState, Listing, Proto};

/// Un aller-retour qui dort `silence` — assez longtemps pour être coupé par le
/// bornage du canal, assez court pour qu'une annulation le voit passer.
#[derive(Clone)]
struct Sommeil {
    silence: Duration,
}

#[async_trait]
impl Sink for Sommeil {
    async fn write(&mut self, _data: &[u8]) -> std::io::Result<()> {
        tokio::time::sleep(self.silence).await;
        Ok(())
    }
}

#[async_trait]
impl Src for Sommeil {
    async fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        tokio::time::sleep(self.silence).await;
        Ok(0)
    }
}

/// Serveur muet : tout répond, mais les octets ne passent plus.
#[derive(Clone)]
struct Muet {
    silence: Duration,
}

impl Muet {
    fn neuve(secs: u64) -> Muet {
        Muet { silence: Duration::from_secs(secs) }
    }
}

#[async_trait]
impl RemoteFs for Muet {
    async fn list(&self, path: &str) -> FsResult<Listing> {
        Ok(Listing {
            path: path.into(),
            entries: Vec::new(),
            source: "test".into(),
            raw: String::new(),
            truncated: false,
        })
    }
    async fn stat(&self, path: &str) -> FsResult<Entry> {
        let name = path.rsplit('/').next().unwrap_or("x").to_string();
        Ok(Entry::dir(name, None))
    }
    async fn realpath(&self, path: &str) -> FsResult<String> {
        Ok(path.into())
    }
    async fn mkdir(&self, _p: &str) -> FsResult<()> {
        Ok(())
    }
    async fn rmdir(&self, _p: &str) -> FsResult<()> {
        Ok(())
    }
    async fn remove(&self, _p: &str) -> FsResult<()> {
        Ok(())
    }
    async fn rename(&self, _f: &str, _t: &str) -> FsResult<()> {
        Ok(())
    }
    async fn chmod(&self, _p: &str, _m: u32) -> FsResult<()> {
        Ok(())
    }
    async fn chown(&self, _p: &str, _u: &str) -> FsResult<()> {
        Ok(())
    }
    async fn set_mtime(&self, _p: &str, _t: i64) -> FsResult<()> {
        Ok(())
    }
    async fn exists(&self, _p: &str) -> bool {
        true
    }
    async fn open_read(&self, _p: &str, _o: u64) -> FsResult<Box<dyn Src + Send>> {
        Ok(Box::new(Sommeil { silence: self.silence }))
    }
    async fn open_write(&self, _p: &str, _o: u64, _a: bool) -> FsResult<Box<dyn Sink + Send>> {
        Ok(Box::new(Sommeil { silence: self.silence }))
    }
    async fn ping(&self) -> FsResult<()> {
        Ok(())
    }
    async fn shutdown(&self) {}
    async fn info(&self) -> ConnInfo {
        ConnInfo::default()
    }
    fn id(&self) -> String {
        "muet".into()
    }
    fn is_closed(&self) -> bool {
        false
    }
    fn last_used(&self) -> i64 {
        zilla_rs::util::now_ms()
    }
    fn touch(&self) {}
    fn proto(&self) -> Proto {
        Proto::Ftp
    }
}

/// Le poste de test : un dossier de travail, un site connecté, un parc muet.
async fn poste(griffe: &'static str, seconde_coup: u64, silence_secs: u64) -> (Arc<zilla_rs::App>, String, Arc<Pool>, std::path::PathBuf, zilla_rs::demosrv::DemoFtp) {
    let racine = std::env::temp_dir().join(format!("zilla-muet-{}-{}/", std::process::id(), griffe));
    let reglages = std::env::temp_dir().join(format!("zilla-muet-cfg-{}-{}", std::process::id(), griffe));
    let _ = std::fs::remove_dir_all(&racine);
    let _ = std::fs::remove_dir_all(&reglages);
    std::fs::create_dir_all(&racine).unwrap();
    std::fs::write(racine.join("bord.txt"), vec![7u8; 1024 * 1024]).unwrap();

    let demo = zilla_rs::demosrv::DemoFtp::start(&racine).await.expect("démo");
    let app = zilla_rs::App::new(reglages, "test").expect("app");
    let (site, _info) = app
        .quickconnect("127.0.0.1", "anonymous", "", Some(demo.port()), "ftp")
        .await
        .expect("connexion");
    {
        let mut s = app.store.write().unwrap();
        s.settings.net.data_timeout = seconde_coup;
        s.settings.transfers.retries = 0;
        s.settings.transfers.max_parallel = 1;
    }
    let parc = Arc::new(Pool::new(1, 30, move || {
        let fs = Muet::neuve(silence_secs);
        let b: zilla_rs::fs::Boxed = Box::pin(async move { Ok(Arc::new(fs) as Arc<dyn RemoteFs>) });
        b
    }));
    app.pools.put(&site, parc.clone());
    let st = app.site(&site).expect("site");
    let repris = app.pool_for(&st, &app.settings()).await.unwrap();
    let _ = repris;
    (app, site, parc, racine, demo)
}

#[tokio::test]
async fn le_geste_a_l_envers_se_dit_a_temps() {
    // Un dossier envoyé comme un fichier, ou un dossier téléchargé comme un
    // fichier : c'est le geste à l'envers, et c'est typiquement ce qui cassait
    // en plein vol avec un tuyau ouvert dans le vide.
    let (app, site, _parc, travaux, _garde) = poste("àlenvers", 5, 5).await;
    let dossier = travaux.join("dossier".to_string());
    std::fs::create_dir_all(dossier.join("en dedans")).unwrap();

    let envoi = zilla_rs::models::TransferJob::new(
        site.clone(),
        "voisin".into(),
        zilla_rs::models::Direction::Up,
        "/de-jaune.txt".into(),
        dossier.to_string_lossy().to_string(),
        0,
    );
    app.transfers.add(envoi.clone());
    app.transfers.notify();

    let reception = zilla_rs::models::TransferJob::new(
        site.clone(),
        "voisin".into(),
        zilla_rs::models::Direction::Down,
        "/dossier-bleu".into(),
        travaux.join("reçu.txt").to_string_lossy().to_string(),
        0,
    );
    app.transfers.add(reception.clone());
    app.transfers.notify();

    let mut messages = std::collections::HashMap::new();
    let mut tombes = 0;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        for j in app.transfers.snapshot() {
            if (j.id == envoi.id || j.id == reception.id) && j.state == JobState::Error {
                messages.insert(j.id.clone(), j.error.clone());
            }
        }
        tombes = messages.len();
        if tombes == 2 {
            break;
        }
    }
    assert_eq!(tombes, 2, "les deux gestes à l'envers n'ont pas été signalés");
    let e = &messages[&envoi.id];
    let r = &messages[&reception.id];
    assert!(e.contains("dossier local"), "envoi : {e}");
    assert!(r.contains("dossier côté serveur"), "réception : {r}");
    assert_eq!(app.transfers.stats().active, 0);
    app.transfers.stop();
}

#[tokio::test]
async fn un_voisin_muet_ne_rien_pas_tout_le_monde() {
    // canal borné à 2 s, sans retry : la ligne doit tomber toute seule
    let (app, site, parc, travaux, _garde) = poste("muet", 2, 3600).await;
    app.enqueue_upload(&site, &[travaux.join("bord.txt").to_string_lossy().to_string()], "/", false)
        .await
        .expect("file");

    let mut etat = JobState::Waiting;
    let mut erreur = String::new();
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(j) = app.transfers.snapshot().first() {
            etat = j.state.clone();
            erreur = j.error.clone();
            if j.state == JobState::Error || j.state == JobState::Done {
                break;
            }
        }
    }
    assert!(etat == JobState::Error, "la ligne aurait dû tomber, sinon c'est le blocage d'origine ({etat:?} — {erreur})");
    assert!(erreur.contains("canal"), "message d'erreur pas causant : {erreur}");
    assert_eq!(app.transfers.stats().active, 0, "un créneau est encore occupé");
    assert_eq!(parc.free_count(), 1, "la connexion n'est pas rendue au parc");

    // une seconde ligne doit repartir, donc la file n'est plus bouchée
    app.enqueue_upload(&site, &[travaux.join("bord.txt").to_string_lossy().to_string()], "/", false)
        .await
        .expect("file");
    let repartie = 'corps: {
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let v: Vec<JobState> = app.transfers.snapshot().iter().map(|j| j.state.clone()).collect();
            if v.iter().filter(|s| **s == JobState::Error).count() >= 2 {
                break 'corps true;
            }
        }
        false
    };
    assert!(repartie, "la seconde ligne ne démarre pas — la file est encore bouchée");
    app.transfers.stop();
}

#[tokio::test]
async fn annuler_en_vol_rend_la_place() {
    // le canal dort 5 s par part : de quoi couper une ligne en plein vol
    let (app, site, parc, travaux, _garde) = poste("envol", 30, 5).await;
    app.enqueue_upload(&site, &[travaux.join("bord.txt").to_string_lossy().to_string()], "/", false)
        .await
        .expect("file");

    let id = 'trouve: {
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if let Some(j) = app.transfers.snapshot().first() {
                if j.state == JobState::Active || j.state == JobState::Done {
                    break 'trouve j.id.clone();
                }
            }
        }
        panic!("la ligne n'a pas démarré");
    };
    assert!(app.transfers.cancel(&id), "l'annulation n'a pas été entendue");

    // l'état passe à « annulé » sur-le-champ, mais la tâche tient le canal
    // jusqu'à la part en cours : c'est la place rendue au parc qu'on surveille
    let mut rendu = false;
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let j = app.transfers.job(&id).expect("ligne toujours là");
        assert_ne!(j.state, JobState::Error, "la ligne a basculé en erreur au lieu d'être annulée : {}", j.error);
        if parc.free_count() == 1 {
            rendu = true;
            break;
        }
    }
    assert!(rendu, "la tâche n'a jamais rendu la connexion : {:?}", app.transfers.job(&id).map(|j| (j.state.clone(), j.error.clone())));
    assert_eq!(app.transfers.stats().active, 0);
    assert_eq!(app.transfers.job(&id).map(|j| j.state), Some(JobState::Cancelled), "la ligne n'a pas la bonne fin");
    app.transfers.stop();
}

