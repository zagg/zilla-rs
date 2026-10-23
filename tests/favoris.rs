//! Le favori est une case nouvelle : une fiche plus ancienne doit se lire
//! sans lui, sinon le gestionnaire de sites refuserait d'ouvrir le moindre
//! site après la mise à jour.

use zilla_rs::Site;

#[test]
fn une_fiche_sans_favori_se_quand_meme() {
    let s: Site = serde_json::from_str(
        r#"{"id":"a","name":"atelier","host":"ftp.example.org","proto":"ftp"}"#,
    )
    .unwrap();
    assert!(!s.favorite, "un site qu'on n'a jamais étoilé n'est pas favori");

    let s = Site {
        favorite: true,
        ..Site::default()
    };
    let x = serde_json::to_string(&s).unwrap();
    assert!(x.contains("\"favorite\":true"), "{x}");
}
