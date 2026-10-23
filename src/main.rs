//! Zilla-RS : client FTP/FTPS/SFTP web, clone autonome de FileZilla.
//!
//! ```text
//! zilla serve    → interface web sur http://<hote>:8734 (écoute 0.0.0.0 par défaut)
//! zilla selftest → auto-tests client complet
//! zilla import   → importe un sitemanager.xml de FileZilla
//! zilla export   → exporte le gestionnaire de sites en XML
//! ```

use std::io::Write;
use std::path::PathBuf;
use tokio::net::TcpListener;
use zilla_rs::api;
use zilla_rs::app::App;
use zilla_rs::util::default_config_dir;

const VERSION: &str = zilla_rs::VERSION;

struct Flags {
    bind: String,
    port: u16,
    config: PathBuf,
    quiet: bool,
}

fn parse_flags(args: &[String]) -> Flags {
    let mut f = Flags {
        bind: "0.0.0.0".into(),
        port: 8734,
        config: default_config_dir(),
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
            "--bind" | "-b" | "--host" => f.bind = get(&mut i),
            "--port" | "-p" => f.port = get(&mut i).parse().unwrap_or(8734),
            "--config" | "--config-dir" | "-c" => f.config = PathBuf::from(get(&mut i)),
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
    // Les options sont lues dans la ligne de commande entiere : `zilla serve -p 9`
    // comme `zilla -p 9` (le nom de sous-commande ne gene pas le parseur).
    let flags = parse_flags(&args);
    match cmd.as_str() {
        "serve" | "run" | "web" => serve(&flags).await,
        "selftest" | "test" => selftest().await,
        "import" => import(&args, &flags),
        "export" => export(&flags),
        "version" | "--version" | "-V" => println!("Zilla-RS {VERSION}"),
        "help" | "--help" | "-h" => usage(),
        other => {
            // Peut-être un simple `zilla --port 1234`.
            if other.starts_with('-') {
                serve(&flags).await;
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
  zilla selftest                enchaîne les auto-tests client
  zilla import <fichier.xml>    importe un sitemanager.xml de FileZilla
  zilla export                  écrit ./sitemanager.xml depuis vos sites

\x1b[4mOptions :\x1b[0m
  -b, --bind <ip>        adresse d'écoute (défaut : 0.0.0.0, toutes les interfaces)
  -p, --port <port>      port d'écoute (défaut : 8734)
  -c, --config <dossier> dossier de réglages (défaut : {})
  -q, --quiet            n'affiche pas d'en-tête
",
        default_config_dir().display()
    );
}

async fn serve(flags: &Flags) {
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
    let router = api::router(app.clone());
    let listener = match TcpListener::bind((flags.bind.as_str(), flags.port)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("échec d'écoute sur {}:{} — {e}", flags.bind, flags.port);
            std::process::exit(1);
        }
    };
    let actual = listener.local_addr().map(|a| a.port()).unwrap_or(flags.port);
    if !flags.quiet {
        banner(&flags.bind, actual, &flags.config);
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

fn banner(bind: &str, port: u16, config: &std::path::Path) {
    let exposed = bind == "0.0.0.0" || bind == "::" || bind.is_empty();
    let lan = outbound_ip().map(|ip| ip.to_string());
    let shown = if exposed {
        lan.clone().unwrap_or_else(|| "127.0.0.1".to_string())
    } else {
        bind.to_string()
    };
    let bar = "═".repeat(58);
    println!("  ╔{bar}╗");
    println!("  ║  \x1b[1mZilla-RS {v}\x1b[0m — votre FileZilla, désormais dans le navigateur            ║",
        v = crate_version_padded());
    println!("  ╚{bar}╝");
    if exposed {
        println!(
            "   Interface   \x1b[36mhttp://{}:{}\x1b[0m  \x1b[2m← accessible depuis n'importe quelle machine du réseau\x1b[0m",
            shown, port
        );
        println!("   Local       http://127.0.0.1:{port}  \x1b[2mdepuis ce poste\x1b[0m");
        println!(
            "   Écoute      \x1b[33m{}:{}\x1b[0m  \x1b[2m(toutes les interfaces, aucun mot de passe)\x1b[0m",
            bind, port
        );
    } else {
        println!("   Interface   \x1b[36mhttp://{}:{}\x1b[0m", shown, port);
        println!("   Écoute      {}:{}", bind, port);
    }
    println!("   Réglages    {}", config.display());
    println!("   \x1b[2mCtrl+C pour arrêter proprement — aucune connexion ne sort de cette machine sans votre ordre.\x1b[0m\n");
    let _ = std::io::stdout().flush();
}

/// Adresse que cette machine utiliserait pour joindre l'extérieur : c'est
/// l'adresse à laquelle le réseau local la voit. Un simple regard de routage,
/// aucun paquet n'est envoyé.
fn outbound_ip() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect(("10.255.255.255", 53)).ok()?;
    sock.local_addr().ok().map(|a| a.ip())
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

