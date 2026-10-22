# Zilla-RS

**Un clone de FileZilla qui passe entièrement par le navigateur — écrit en Rust, servi par un seul binaire.**

Zilla-RS est un client de transfert autonome et sans dépendance : un seul processus Rust
héberge toute l'application (API HTTP, mises à jour temps réel WebSocket, moteur de transferts,
pile de protocoles FTP/FTPS/SFTP) et distribue une interface web maison. Aucun CDN, aucun Node,
aucun bundleur, aucun Electron : ouvrez `http://127.0.0.1:8230` et transférez.

## Pourquoi ce projet

FileZilla est un très bon outil, mais c'est une interface graphique de bureau. Zilla-RS en
garde exactement les *reflexes* — navigation deux panneaux, gestionnaire de sites, file de
transferts, journal que l'on lit vraiment — tout en étant :

- **Sans écran.** Sur un serveur, un conteneur, un VPS : l'interface n'est qu'une page web.
- **Pilotable.** Tout ce que fait l'interface est un point de terminaison HTTP JSON.
- ** Léger.** Un binaire statique, quelques dizaines de Mio de RAM, zéro dépendance runtime.
- **Compatible.** Import/export du `sitemanager.xml` de FileZilla sans le abîmer.

## Fonctionnalités

**Protocoles**
- FTP (modes passif `PASV`/`EPSV` et actif `PORT`/`EPRT`)
- FTP sur TLS explicite (`AUTH TLS` + `PBSZ`/`PROT P`) et FTPS implicite
- SFTP (SSH-2) via `russh`, avec authentification par mot de passe ou clé
- MLSD/MLST avec repli LIST (formats Unix et DOS), `SIZE`, `MDTM`, `UTF8`, `REST`

**Deux panneaux**
- Panneau local : vrai fichier local, fil d'Ariane, favoris, arbre, recherche
- Panneau distant : mêmes fonctions, plus connexion/deconnexion et keepalive
- Tri par colonnes, filtres, fichiers cachés, sélection multiple, menus contextuels, glisser-déposer
- Clavier : F2 renommer, F5 actualiser, Ctrl+K palette de commandes, Alt+flèches naviguer

**Moteur de transferts**
- Transferts parallèles avec un pool de connexions par site
- Priorités, limitation de bande passante, pause/reprise/annulation/essai à nouveau
- Reprise (`REST`), stratégies écraser/passer/plus récent/rebaptiser, préservation des dates
- Récursion de dossiers, persistance de la file, notification de fin de file

**Gestionnaire de sites**
- Groupes imbriqués, réglages par site (délais, commandes, niveau TLS, mode d'identification)
- Import/export XML compatible FileZilla
- Connexion rapide, sites récents, favoris

**Réglages et journal**
- Neuf onglets de réglages appliqués à chaud via WebSocket
- Catégories de journal (`general`, `command`, `status`, `transfer` …), horodatage, sauvegarde
- Thèmes (sombre/clair), police, taille des panneaux — sauvegardés par navigateur

**Auto-tests**
- 16 contrôles de bout en bout contre un serveur FTP de démonstration, dans l'UI ou en ligne de commande

## Démarrage rapide

```sh
git clone https://github.com/zagg/zilla-rs.git
cd zilla-rs
cargo run --release            # http://127.0.0.1:8230
```

Pour cliquer tout de suite, le mode démonstration installe un serveur FTP factice et des fichiers :

```sh
cargo run --release -- demo     # FTP de démo sur 127.0.0.1:2121, UI sur 8230
```

## Ligne de commande

```
zilla serve   [-c config] [--port N] [--host IP] [--no-browser]   démarre l'application web
zilla demo    [-c config] [--port N]                              serveur FTP de démo + UI
zilla selftest [-c config]                                        lance les 16 contrôles
zilla import  <sitemanager.xml>                                   importe les sites FileZilla
zilla export  <sitemanager.xml>                                   exporte en XML FileZilla
zilla version
```

La configuration est un fichier JSON unique par dossier `--config` (par défaut
`~/.config/zilla-rs/zilla.json`) qui contient aussi les sites, les récents, les favoris et la file.

## API HTTP

Tous les points sont sous `/api`. Le serveur publie un jeton par session et l'injecte dans la
page ; les appels qui modifient l'état doivent le renvoyer dans `x-zilla-token`.

```sh
JETON=$(curl -s localhost:8230/api/status | jq -r .data.token)

# connexion rapide en SFTP
curl -s -H "content-type: application/json" -H "x-zilla-token: $JETON" \
  -d '{"host":"exemple.fr","port":22,"proto":"sftp","user":"bob","password":"secret"}' \
  localhost:8230/api/connect

# lister un répertoire
curl -s -H "x-zilla-token: $JETON" \
  "localhost:8230/api/remote/list?site=<idSite>&path=/var"

# demander un téléchargement récursif
curl -s -H "content-type: application/json" -H "x-zilla-token: $JETON" \
  -d '{"site":"<idSite>","paths":["/var/log"],"dir":"/tmp/sauvegarde","dirs":true}' \
  localhost:8230/api/download
```

`/_ws` diffuse les évènements (`log`, `queue`, `connections`, `toast`) pour rafraîchir l'interface.
Contrôle de santé : `GET /api/selftest`.

## Notes de rétro-ingénierie

Le comportement des protocoles, les formulations du journal, le format de `sitemanager.xml` et
les raccourcis clavier ont été déduits de FileZilla 3.68 (chaînes et ressources livrées) avec
une démarche de rétro-ingénierie — l'implémentation est du Rust original, aucun code FileZilla
n'est utilisé ni compilé. Voir `docs/REA.md`.

## Sécurité

- Le serveur écoute sur `127.0.0.1` par défaut ; `--host` permet de l'exposer, et dans ce cas
  le garde-fou à jeton est la seule authentification. Pas de mode multi-utilisateur.
- Les mots de passe des sites sont stockés en clair dans le fichier de configuration, comme dans
  le XML de FileZilla : protégez ce fichier (`chmod 600`).
- Le TLS s'appuie sur `rustls` avec le magasin racine Mozilla (`webpki-roots`). La validation
  des certificats est forcée, il n'y a pas d'option « accepter n'importe quel certificat ».
- Ne pas exposer le panneau local sur un réseau non fiable : il lit et écrit de vrais fichiers.

## Tests

```sh
cargo test --lib          # 20 tests unitaires (listages, chemins, identifiants)
cargo test                # + tests d'intégration (démo, pools, file de transferts)
cargo build --release
./target/release/zilla selftest
```

## Licence

MIT.
