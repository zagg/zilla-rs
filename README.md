# Zilla-RS

**A FileZilla look-alike FTP/FTPS/SFTP client that runs entirely in your browser — written in Rust, served by a single small binary.**

Zilla-RS is a self-contained, autonomous transfer client: one Rust process hosts the whole
application (HTTP API, WebSocket live updates, transfer engine, FTP/FTPS/SFTP protocol stack)
and serves a dependency-free web UI. No CDN, no Node, no bundling, no Electron, no external
assets: open `http://<this-host>:8734` and start transferring.

## Why this exists

FileZilla is a fine tool, but it is a desktop GUI. Zilla-RS keeps the exact *feel* of
FileZilla — dual-pane browsing, site manager, transfer queue, the log you actually read — while
being:

- **Headless-first.** Runs on a server, in a container, on a VPS; the UI is just a web page.
- **Scriptable.** Everything the UI does is a plain JSON HTTP endpoint.
- **Lightweight.** One static binary, ~10 MB of RAM, no runtime dependencies.
- **Compatible.** Imports and exports FileZilla's `sitemanager.xml` without mangling it.

## Features

**Protocols**
- FTP (passive `PASV`/`EPSV` and active `PORT`/`EPRT` modes)
- FTP over explicit TLS (`AUTH TLS` + `PBSZ`/`PROT P`) and implicit FTPS
- SFTP (SSH-2) via `russh`, with password and key authentication
- MLSD/MLST with LIST fallback (Unix and DOS listings), `SIZE`, `MDTM`, `UTF8`, `REST`

**Dual-pane browser**
- Local pane with real filesystem access, breadcrumbs, bookmarks, tree view, search
- Remote pane with the same affordances, plus connect/disconnect and keepalive
- Column sorting, filters, hidden-file toggle, multi-select, context menus, drag & drop
- Keyboard-first: F2 rename, F5 refresh, Ctrl+K command palette, Alt+arrows navigation

**Transfer engine**
- Parallel transfers with per-site connection pools
- Priorities, bandwidth throttling, pause/resume/cancel/retry
- Resume (`REST`), overwrite/skip/newer/rename policies, mtime preservation (`MDTM`/`MFMT`)
- Directory recursion, queue persistence across restarts, completion notification
- Both panes are told the moment a row finishes: the folder that received (or
  lost) a file is re-listed straight away, without waiting for the page's poll
- A finished row leaves the waiting list; the tally stays on the strip and one
  click brings the folded rows back
- One 🧹 empties the queue outright: no row left behind, and the transfers that
  were running hand their slot back at the next chunk
- The queue lives in the bottom strip under the "Transferts" tab instead of
  between the two file panes, so both panes keep the full width

**Site manager**
- Nested groups, per-site overrides (timeouts, commands, TLS level, logintype)
- On connect, the remote pane opens on the directory the site asks for (`/`
  lets the server choose) and the local pane on its `Local directory`, which is
  also where downloads land
- FileZilla-compatible XML import/export
- Quick connect, recents, bookmarks
- A site flagged as favourite — the star in the tree, or the "favourite" box on
  its sheet — hangs off the top bar as a button: one click to connect, one to
  get out, and the dot on the button says whether a wire is open
- "Se connecter" greys out while a connection holds the place, "Déconnecter"
  greys out when none does: the state reads off the buttons, not off the log

**Settings & logging**
- Nine grouped settings tabs, applied live over WebSocket
- Log categories (`general`, `command`, `status`, `transfer`, …), timestamps, save to file
- Themes (dark/light), font, pane layout — persisted per browser

**Folder comparison**
- `GET /api/compare?site=…&local=…&remote=…` walks both trees and reports `same`, `different`,
  `local_only`, `remote_only`, plus bytes to download/upload
- Drives the "Compare folders" dialog (Ctrl+Shift+C): pick the differences, then queue them
  in one direction while keeping the tree structure (`base` parameter)

**Selftest**
- 16 end-to-end checks run against a throwaway FTP server the checks start themselves, on the
  UI or from the CLI

## Quick start

```sh
git clone https://github.com/zagg/zilla-rs.git
cd zilla-rs
cargo run --release            # listens on 0.0.0.0:8734 — open http://<this-host>:8734
```

## CLI

```
zilla serve   [-c dir] [-b IP] [-p N] [-q]                        start the web app
zilla selftest [-c config]                                        run the 16 protocol checks
zilla import  <sitemanager.xml>                                   import FileZilla sites
zilla export  <sitemanager.xml>                                   export sites to FileZilla XML
zilla version
```

`-b/--bind` (or `--host`) defaults to `0.0.0.0`, i.e. every interface: the UI is reachable
from the local network out of the box, and `http://127.0.0.1:8734` still works from the
machine itself. Pass `-b 127.0.0.1` to keep it local-only.

Configuration is a single JSON file per `--config` directory (default
`~/.config/zilla-rs/zilla.json`), which also holds sites, recents, bookmarks and the
transfer queue.

## HTTP API

All endpoints live under `/api`. No authentication, no session: the server is a LAN tool,
anyone who can reach the port can drive it.

```sh
# quick-connect over SFTP
curl -s -H "content-type: application/json" \
  -d '{"host":"example.org","port":22,"proto":"sftp","user":"bob","password":"secret"}' \
  localhost:8734/api/connect

# list a directory
curl -s "localhost:8734/api/remote/list?site=<siteId>&path=/var"

# queue a recursive download
curl -s -H "content-type: application/json" \
  -d '{"site":"<siteId>","paths":["/var/log"],"dir":"/tmp/back","dirs":true}' \
  localhost:8734/api/download

# keep the tree: every path below `base` is mirrored on the other side
curl -s -H "content-type: application/json" \
  -d '{"site":"<siteId>","paths":["/srv/www/index.html"],"dir":"/tmp/back","dirs":true,"base":"/srv/www"}' \
  localhost:8734/api/download
```

`paths` may be absolute, or relative when `base` is given — that is exactly how the
compare dialog feeds the queue.

`/_ws` streams JSON events (`log`, `queue`, `connections`, `toast`) for live UI refresh.
End-to-end health checks: `GET /api/selftest`.

## Reverse-engineering notes

Protocol behaviour, log phrasing, `sitemanager.xml` layout and keyboard shortcuts were derived
from FileZilla 3.68 (strings and shipped resources) using a reverse-engineering workflow — the
implementation is original Rust, no FileZilla code is used or linked. See `docs/REA.md`.

## Security notes

- The web server listens on `0.0.0.0:8734` by default: anyone on the network can open the UI
  and drive it. There is no authentication and no multi-user mode — keep it on a trusted
  network, or bind it back to the machine with `--bind 127.0.0.1`.
- Site passwords are stored in the local config file in clear text, exactly like FileZilla's
  XML; keep that file protected (`chmod 600`).
- TLS uses `rustls` with the Mozilla root store (`webpki-roots`). Certificate validation is
  enforced; there is no "accept any certificate" switch.
- Do not expose the local pane to untrusted networks: it reads and writes real files.

## Testing

```sh
cargo test --lib          # 20 unit tests (list parsers, paths, ids)
cargo test                # + integration tests (fixture FTP server, pools, transfer queue)
cargo build --release
./target/release/zilla selftest
```

## License

MIT.
