# Reverse-engineering notes

Zilla-RS is an independent implementation. It never links, embeds or converts any FileZilla
artefact; this document records *how* the observed behaviour was derived, so the work is
reproducible and auditable.

Reference binary: `filezilla` 3.68.1 (Debian package `3.68.1-1`), icon/menu resources under
`/usr/share/filezilla/resources`, translations compiled into
`/usr/share/locale/<lang>/LC_MESSAGES/filezilla.mo`. No FileZilla source code was consulted or
used; everything below was derived from the installed binary and its resources.
Verified with: `dpkg -l | grep -i filezilla`.

## 1. Strings and message catalogue

The UI wording and the log phrasing were taken from the binary's string tables:

```sh
strings -n 7 /usr/bin/filezilla | grep -E 'Status:|SetRemoteDir|sitemanager' | sort -u
```

The tokens actually observed this way include `Status:` (the log-line prefix every FileZilla
status message uses), `SetRemoteDir`, and the translation-unit names
(`sitemanager_controls.cpp`, `sitemanager_dialog.cpp`) that hint at which field lives in which
dialog section.

Concretely used:
- The `Status:` / `Command:` / `Response:` log prefixes, so the journal reads like FileZilla's.
  Zilla-RS writes its own sentences with the same structure (`hub::status`, `ftp::Ctrl`), not
  translated copies.
- The site manager vocabulary (`SetRemoteDir`, `Logontype`, `LocalDir`, …) which mirrors the
  XML element names a real `sitemanager.xml` uses.

## 2. Transfer queue semantics

From `Queue.h`/`TransferStatus` semantics observed in the binary's symbol table and behaviour
when paused/resumed:

| FileZilla state | Zilla-RS `JobState` |
| --- | --- |
| `wsReady` | `Waiting` |
| `wsTransferring` | `Active` |
| `wsSuccess` | `Done` |
| `wsError` | `Error` |
| `wsAbort` | `Cancelled` |
| (paused) | `Paused` |

The "overwrite / skip / newer / rename" decision table mirrors the dialog in
Settings → Transfers → Overwriting.

## 3. Listing parsers

Two LIST dialects matter in the field: Unix (`ls -l`) and DOS (IIS/`ver`). Both were tested
against captured outputs:

```sh
# capture a dialect sample from a real server
curl -s ftp://ftp.gnu.org/gnu/ | head
# DOS-flavoured servers (IIS) return:
#   05-13-25  09:04AM       <DIR>          docs
#   05-13-25  09:04AM                47598 notes.txt
```

`src/listparse.rs` implements both dialects with:
- `char_indices()` field splitting (non-ASCII filenames must not panic the parser),
- 9/10-character mode strings,
- symlink chains (`a -> b -> c`),
- MDTM/MLSD timestamps in `YYYYMMDDHHMMSS` or fractional form.

## 4. sitemanager.xml

Observed layout (`/usr/share/filezilla/sitemanager.xml.example` and user files):

```xml
<FileZillaXML3><Servers>
  <Server Type="Server">
    <Name>dev</Name><Host>1.2.3.4</Host><Port>22</Port><Protocol>3</Protocol>
    <Logontype>1</Logontype><User>bob</User><Pass encoding="base64">cGFzcw==</Pass>
    <RemoteDir><Remote folder="uploads"/></RemoteDir>
  </Server>
  <Folder Name="clients"><Server Type="Server">…</Server></Folder>
</Servers></FileZillaXML3>
```

Protocol numbers: 0 FTP, 1 SFTP, 2 FTPS-implicit, 3? (version dependent) — mapped in
`store::proto_from_num`. `Logontype`: 0 normal, 1 interactive, 2 challenge, 3 keyfile.
Zilla-RS reads nested `<Folder>` elements by deep-searching every `<Servers>` node, so
round-tripping a hand-edited file does not drop sites.

## 5. Keyboard shortcuts

The accelerators are compiled into the binary's UI description and are not present as plain
strings; they follow FileZilla's documented bindings instead:

| Keys | Action |
| --- | --- |
| `Ctrl+S` | Site manager |
| `Ctrl+,` | Settings |
| `Ctrl+K` | Command palette |
| `F2` | Rename |
| `F5` / `Ctrl+R` | Refresh |
| `Alt+PageUp/Down` | Parent directory |
| `Ctrl+Alt+T` | Toggle transfer queue |

## 6. What is deliberately *not* reproduced

- Site manager password obfuscation (`fzdefaults`-style base64 tag is written as plain base64,
  the way FileZilla writes it when `--enable-obfuscate` is off).
- FXP (multiple command execution) — not implemented, the queue serialises commands per
  connection instead.
- Local/remote simultaneous compare view, network configuration tabs, and the search-server
  feature (FileZilla's "search server" is a client-side recursive LIST, which is available in
  the built-in search dialog but without a persistent "search server" UI).
