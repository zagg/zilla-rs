//! Zilla-RS : client FTP/FTPS/SFTP web, clone autonome de FileZilla.
//!
//! ```text
//! zilla serve    → interface web sur http://127.0.0.1:8734
//! zilla demo     → interface + mini-serveur FTP de démonstration
//! zilla selftest → auto-tests client complet
//! zilla import   → importe un sitemanager.xml de FileZilla
//! zilla export   → exporte le gestionnaire de sites en XML
//! ```

use std::io::Write;
use std::path::PathBuf;
use tokio::net::TcpListener;
use zilla_rs::api;
use zilla_rs::app::App;
use zilla_rs::demosrv::DemoFtp;
use zilla_rs::util::default_config_dir;

const VERSION: &str = zilla_rs::VERSION;

struct Flags {
    bind: String,
    port: u16,
    config: PathBuf,
    root: PathBuf,
    quiet: bool,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut f = Flags {
        bind: "127.0.0.1".into(),
        port: 8734,
        config: default_config_dir(),
        root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        quiet: false,
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let get = |i: &mut usize| {
            *i += 1;
            args.get(*i).cloned().unwrap_or_default()
        };
        match a {
            "--bind" | "-b" => f.bind = get(&mut i),
            "--port" | "-p" => f.port = get(&mut i).parse().unwrap_or(8734),
            "--config" | "--config-dir" | "-c" => f.config = PathBuf::from(get(&mut i)),
            "--root" => f.root = PathBuf::from(get(&mut i)),
            "--quiet" | "-q" => f.quiet = true,
            _ => {}
        }
        i += 1;
    }
    f
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().cloned().unwrap_or_else(|| "serve".into());
    let flags = parse_flags(&args[1.min(args.len())..]);
    match cmd.as_str() {
        "serve" | "run" | "web" => serve(&flags, false).await,
        "demo" => serve(&flags, true).await,
        "selftest" | "test" => selftest().await,
        "import" => import(&args, &flags),
        "export" => export(&flags),
        "version" | "--version" | "-V" => println!("Zilla-RS {VERSION}"),
        "help" | "--help" | "-h" => usage(),
        other => {
            // Peut-être un simple `zilla --port 1234`.
            if other.starts_with('-') {
                serve(&flags, false).await;
            } else {
                eprintln!("sous-commande inconnue : {other}\n");
                usage();
                std::process::exit(2);
            }
        }
    }
}

fn usage() {
    println!(
        "\x1b[1mZilla-RS {VERSION}\x1b[0m — client FTP/FTPS/SFTP web, clone autonome de FileZilla

\x1b[4mUsage :\x1b[0m
  zilla serve    [options]      démarre l'interface web
  zilla demo     [options]      interface + mini-serveur FTP local
  zilla selftest                enchaîne les auto-tests client
  zilla import <fichier.xml>    importe un sitemanager.xml de FileZilla
  zilla export                  écrit ./sitemanager.xml depuis vos sites

\x1b[4mOptions :\x1b[0m
  -b, --bind <ip>        adresse d'écoute (défaut : 127.0.0.1)
  -p, --port <port>      port d'écoute (défaut : 8734)
  -c, --config <dossier> dossier de réglages (défaut : {})
      --root <dossier>   racine du serveur de démonstration
  -q, --quiet            n'affiche pas d'en-tête
",
        default_config_dir().display()
    );
}

async fn serve(flags: &Flags, demo: bool) {
    if let Err(e) = std::fs::create_dir_all(&flags.config) {
        eprintln!("dossier de configuration inaccessible : {e}");
        std::process::exit(1);
    }
    let app = match App::new(flags.config.clone(), VERSION) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("erreur de démarrage : {e}");
            std::process::exit(1);
        }
    };
    let public = flags.bind != "127.0.0.1" && flags.bind != "localhost";
    let router = api::router(app.clone(), public);
    let listener = match TcpListener::bind((flags.bind.as_str(), flags.port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("échec d'écoute sur {}:{} — {e}", flags.bind, flags.port);
            std::process::exit(1);
        }
    };
    let actual = listener.local_addr().map(|a| a.port()).unwrap_or(flags.port);
    if !flags.quiet {
        banner(&flags.bind, actual, &app.token, &flags.config, demo);
    }
    if demo {
        let root = if flags.root.join("demo").is_dir() {
            flags.root.join("demo")
        } else {
            let d = flags.config.join("demo");
            std::fs::create_dir_all(&d).ok();
            seed_demo_files(&d);
            d
        };
        match DemoFtp::start(&root).await {
            Ok(d) => {
                if !flags.quiet {
                    println!(
                        "  Serveur de démo : {} (dossier : {})",
                        d.url(),
                        root.display()
                    );
                }
                app.hub.info(
                    "general",
                    format!("Mini-serveur FTP de démonstration démarré sur {}", d.url()),
                );
                std::mem::forget(d);
            }
            Err(e) => {
                app.hub.err("general", format!("Serveur de démo indisponible : {e}"));
            }
        }
    }
    let app2 = app.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            app2.shutdown();
        })
        .await
        .unwrap_or_else(|e| {
            eprintln!("serveur web arrêté : {e}");
            app.shutdown();
            std::process::exit(1);
        });
}

fn banner(bind: &str, port: u16, token: &str, config: &std::path::Path, demo: bool) {
    let host = if bind == "0.0.0.0" { "127.0.0.1" } else { bind };
    let bar = "═".repeat(58);
    println!("  ╔{bar}╗");
    println!("  ║  \x1b[1mZilla-RS {v}\x1b[0m — votre FileZilla, désormais dans le navigateur{d}  ║",
        v = crate_version_padded(),
        d = if demo { " + démo     " } else { "            " });
    println!("  ╚{bar}╝");
    println!("   Interface   \x1b[36mhttp://{}:{}\x1b[0m", host, port);
    println!("   Réglages    {}", config.display());
    if bind != "127.0.0.1" {
        println!("   \x1b[33mJeton\x1b[0m (en-tête \x1b[2mX-Zilla-Token\x1b[0m) : {}", token);
    }
    println!("   \x1b[2mCtrl+C pour arrêter proprement — aucune connexion ne sort de cette machine sans votre ordre.\x1b[0m\n");
    let _ = std::io::stdout().flush();
}

fn crate_version_padded() -> String {
    let v = format!("v{VERSION}");
    let n = v.chars().count();
    if n <= 7 {
        v
    } else {
        v.chars().take(7).collect()
    }
}

async fn selftest() {
    println!("Auto-tests Zilla-RS {VERSION}…");
    let r = zilla_rs::selftest::run(VERSION).await;
    if let Some(checks) = r.get("checks") {
        for c in checks.as_array().unwrap_or(&vec![]) {
            let ok = c["ok"].as_bool().unwrap_or(false);
            let mark = if ok { "\x1b[32m✓\x1b[0m" } else { "\x1b[31m✗\x1b[0m" };
            println!(
                "  {} {:<32} {}",
                mark,
                c["name"].as_str().unwrap_or("?"),
                if ok {
                    String::new()
                } else {
                    format!("→ {}", c["note"].as_str().unwrap_or(""))
                }
            );
        }
    }
    let ok = r["ok"].as_bool().unwrap_or(false);
    println!(
        "\n  {} — {} contrôles, {} échec(s), {} ms\n",
        if ok {
            "\x1b[32mtous les tests sont verts\x1b[0m"
        } else {
            "\x1b[31échec partiel ou totalex1b[0m"
        },
        r["passes"].as_u64().unwrap_or(0),
        r["failures"].as_u64().unwrap_or(0),
        r["ms"].as_u64().unwrap_or(0)
    );
    if !ok {
        println!("{:#}", r);
        std::process::exit(1);
    }
}

fn import(args: &[String], flags: &Flags) {
    let path = args.iter().find(|a| !a.starts_with('-')).cloned();
    let Some(p) = path else {
        eprintln!("il faut le chemin du fichier XML");
        std::process::exit(2);
    };
    let xml = match std::fs::read_to_string(&p) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lecture impossible de {p} : {e}");
            std::process::exit(1);
        }
    };
    match zilla_rs::store::import_sitemanager_xml(&xml) {
        Ok(list) => {
            let store = zilla_rs::store::Store::load(&flags.config);
            let mut store = match store {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("réglages illisibles : {e}");
                    std::process::exit(1);
                }
            };
            let mut n = 0;
            for mut site in list {
                if let Some(old) = store
                    .sites
                    .iter()
                    .find(|x| x.host == site.host && x.name == site.name)
                {
                    site.id = old.id.clone();
                }
                store.upsert_site(site);
                n += 1;
            }
            println!("  {} site(s) importé(s) dans {}", n, flags.config.display());
        }
        Err(e) => {
            eprintln!("import refusé : {e}");
            std::process::exit(1);
        }
    }
}

fn export(flags: &Flags) {
    let store = match zilla_rs::store::Store::load(&flags.config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("lecture des sites : {e}");
            std::process::exit(1);
        }
    };
    let xml = zilla_rs::store::export_sitemanager_xml(&store.sites);
    let out = std::env::current_dir().unwrap_or_default().join("sitemanager.xml");
    if let Err(e) = std::fs::write(&out, xml) {
        eprintln!("écriture impossible : {e}");
        std::process::exit(1);
    }
    println!(
        "  {} site(s) écrit(s) dans {}",
        store.sites.len(),
        out.display()
    );
}

fn seed_demo_files(dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dir.join("docs"));
    if dir.join("bienvenue.txt").exists() {
        return;
    }
    let files: [(&str, &[u8]); 6] = [
        (
            "bienvenue.txt",
            "Bienvenue dans la zone de démonstration de Zilla-RS.\n\nTéléchargez, envoyez, renommez : tout est permis.".as_bytes(),
        ),
        ("notes.md", "# Notes\n\n- fichier texte\n- deux listes\n".as_bytes()),
        (
            "docs/liseuse.txt",
            "document de démonstration dans un sous-dossier\n".as_bytes(),
        ),
        ("docs/rapport.csv", "id,nom,note\n1,alice,18\n2,bruno,15\n".as_bytes()),
        (
            "photos.svg",
            "<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'><rect width='96' height='96' rx='16' fill='#22d3ee'/><text x='48' y='60' font-size='42' text-anchor='middle' fill='#001017'>Z</text></svg>".as_bytes(),
        ),
        ("grosse-bille.bin", &[7u8; 200_000][..]),
    ];
    for (n, c) in files {
        let _ = std::fs::write(dir.join(n), c);
    }
}
