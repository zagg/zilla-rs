# Zilla-RS

**Un clone de FileZilla qui passe entièrement par le navigateur — écrit en Rust, servi par un seul binaire.**

Zilla-RS est un client de transfert autonome et sans dépendance : un seul processus Rust
héberge toute l'application (API HTTP, mises à jour temps réel WebSocket, moteur de transferts,
pile de protocoles FTP/FTPS/SFTP) et distribue une interface web maison. Aucun CDN, aucun Node,
aucun bundleur, aucun Electron : ouvrez `http://<cette-machine>:8734` et transférez.

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
- Reprise (`REST`), stratégies écraser/passer/plus récent/rebaptiser, préservation des dates (`MDTM`/`MFMT`)
- Récursion de dossiers, persistance de la file, notification de fin de file
- Les deux panneaux sont prévenus dès qu'une ligne se termine : le dossier qui a
  reçu ou lâché un fichier est redemandé sur-le-champ, sans attendre la sonde
- Une ligne terminée s'efface de la file d'attente ; le décompte reste au bandeau
  et un clic la rend pour la relire
- La file tient dans le volet du bas, sous l'onglet « Transferts », et plus entre
  les deux panneaux de fichiers — les deux panneaux gardent toute la largeur

**Gestionnaire de sites**
- Groupes imbriqués, réglages par site (délais, commandes, niveau TLS, mode d'identification)
- Au branchement, le panneau distant s'ouvre sur le répertoire demandé par le site
  (`/` laisse le serveur choisir) et le panneau local sur son `Répertoire local` :
  c'est là que les téléchargements atterrissent
- Import/export XML compatible FileZilla
- Connexion rapide, sites récents, favoris

**Réglages et journal**
- Neuf onglets de réglages appliqués à chaud via WebSocket
- Catégories de journal (`general`, `command`, `status`, `transfer` …), horodatage, sauvegarde
- Thèmes (sombre/clair), police, taille des panneaux — sauvegardés par navigateur

**Comparaison de dossiers**
- `GET /api/compare?site=…&local=…&remote=…` parcourt les deux arborescences et rend `same`,
  `different`, `local_only`, `remote_only`, plus les octets à descendre ou à monter
- Dialogue « Comparaison des dossiers » (Ctrl+Maj+C) : on coche les écarts, puis on les envoie
  d'un côté ou de l'autre en gardant la structure (`base`)

**Auto-tests**
- 16 contrôles de bout en bout contre un serveur FTP ephémère lancé par le test, dans l'UI ou en ligne de commande

## Démarrage rapide

```sh
git clone https://github.com/zagg/zilla-rs.git
cd zilla-rs
cargo run --release            # écoute sur 0.0.0.0:8734 — ouvrez http://<cette-machine>:8734
```

## Ligne de commande

```
zilla serve   [-c dir] [-b ip] [-p n] [-q]                        démarre l'application web
zilla selftest [-c config]                                        lance les 16 contrôles
zilla import  <sitemanager.xml>                                   importe les sites FileZilla
zilla export  <sitemanager.xml>                                   exporte en XML FileZilla
zilla version
```

`-b/--bind` (ou `--host`) vaut `0.0.0.0` par defaut, soit toutes les interfaces : l'interface
est atteignable depuis le reseau local des le demarrage, et `http://127.0.0.1:8734` fonctionne
toujours depuis la machine. Utilisez `-b 127.0.0.1` pour la limiter au poste.

La configuration est un fichier JSON unique par dossier `--config` (par défaut
`~/.config/zilla-rs/zilla.json`) qui contient aussi les sites, les récents, les favoris et la file.

## API HTTP

Tous les points sont sous `/api`. Aucun authentifiant, aucune session : c'est un outil de
reseau local, quiconque atteint le port peut piloter l'application.

```sh
# connexion rapide en SFTP
curl -s -H "content-type: application/json" \
  -d '{"host":"exemple.fr","port":22,"proto":"sftp","user":"bob","password":"secret"}' \
  localhost:8734/api/connect

# lister un répertoire
curl -s "localhost:8734/api/remote/list?site=<idSite>&path=/var"

# demander un téléchargement récursif
curl -s -H "content-type: application/json" \
  -d '{"site":"<idSite>","paths":["/var/log"],"dir":"/tmp/sauvegarde","dirs":true}' \
  localhost:8734/api/download

# en gardant l'arborescence : tout ce qui est sous `base` est recopié a l'identique
curl -s -H "content-type: application/json" \
  -d '{"site":"<idSite>","paths":["/srv/www/index.html"],"dir":"/tmp/sauvegarde","dirs":true,"base":"/srv/www"}' \
  localhost:8734/api/download
```

`paths` accepte des chemins absolus, ou relatifs quand `base` est fourni — c'est exactement
la façon dont le dialogue de comparaison nourrit la file.

`/_ws` diffuse les évènements (`log`, `queue`, `connections`, `toast`) pour rafraîchir l'interface.
Contrôle de santé : `GET /api/selftest`.

## Notes de rétro-ingénierie

Le comportement des protocoles, les formulations du journal, le format de `sitemanager.xml` et
les raccourcis clavier ont été déduits de FileZilla 3.68 (chaînes et ressources livrées) avec
une démarche de rétro-ingénierie — l'implémentation est du Rust original, aucun code FileZilla
n'est utilisé ni compilé. Voir `docs/REA.md`.

## Sécurité

- Le serveur écoute sur `0.0.0.0:8734` par defaut : tout le reseau peut ouvrir l'interface et
  la piloter. Il n'y a aucune authentification, aucun mode multi-utilisateur — gardez-le sur un
  reseau de confiance, ou revenez a l'ecoute locale avec `--bind 127.0.0.1`.
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
