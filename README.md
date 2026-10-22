# Zilla-RS

**A FileZilla look-alike FTP/FTPS/SFTP client that runs entirely in your browser — written in Rust, served by a single small binary.**

Zilla-RS is a self-contained, autonomous transfer client: one Rust process hosts the whole
application (HTTP API, WebSocket live updates, transfer engine, FTP/FTPS/SFTP protocol stack)
and serves a dependency-free web UI. No CDN, no Node, no bundling, no Electron, no external
assets: open `http://127.0.0.1:8230` and start transferring.

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
- Resume (`REST`), overwrite/skip/newer/rename policies, mtime preservation
- Directory recursion, queue persistence across restarts, completion notification

**Site manager**
- Nested groups, per-site overrides (timeouts, commands, TLS level, logintype)
- FileZilla-compatible XML import/export
- Quick connect, recents, bookmarks

**Settings & logging**
- Nine grouped settings tabs, applied live over WebSocket
- Log categories (`general`, `command`, `status`, `transfer`, …), timestamps, save to file
- Themes (dark/light), font, pane layout — persisted per browser

**Selftest**
- 16 end-to-end checks against an embedded demo FTP server, run from the UI or the CLI

## Quick start

```sh
git clone https://github.com/zagg/zilla-rs.git
cd zilla-rs
cargo run --release            # opens http://127.0.0.1:8230
```

Want something to click around? Demo mode seeds a fake FTP server and files for you:

```sh
cargo run --release -- demo     # embedded FTP on 127.0.0.1:2121, UI on 8230
```

## CLI

```
zilla serve   [-c config] [--port N] [--host IP] [--no-browser]   start the web app
zilla demo    [-c config] [--port N]                              demo FTP server + UI
zilla selftest [-c config]                                        run the 16 protocol checks
zilla import  <sitemanager.xml>                                   import FileZilla sites
zilla export  <sitemanager.xml>                                   export sites to FileZilla XML
zilla version
```

Configuration is a single JSON file per `--config` directory (default
`~/.config/zilla-rs/zilla.json`), which also holds sites, recents, bookmarks and the
transfer queue.

## HTTP API

All endpoints live under `/api`. The server prints a per-session token into the log and injects
it into the page; mutating calls must send it as `x-zilla-token`.

```sh
TOKEN=$(curl -s localhost:8230/api/status | jq -r .data.token)

# quick-connect over SFTP
curl -s -H "content-type: application/json" -H "x-zilla-token: $TOKEN" \
  -d '{"host":"example.org","port":22,"proto":"sftp","user":"bob","password":"secret"}' \
  localhost:8230/api/connect

# list a directory
curl -s -H "x-zilla-token: $TOKEN" \
  "localhost:8230/api/remote/list?site=<siteId>&path=/var"

# queue a recursive download
curl -s -H "content-type: application/json" -H "x-zilla-token: $TOKEN" \
  -d '{"site":"<siteId>","paths":["/var/log"],"dir":"/tmp/back","dirs":true}' \
  localhost:8230/api/download
```

`/_ws` streams JSON events (`log`, `queue`, `connections`, `toast`) for live UI refresh.
End-to-end health checks: `GET /api/selftest`.

## Reverse-engineering notes

Protocol behaviour, log phrasing, `sitemanager.xml` layout and keyboard shortcuts were derived
from FileZilla 3.68 (strings and shipped resources) using a reverse-engineering workflow — the
implementation is original Rust, no FileZilla code is used or linked. See `docs/REA.md`.

## Security notes

- The web server binds to `127.0.0.1` by default; `--host` lets you expose it, in which case
  the token guard is your only authentication. There is no multi-user mode.
- Site passwords are stored in the local config file in clear text, exactly like FileZilla's
  XML; keep that file protected (`chmod 600`).
- TLS uses `rustls` with the Mozilla root store (`webpki-roots`). Certificate validation is
  enforced; there is no "accept any certificate" switch.
- Do not expose the local pane to untrusted networks: it reads and writes real files.

## Testing

```sh
cargo test --lib          # 20 unit tests (list parsers, paths, ids)
cargo test                # + integration tests (demo server, pools, transfer queue)
cargo build --release
./target/release/zilla selftest
```

## License

MIT.
