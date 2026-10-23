//! Ce que la file montre et ce qu'elle laisse derrière elle.
//!
//! La page compare `t.state === "done"` depuis le premier jour : une ligne
//! écrite « Done » ne quitte jamais la liste d'attente, et son état là-bas
//! s'affiche « en attente » pour l'éternité — le raté que les gens appellent
//! « mes téléchargements restent coincés ». Le contrat se tient donc ici,
//! comme pour les genres d'entrée (`kinds.rs`) : minuscule sur le fil, les
//! deux casse acceptées à la lecture.

use std::sync::Arc;
use zilla_rs::hub::LogHub;
use zilla_rs::models::{Direction, JobState, Priority, TransferJob};
use zilla_rs::transfers::Transfers;

fn etat(e: JobState) -> String {
    serde_json::to_string(&e).unwrap()
}

fn ligne(e: JobState) -> TransferJob {
    let mut j = TransferJob::new(
        "s1".into(),
        "démo".into(),
        Direction::Down,
        "/atelier/fiche.txt".into(),
        "/root/fiche.txt".into(),
        1024,
    );
    j.state = e;
    j
}

fn file(kb: usize) -> Arc<Transfers> {
    Transfers::new(Arc::new(LogHub::new(200)), kb)
}

#[test]
fn etats_en_minuscules_sur_le_reseau() {
    assert_eq!(etat(JobState::Waiting), "\"waiting\"");
    assert_eq!(etat(JobState::Active), "\"active\"");
    assert_eq!(etat(JobState::Done), "\"done\"");
    assert_eq!(etat(JobState::Error), "\"error\"");
    assert_eq!(etat(JobState::Cancelled), "\"cancelled\"");
    assert_eq!(etat(JobState::Paused), "\"paused\"");
    // le sens du voyage, lui aussi, se compare en minuscules côté page
    assert_eq!(
        serde_json::to_string(&Direction::Down).unwrap(),
        "\"down\""
    );
    assert_eq!(serde_json::to_string(&Priority::High).unwrap(), "\"high\"");
}

#[test]
fn une_ligne_finie_sy_lit_finie() {
    let s = serde_json::to_string(&ligne(JobState::Done)).unwrap();
    assert!(s.contains("\"state\":\"done\""), "{s}");
    assert!(s.contains("\"direction\":\"down\""), "{s}");
    assert!(s.contains("\"priority\":\"normal\""), "{s}");
}

#[test]
fn la_casse_d_autrefois_reste_acceptee_en_lecture() {
    // `transferqueue.json` dort sur le disque : ce qu'y ont écrit les versions
    // antérieures doit rester lisible, faute de quoi la file reprendrait des
    // lignes en « ? » au premier démarrage d'après mise à jour.
    for (old, want) in [
        ("\"Done\"", JobState::Done),
        ("\"done\"", JobState::Done),
        ("\"Waiting\"", JobState::Waiting),
        ("\"paused\"", JobState::Paused),
    ] {
        assert_eq!(serde_json::from_str::<JobState>(old).unwrap(), want, "{old}");
    }
    assert_eq!(
        serde_json::from_str::<Direction>("\"Down\"").unwrap(),
        Direction::Down
    );
    assert_eq!(
        serde_json::from_str::<Direction>("\"up\"").unwrap(),
        Direction::Up
    );
    assert_eq!(
        serde_json::from_str::<Priority>("\"High\"").unwrap(),
        Priority::High
    );
    assert_eq!(
        serde_json::from_str::<Priority>("\"low\"").unwrap(),
        Priority::Low
    );
}

#[test]
fn vider_la_file_n_y_laisse_rien() {
    let q = file(8);
    q.add_many(vec![
        ligne(JobState::Waiting),
        ligne(JobState::Active),
        ligne(JobState::Done),
    ]);
    assert_eq!(q.stats().pending, 1);
    // le bouton compte ce qu'il retire : la page l'affiche, le doute n'a pas lieu d'être
    assert_eq!(q.clear_all(), 3);
    assert!(q.snapshot().is_empty());
    let s = q.stats();
    assert_eq!(
        (s.pending, s.active, s.done, s.failed),
        (0, 0, 0, 0),
        "une file vidée doit le dire"
    );
}

#[test]
fn une_ligne_effacee_ne_repousse_pas_quand_sa_part_passe() {
    let q = file(8);
    let job = ligne(JobState::Active);
    let id = job.id.clone();
    q.add(job);
    q.clear_all();
    // la part d'octets suivante aboutit après la purge : la ligne ne doit pas
    // revenir hanter la liste comme si de rien n'était
    q.fail(&id, "retard à l'appel");
    assert!(q.snapshot().is_empty(), "aucune ligne après la purge");
    assert_eq!(q.stats().failed, 0);
}
