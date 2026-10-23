// Le navigateur compare `e.kind === "dir"` depuis le premier jour : un genre
// écrit « Dir » lui fait voir un fichier, et le double-clic ne fait plus que
// transférer (ou rien du tout) au lieu de entrer dans le dossier.
use zilla_rs::models::{Entry, EntryKind};

fn json_of(e: &Entry) -> String {
    serde_json::to_string(e).unwrap()
}

#[test]
fn genres_en_minuscules_sur_le_reseau() {
    assert!(json_of(&Entry::dir("web".into(), None)).contains("\"kind\":\"dir\""));
    assert!(json_of(&Entry::file("u.txt".into(), 1, None)).contains("\"kind\":\"file\""));
    let l = Entry { kind: EntryKind::Symlink, ..Entry::dir("link".into(), None) };
    assert!(json_of(&l).contains("\"kind\":\"symlink\""));
    let o = Entry { kind: EntryKind::Other, ..Entry::dir("dev".into(), None) };
    assert!(json_of(&o).contains("\"kind\":\"other\""));
}

#[test]
fn la_casse_d_autrefois_reste_acceptee_en_lecture() {
    // réglages, caches ou clients écrits par les versions antérieures
    for (old, want) in [
        ("\"Dir\"", EntryKind::Dir),
        ("\"File\"", EntryKind::File),
        ("\"Symlink\"", EntryKind::Symlink),
        ("\"dir\"", EntryKind::Dir),
    ] {
        let got: EntryKind = serde_json::from_str(old).unwrap();
        assert_eq!(got, want, "{old}");
    }
}

#[test]
fn une_entree_reroutee_en_dossier_se_repertorie() {
    let mut d = Entry::dir("src".into(), None);
    d.mode = Some(0o755);
    assert!(d.is_dir());
    assert_eq!(d.perm_str().chars().next().unwrap(), 'd');
    assert_eq!(d.perm_str().len(), 10, "{:?}", d.perm_str());
}
