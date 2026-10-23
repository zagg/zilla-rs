/* Zilla-RS — client web autonome (aucune dépendance externe) */
"use strict";

/* ═══════════════════════════ outils ═══════════════════════════ */
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const el = (tag, attrs = {}, ...kids) => {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") n.className = v;
    else if (k === "html") n.innerHTML = v;
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else if (v !== null && v !== undefined) n.setAttribute(k, v);
  }
  for (const c of kids.flat()) {
    if (c === null || c === undefined) continue;
    n.appendChild(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return n;
};
const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const debounce = (fn, ms = 260) => { let t; return (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); }; };

const fmt = {
  size(n) {
    n = Number(n) || 0;
    const u = ["o", "Kio", "Mio", "Gio", "Tio", "Pio"];
    let i = 0, v = n;
    while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
    const d = v < 10 ? 2 : v < 100 ? 1 : 0;
    return `${v.toFixed(d).replace(".", ",")} ${u[i]}`;
  },
  speed(b) { return `${fmt.size(b)}/s`; },
  time(ts) {
    if (!ts) return "";
    const d = new Date(ts * 1000);
    return d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  },
  date(ts) {
    if (!ts) return "";
    const d = new Date(ts * 1000);
    const now = Date.now();
    const same = d.getFullYear() === new Date().getFullYear();
    const mo = d.toLocaleDateString("fr-FR", { month: "short" });
    return same ? `${mo} ${String(d.getDate()).padStart(2, "0")} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`
      : `${mo} ${String(d.getDate()).padStart(2, "0")} ${d.getFullYear()}`;
  },
  dur(s) {
    s = Math.max(0, Math.floor(s || 0));
    const h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60), x = s % 60;
    return h ? `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(x).padStart(2, "0")}` : `${m}:${String(x).padStart(2, "0")}`;
  },
};

async function apiGet(path) {
  const r = await fetch("/api" + path);
  const j = await r.json().catch(() => ({ ok: false, error: "réponse invalide" }));
  if (!j.ok) throw new Error(j.error || "erreur " + r.status);
  return j.data;
}
async function apiPost(path, body) {
  const r = await fetch("/api" + path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body ?? {}),
  });
  const j = await r.json().catch(() => ({ ok: false, error: "réponse invalide" }));
  if (!j.ok) throw new Error(j.error || "erreur " + r.status);
  return j.data;
}

const store = {
  get(k, d) { try { const v = localStorage.getItem("zilla:" + k); return v === null ? d : JSON.parse(v); } catch { return d; } },
  set(k, v) { try { localStorage.setItem("zilla:" + k, JSON.stringify(v)); } catch { } },
};

/* ═══════════════════════════ icônes ═══════════════════════════ */
const IC = {
  dir: '<svg viewBox="0 0 24 24" class="ic"><path fill="#fbbf24" d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
  file: '<svg viewBox="0 0 24 24" class="ic"><path fill="#7dd3fc" d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/></svg>',
  text: '<svg viewBox="0 0 24 24" class="ic"><path fill="#a3e635" d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path stroke="#16233a" stroke-width="1.4" d="M9 12h6M9 15h6M9 9h3"/></svg>',
  img: '<svg viewBox="0 0 24 24" class="ic"><rect x="3" y="5" width="18" height="14" rx="2" fill="#c4b5fd"/><circle cx="9" cy="10" r="1.7" fill="#fff"/><path d="M4 18l5-4 3 3 4-4 4 5z" fill="#7c3aed"/></svg>',
  arch: '<svg viewBox="0 0 24 24" class="ic"><path fill="#f0abfc" d="M4 5h16v4H4zm1 5h14v9H5z"/><path stroke="#4a1758" d="M12 5v4M10 12h4v3h-4z"/></svg>',
  bin: '<svg viewBox="0 0 24 24" class="ic"><path fill="#fda4af" d="M6 7h12l-1 13H7z"/><path fill="#9f1239" d="M9 4h6v2H9z"/></svg>',
  link: '<svg viewBox="0 0 24 24" class="ic"><path stroke="#60a5fa" stroke-width="2" fill="none" d="M9 15l6-6M8 13l-2 2a3 3 0 0 0 4 4l2-2M16 11l2-2a3 3 0 0 0-4-4l-2 2"/></svg>',
  code: '<svg viewBox="0 0 24 24" class="ic"><path fill="#94a3b8" d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path stroke="#0f172a" stroke-width="1.5" fill="none" d="M10 12l-2 2 2 2m4-4l2 2-2 2"/></svg>',
  media: '<svg viewBox="0 0 24 24" class="ic"><rect x="3" y="5" width="18" height="14" rx="2" fill="#22d3ee"/><path fill="#083344" d="M10 9l6 3-6 3z"/></svg>',
  up: '<svg viewBox="0 0 24 24" class="ic"><path fill="#34d399" d="M12 3l6 7h-4v6h-4v-6H6z"/></svg>',
};
/* ════════════════════════ genres d'entrée ════════════════════════ */
/* Le serveur renvoie « dir » | « file » | « symlink » | « other ». On tolère
   au passage d'autres écritures (casse haute, « directory », « link ») pour
   qu'un vieux listing ou un autre fournisseur ne fasse pas planter la
   navigation : un dossier doit rester un dossier, sinon le double-clic
   ne transfère plus rien et n'ouvre plus le répertoire. */
function normKind(k) {
  const s = String(k || "").toLowerCase();
  if (s === "dir" || s === "directory") return "dir";
  if (s === "link" || s === "symlink") return "symlink";
  if (s === "file" || s === "f") return "file";
  return s || "file";
}
const isDir = (e) => normKind(e && e.kind) === "dir";
const isLink = (e) => normKind(e && e.kind) === "symlink" || !!(e && e.link);
function normEntries(list) {
  for (const e of (list || [])) e.kind = normKind(e.kind);
  return list || [];
}

function iconFor(e, local) {
  if (isDir(e)) return IC.dir;
  if (isLink(e)) return IC.link;
  const ext = (e.name.split(".").pop() || "").toLowerCase();
  if (["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico"].includes(ext)) return IC.img;
  if (["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "iso"].includes(ext)) return IC.arch;
  if (["mp3", "wav", "ogg", "flac", "mp4", "mkv", "avi", "mov", "webm"].includes(ext)) return IC.media;
  if (["js", "ts", "rs", "py", "go", "c", "h", "cpp", "java", "html", "css", "json", "yml", "yaml", "toml", "sh"].includes(ext)) return IC.code;
  if (["txt", "md", "log", "csv", "ini", "cfg", "xml"].includes(ext)) return IC.text;
  return IC.file;
}

/* ═══════════════════════════ état ═══════════════════════════ */
const S = {
  version: "",
  settings: {},
  sites: [],
  connected: {},
  queue: { jobs: [], stats: {} },
  local: { path: "/", entries: [], sel: new Set(), sort: { key: "name", desc: false }, filter: "", dirsFirst: true },
  remote: { site: null, path: "/", entries: [], sel: new Set(), sort: { key: "name", desc: false }, filter: "", dirsFirst: true, connected: false, info: null },
  active: "local",
  log: { seq: 0, cat: "all", verbose: false, lines: [] },
  recents: { hosts: [], local: [], remote: [] },
  bookmarks: [],
  selectedSite: null,
  ws: null,
  wsT: 0,
  cols: store.get("cols", {}),
};
const PANES = { local: S.local, remote: S.remote };

/* ═══════════════════════════ journal ═══════════════════════════ */
function logPush(line, jump = false) {
  S.log.lines.push(line);
  if (S.log.lines.length > 2500) S.log.lines.splice(0, 600);
  if (jump) S.log.seq = (S.log.seq || 0);
}
function renderLog() {
  const box = $("#log");
  const stick = box.scrollHeight - box.scrollTop - box.clientHeight < 60;
  const f = S.log.cat;
  const show = S.log.lines.filter((l) => (f === "all" || l.cat === f) && (S.log.verbose || l.level !== "trace"));
  const html = show.map((l) => `<div class="l ${esc(l.level)} ${esc(l.cat)}"><span class="ts">${esc(fmt.time((l.ts || 0) / 1000))}</span><span class="lvl">${esc(l.level)}</span><span class="txt">${esc(l.text)}</span></div>`).join("");
  box.innerHTML = html || `<div class="empty">Journal vide.</div>`;
  if (stick) box.scrollTop = box.scrollHeight;
  const d = box.dataset.dir;
  if (d === "down") box.scrollTop = box.scrollHeight;
  delete box.dataset.dir;
}
function logSay(cat, level, text) { pushLog({ ts: Date.now(), cat, level, text }); }
function pushLog(line) {
  const same = S.log.lines.length && S.log.lines[S.log.lines.length - 1].text === line.text;
  if (!same) S.log.lines.push(line);
  if (S.log.lines.length > 3000) S.log.lines.splice(0, 800);
  renderLog();
}

/* ═══════════════════════════ toasts ═══════════════════════════ */
function toast(text, kind = "", title = "") {
  const t = el("div", { class: "toast " + kind }, title && el("div", { class: "t" }, title), el("div", {}, text));
  $("#toasts").appendChild(t);
  setTimeout(() => { t.style.transition = "opacity .3s, transform .3s"; t.style.opacity = "0"; t.style.transform = "translateY(6px)"; setTimeout(() => t.remove(), 320); }, kind === "err" ? 5200 : 3200);
}
function fail(e, ctx = "") {
  const msg = (e && e.message) || String(e);
  toast((ctx ? ctx + " — " : "") + msg, "err", "Erreur");
  console.error(ctx, e);
}

/* ═══════════════════════════ panneaux ═══════════════════════════ */
const COLS = {
  name: { w: 300, label: "Nom" },
  size: { w: 92, num: true, label: "Taille" },
  type: { w: 100, label: "Type" },
  modified: { w: 112, label: "Date de modification" },
  perm: { w: 92, label: "Permissions", mono: true },
  user: { w: 76, label: "User" },
  group: { w: 84, label: "Groupe" },
};
function colsFor(which) {
  const list = (S.settings.ui && S.settings.ui[which + "_columns"]) || ["name", "size", "type", "modified", "perm"];
  return list.filter((c) => COLS[c]);
}
function colWidth(which, col) { return S.cols[which + "." + col] ?? COLS[col].w; }
function gridTemplate(which) {
  const cs = colsFor(which);
  const rest = cs.filter((c) => c !== "name");
  const used = rest.reduce((a, c) => a + colWidth(which, c), 0) + 8;
  return `${Math.max(90, 1000) - used}px `.trim() && `${cs.map((c) => (c === "name" ? "minmax(120px, 1fr)" : colWidth(which, c) + "px")).join(" ")}`;
}

function cellText(which, col, e) {
  switch (col) {
    case "name": return e.name;
    case "size": return isDir(e) ? "" : fmt.size(e.size);
    case "type": return isDir(e) ? "Répertoire" : isLink(e) ? "Lien" : "Fichier";
    case "modified": return fmt.date(e.mtime);
    case "perm": return e.mode != null ? perms(e.mode, isDir(e)) : "";
    case "user": return e.user || "";
    case "group": return e.group || "";
  }
  return "";
}
function perms(mode, dir) {
  let s = dir ? "d" : "-";
  for (let i = 0; i < 9; i++) {
    const b = (mode >> (8 - i)) & 1;
    s += "rwx"[i % 3] && b ? "rwx"[i % 3] : "-";
  }
  return s;
}

function filtered(which) {
  const p = PANES[which];
  let v = p.entries.filter((e) => !p.filter || e.name.toLowerCase().includes(p.filter.toLowerCase()));
  const k = p.sort.key, desc = p.sort.desc ? -1 : 1;
  v.sort((a, b) => {
    if (p.dirsFirst) {
      if (isDir(a) && !isDir(b)) return -1;
      if (!isDir(a) && isDir(b)) return 1;
    }
    let r = 0;
    if (k === "size") r = (a.size || 0) - (b.size || 0);
    else if (k === "type") r = (normKind(a.kind) || "").localeCompare(normKind(b.kind) || "");
    else if (k === "modified") r = (a.mtime || 0) - (b.mtime || 0);
    else if (k === "perm") r = (a.mode || 0) - (b.mode || 0);
    else if (k === "user" || k === "group") r = (a[k] || "").localeCompare(b[k] || "");
    else r = (a.name || "").localeCompare(b.name || "", "fr", { numeric: true });
    return r * desc;
  });
  return v;
}

function renderPane(which) {
  const p = PANES[which];
  const list = $("#list-" + which), col = $("#cols-" + which);
  const cs = colsFor(which);
  col.style.gridTemplateColumns = gridTemplate(which);
  col.innerHTML = cs.map((c) => `<div class="col ${p.sort.key === c ? "sorted" : ""} ${p.sort.desc ? "desc" : ""}" data-col="${c}" style="width:${colWidth(which, c)}px" title="Trier par ${COLS[c].label}">${esc(COLS[c].label)}</div>`).join("");
  const v = filtered(which);
  const LIMIT = 2500;
  const rows = [];
  for (let i = 0; i < Math.min(v.length, LIMIT); i++) {
    const e = v[i];
    const idx = p.entries.indexOf(e);
    const sel = p.sel.has(e.name);
    const cls = ["r", isDir(e) ? "dir" : isLink(e) ? "link" : "", e.hidden ? "hidden" : "", sel ? "sel" : ""].filter(Boolean).join(" ");
    const cells = cs.map((c) => {
      const t = cellText(which, c, e);
      if (c === "name") return `<div class="c name">${iconFor(e, which === "local")}<span>${esc(t)}</span>${e.link ? `<span class="dim">→ ${esc(e.link)}</span>` : ""}</div>`;
      return `<div class="c ${COLS[c].num ? "num" : ""} ${COLS[c].mono ? "mono" : ""} dim">${esc(t)}</div>`;
    }).join("");
    rows.push(`<div class="${cls}" data-idx="${idx}" data-name="${esc(e.name)}" draggable="true" style="grid-template-columns:${gridTemplate(which)}">${cells}</div>`);
  }
  if (v.length > LIMIT) rows.push(`<div class="empty">+ ${v.length - LIMIT} autres éléments — filtrez pour alléger.</div>`);
  list.innerHTML =
    rows.join("") ||
    (which === "remote" && !S.remote.connected
      ? `<div class="empty">Non connecté — choisissez un site dans le gestionnaire, ou <button type="button" class="link" id="qc-hint">connexion rapide</button>.</div>`
      : `<div class="empty">${which === "remote" ? "Aucun élément à afficher." : "Rien à afficher."}</div>`);
  const qc = $("#qc-hint");
  if (qc) qc.addEventListener("click", (ev) => { ev.stopPropagation(); const h = $("#qc-host"); if (h) { h.focus(); h.select?.(); } });
  status(which);
}

function status(which) {
  const p = PANES[which];
  const v = filtered(which);
  const files = v.filter((e) => !isDir(e));
  const bytes = files.reduce((a, e) => a + (e.size || 0), 0);
  const filt = p.filter ? ` · ${v.length}/${p.entries.length} affichés` : "";
  const txt = `<b>${esc(p.path || "/")}</b> — ${v.filter(isDir).length} dossier(s), ${files.length} fichier(s), ${fmt.size(bytes)}${filt}`;
  $(which === "local" ? "#st-local" : "#st-remote").innerHTML = txt;
  const crumbs = $("#crumbs-" + which);
  crumbs.innerHTML = (which === "local" ? localCrumbs(p.path) : remoteCrumbs(p.path)).join("");
  const pe = $("#pathedit-" + which);
  if (pe && document.activeElement !== pe) pe.value = p.path || "/";
}
function localCrumbs(path) {
  const parts = (path || "/").split("/").filter(Boolean);
  const out = [`<div class="cr" data-go="/">⌂</div>`];
  let acc = "";
  for (const [i, p] of parts.entries()) {
    acc += "/" + p;
    out.push(`<span class="slash">›</span>`);
    out.push(`<div class="cr ${i === parts.length - 1 ? "cur" : ""}" data-go="${esc(acc)}">${esc(p)}</div>`);
  }
  return out;
}
function remoteCrumbs(path) {
  if (!S.remote.connected) return [`<div class="cr" style="cursor:default">— non connecté —</div>`];
  const parts = (path || "/").split("/").filter(Boolean);
  const out = [`<div class="cr" data-go="/">⌂</div>`];
  let acc = "";
  for (const [i, p] of parts.entries()) {
    acc += "/" + p;
    out.push(`<span class="slash">›</span>`);
    out.push(`<div class="cr ${i === parts.length - 1 ? "cur" : ""}" data-go="${esc(acc)}">${esc(p)}</div>`);
  }
  return out;
}

/* chargement */
async function loadLocal(path = S.local.path) {
  try {
    const d = await apiGet("/local/list?path=" + encodeURIComponent(path) + "&hidden=" + (S.settings.general?.show_hidden_local ? "true" : "false"));
    S.local.path = d.path;
    S.local.entries = normEntries(d.entries || []);
    keepSel(S.local);
    renderPane("local");
    if (S.active === "local") $("#pane-local").scrollTop = 0;
  } catch (e) { fail(e, "répertoire local"); }
}
async function loadRemote(path = S.remote.path, quiet = false) {
  if (!S.remote.site) return;
  try {
    const d = await apiGet(`/remote/list?site=${encodeURIComponent(S.remote.site)}&path=` + encodeURIComponent(path));
    S.remote.path = d.path;
    S.remote.entries = normEntries(d.entries || []);
    S.remote.connected = true;
    keepSel(S.remote);
    renderPane("remote");
    $("#st-conns").innerHTML = connBadge();
  } catch (e) {
    if (!quiet) fail(e, "répertoire distant");
    throw e;
  }
}
function keepSel(p) {
  const names = new Set(p.entries.map((e) => e.name));
  p.sel = new Set([...p.sel].filter((n) => names.has(n)));
}
function selected(which) {
  const p = PANES[which];
  return p.entries.filter((e) => p.sel.has(e.name));
}

/* La liste est entièrement revue à chaque rendu : entre les deux clics d'un
   double-clic, le nœud touché peut avoir été remplacé. On retrouve donc
   l'entrée par son nom, quitte à refaire un pointage visuel. */
function rowOf(which, ev) {
  const list = $("#list-" + which);
  let row = ev.target && ev.target.closest ? ev.target.closest(".r") : null;
  if (row && !list.contains(row)) row = null;
  if (!row && (ev.clientX || ev.clientY) && typeof document.elementFromPoint === "function") {
    const hit = document.elementFromPoint(ev.clientX, ev.clientY);
    row = hit && hit.closest ? hit.closest(".r") : null;
    if (row && !list.contains(row)) row = null;
  }
  return row;
}
function entryOf(which, ev) {
  const p = PANES[which];
  const row = rowOf(which, ev);
  if (!row) return null;
  const byIdx = p.entries[+row.dataset.idx];
  if (byIdx && byIdx.name === row.dataset.name) return byIdx;
  return p.entries.find((x) => x.name === row.dataset.name) || null;
}
/* Ne retouche que la surbrillance : le DOM reste stable, le double-clic garde
   son nœud vivant et le défilement ne saute pas. */
function paintSel(which) {
  const p = PANES[which];
  for (const row of $("#list-" + which).querySelectorAll(".r")) row.classList.toggle("sel", p.sel.has(row.dataset.name));
}

/* navigation */
async function goto(which, path) {
  if (which === "local") await loadLocal(path);
  else {
    if (!S.remote.connected && !S.remote.site) { toast("Aucune connexion distante active.", "warn"); return; }
    await loadRemote(path, true);
  }
}
function parentOf(p) {
  const s = (p || "/").replace(/[\\/]+$/, "") || "/";
  if (s === "/" ) return "/";
  const i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\"));
  return i <= 0 ? "/" : s.slice(0, i);
}

/* ═══════════════════════════ sélection ═══════════════════════════ */
function bindPane(which) {
  const pane = $("#pane-" + which);
  const list = $("#list-" + which);
  pane.addEventListener("mousedown", () => { S.active = which; pane.focus(); });
  list.addEventListener("click", (ev) => {
    const e = entryOf(which, ev);
    if (!e) return;
    const p = PANES[which];
    const items = filtered(which);
    if (ev.ctrlKey || ev.metaKey) {
      p.sel.has(e.name) ? p.sel.delete(e.name) : p.sel.add(e.name);
    } else if (ev.shiftKey && p.anchor) {
      const names = items.map((x) => x.name);
      const a = names.indexOf(p.anchor), b = names.indexOf(e.name);
      p.sel = new Set(names.slice(Math.min(a, b), Math.max(a, b) + 1));
    } else {
      p.sel = new Set([e.name]);
      p.anchor = e.name;
    }
    paintSel(which);
  });
  list.addEventListener("dblclick", (ev) => {
    const e = entryOf(which, ev); if (e) open(which, e);
  });
  list.addEventListener("contextmenu", (ev) => {
    ev.preventDefault();
    const p = PANES[which];
    const e = entryOf(which, ev);
    if (e) {
      if (!p.sel.has(e.name)) { p.sel = new Set([e.name]); renderPane(which); }
    } else p.sel.clear();
    menu(which, ev.clientX, ev.clientY);
  });
  list.addEventListener("dragstart", (ev) => {
    const p = PANES[which];
    const e = entryOf(which, ev);
    if (!e) return;
    if (!p.sel.has(e.name)) { p.sel = new Set([e.name]); renderPane(which); }
    const names = [...p.sel];
    const base = which === "local" ? p.path : p.path;
    ev.dataTransfer.setData("text/zilla", JSON.stringify({ from: which, base, names }));
    ev.dataTransfer.effectAllowed = "copyMove";
  });
  pane.addEventListener("dragover", (ev) => { ev.preventDefault(); pane.classList.add("dragover"); });
  pane.addEventListener("dragleave", () => pane.classList.remove("dragover"));
  pane.addEventListener("drop", async (ev) => {
    ev.preventDefault(); pane.classList.remove("dragover");
    const data = ev.dataTransfer.getData("text/zilla");
    if (data) {
      const { from, base, names } = JSON.parse(data);
      if (from === which) return;
      const target = PANES[which].path;
      if (from === "local" && which === "remote") uploadPaths(names.map((n) => joinLocal(base, n)), target);
      else if (from === "remote" && which === "local") downloadPaths(names.map((n) => joinRemote(base, n)), target);
      return;
    }
    // fichiers du navigateur -> panneau local
    const files = [...(ev.dataTransfer.files || [])];
    if (files.length && which === "local") {
      const dir = PANES.local.path;
      for (const f of files) {
        const buf = await f.arrayBuffer();
        await apiPost("/local/save", { path: joinLocal(dir, f.name), content: b64(buf), base64: true });
      }
      toast(`${files.length} fichier(s) enregistré(s) localement.`, "ok", "Reçu");
      loadLocal();
    } else if (files.length && which === "remote") {
      toast("Étape intermédiaire : enregistrez d'abord les fichiers dans le panneau local.", "warn");
    }
  });
  $("#cols-" + which).addEventListener("click", (ev) => {
    const c = ev.target.closest(".col"); if (!c) return;
    const p = PANES[which];
    if (p.sort.key === c.dataset.col) p.sort.desc = !p.sort.desc;
    else { p.sort.key = c.dataset.col; p.sort.desc = c.dataset.col === "size" || c.dataset.col === "modified"; }
    renderPane(which);
  });
  $("#cols-" + which).addEventListener("mousedown", (ev) => {
    const c = ev.target.closest(".col");
    if (!c || ev.offsetX < 6) return;
    ev.preventDefault();
    const key = which + "." + c.dataset.col;
    const startX = ev.clientX, start = colWidth(which, c.dataset.col);
    const mv = (e) => { S.cols[key] = Math.max(45, start + (e.clientX - startX)); store.set("cols", S.cols); renderPane(which); };
    const up = () => { removeEventListener("mousemove", mv); removeEventListener("mouseup", up); };
    addEventListener("mousemove", mv); addEventListener("mouseup", up);
  });
  $("#crumbs-" + which).addEventListener("click", (ev) => {
    const c = ev.target.closest(".cr"); if (!c || !c.dataset.go) return;
    goto(which, c.dataset.go);
  });
  $("#cols-" + which).addEventListener("dblclick", (ev) => {
    const c = ev.target.closest(".col"); if (c) { S.cols[which + "." + c.dataset.col] = COLS[c.dataset.col].w; store.set("cols", S.cols); renderPane(which); }
  });
  const f = $("#filter-" + which);
  f.addEventListener("input", debounce(() => { PANES[which].filter = f.value.trim(); renderPane(which); }, 180));
  f.addEventListener("keydown", (ev) => { if (ev.key === "Escape") { f.value = ""; PANES[which].filter = ""; renderPane(which); f.blur(); } });
  const pe = $("#pathedit-" + which);
  pe.addEventListener("keydown", (ev) => { if (ev.key === "Enter") { goto(which, pe.value.trim() || "/"); pe.blur(); } });
  $("#up-" + which).addEventListener("click", () => goto(which, parentOf(PANES[which].path)));
  $("#pane-" + which).querySelector('[data-act="refresh"]').addEventListener("click", () => (which === "local" ? loadLocal() : loadRemote()));
  $("#pane-" + which).querySelector('[data-act="mkdir"]').addEventListener("click", () => mkdir(which));
  pane.addEventListener("keydown", (ev) => { if (ev.key === "Enter") { const e = selected(which)[0]; if (e) open(which, e); } });
}
function joinLocal(base, name) { return (base === "/" ? "" : base) + "/" + name; }
function joinRemote(base, name) { return (base === "/" ? "" : base) + "/" + name; }

/* ═══════════════════════════ opérations ═══════════════════════════ */
function open(which, e) {
  const mode = (S.settings.general && S.settings.general.double_click) || "transfer";
  if (isDir(e)) return goto(which, joinRemote(PANES[which].path, e.name));
  if (mode === "view") return view(which, e);
  if (which === "local") return upload([joinLocal(PANES.local.path, e.name)], S.remote.path, true);
  return download([e.path || joinRemote(S.remote.path, e.name)], S.local.path, true);
}
async function view(which, e) {
  const path = which === "local" ? joinLocal(PANES.local.path, e.name) : (e.path || joinRemote(S.remote.path, e.name));
  const url = `/api/${which === "local" ? "local" : "remote"}/file?path=${encodeURIComponent(path)}` + (which === "remote" ? `&site=${encodeURIComponent(S.remote.site)}` : "");
  const ext = (e.name.split(".").pop() || "").toLowerCase();
  if (["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico"].includes(ext)) return dialogImage(path, url);
  if (["mp3", "wav", "ogg", "flac"].includes(ext)) return dialogMedia(path, url, "audio");
  if (["mp4", "mkv", "webm", "mov", "avi"].includes(ext)) return dialogMedia(path, url, "video");
  if (["zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "iso", "pdf", "bin"].includes(ext)) {
    toast("Aperçu indisponible pour ce type de binaire — utilisez « Transférer ».", "warn");
    return;
  }
  return dialogEditor(path, which, e);
}

async function download(paths, dir, dirsToo) {
  try {
    const d = await apiPost("/download", { site: S.remote.site, paths, dir, dirs: !!dirsToo });
    toast(`${paths.length} article(s) mis(s) en file de téléchargement.`, "ok", "Transfert");
    refreshQueue();
  } catch (e) { fail(e, "téléchargement"); }
}
async function upload(paths, dir, dirsToo) {
  if (!S.remote.connected) return toast("Aucune connexion distante active.", "warn");
  try {
    await apiPost("/upload", { site: S.remote.site, paths, dir, dirs: !!dirsToo });
    toast(`${paths.length} article(s) mis(s) en file d'envoi.`, "ok", "Transfert");
    refreshQueue();
  } catch (e) { fail(e, "téléversement"); }
}
function downloadPaths(names, dir) { download(names.map((n) => n), dir, true); }
function uploadPaths(names, dir) { upload(names, dir, true); }

async function mkdir(which) {
  const p = PANES[which].path;
  const name = await promptBox("Nouveau dossier", "Nom du dossier :");
  if (!name) return;
  const path = (p === "/" ? "" : p) + "/" + name;
  try {
    if (which === "local") await apiPost("/local/op", { op: { kind: "mkdir", path, recursive: true } });
    else await apiPost("/remote/op", { site: S.remote.site, op: { Mkdir: path } });
    which === "local" ? loadLocal() : loadRemote();
  } catch (e) { fail(e); }
}
async function rename(which, e) {
  const p = PANES[which].path;
  const name = await promptBox("Renommer", "Nouveau nom :", e.name);
  if (!name || name === e.name) return;
  try {
    if (which === "local") await apiPost("/local/op", { op: { kind: "rename", from: joinLocal(p, e.name), to: joinLocal(p, name) } });
    else await apiPost("/remote/op", { site: S.remote.site, op: { Rename: { from: joinRemote(p, e.name), to: joinRemote(p, name) } } });
    which === "local" ? loadLocal() : loadRemote();
  } catch (e2) { fail(e2); }
}
async function del(which) {
  const items = selected(which);
  if (!items.length) return;
  const n = items.length;
  const ok = await confirmBox("Supprimer", `Supprimer ${n} élément(s) de ${which === "local" ? "disque local" : "serveur"} ? Cette action est définitive.`);
  if (!ok) return;
  try {
    for (const e of items) {
      if (which === "local") await apiPost("/local/op", { op: { kind: "remove", path: joinLocal(PANES.local.path, e.name), recursive: true } });
      else await apiPost("/remote/op", { site: S.remote.site, op: isDir(e) ? { Rmdir: joinRemote(PANES.remote.path, e.name) } : { Remove: joinRemote(PANES.remote.path, e.name) } });
    }
    toast(`${n} élément(s) supprimé(s).`, "ok");
    which === "local" ? loadLocal() : loadRemote();
  } catch (e) { fail(e, "suppression"); }
}
async function chmod(which, e) {
  const path = which === "local" ? joinLocal(PANES.local.path, e.name) : joinRemote(PANES.remote.path, e.name);
  const cur = e.mode != null ? (e.mode & 0o7777).toString(8).padStart(4, "0") : "644";
  const mode = await promptBox("Permissions", "Mode (ex. 644, ou -rwxr-xr-x) :", cur);
  if (!mode) return;
  try {
    if (which === "local") await apiPost("/local/op", { op: { kind: "chmod", path, mode: parseMode(mode) } });
    else await apiPost("/remote/op", { site: S.remote.site, op: { Chmod: { path, mode: parseMode(mode) } } });
    toast("Permissions mises à jour.", "ok");
    which === "local" ? loadLocal() : loadRemote();
  } catch (err) { fail(err, "permissions"); }
}
function parseMode(s) {
  if (/^[0-7]{1,4}$/.test(s.trim())) return parseInt(s.trim(), 8);
  let m = 0, i = 0;
  for (const c of s.replace(/^[dlcbps-]/, "")) { if (i >= 9) break; m = (m << 1) | (c !== "-" ? 1 : 0); i++; }
  return m << (9 - i);
}

/* ═══════════════════════════ menu contextuel ═══════════════════════════ */
function menu(which, x, y) {
  const m = $("#context");
  m.innerHTML = "";
  const items = selected(which);
  const one = items.length === 1 ? items[0] : null;
  const dir = (p) => joinRemote(PANES[which].path, p);
  const out = [];
  const head = el("div", { class: "head" }, items.length > 1 ? `${items.length} éléments` : one ? one.name : "Aucune sélection");
  m.appendChild(head);
  const add = (label, fn, key, dis) => {
    const i = el("div", { class: "mi" + (dis ? "" : ""), onclick: () => { closeMenu(); fn(); } }, el("span", {}, label), key && el("span", { class: "k" }, key));
    if (dis) i.setAttribute("disabled", "");
    m.appendChild(i);
  };
  if (one) add("Ouvrir", () => open(which, one), "Entrée");
  if (one && !isDir(one)) add("Voir / éditer", () => view(which, one), "Ctrl+I");
  if (items.length) {
    add(which === "local" ? "Téléverser vers le serveur" : "Télécharger vers le disque", () => (which === "local" ? upload(items.map((e) => dir(e.name)), S.remote.path, true) : download(items.map((e) => dir(e.name)), S.local.path, true)), "Ctrl+Entrée");
    add("Ajouter à la file (dossiers compris)", () => (which === "local" ? upload(items.map((e) => dir(e.name)), S.remote.path, true) : download(items.map((e) => dir(e.name)), S.local.path, true)), "Ctrl+T");
  }
  m.appendChild(el("hr"));
  if (one) {
    add("Renommer", () => rename(which, one), "F2");
    add("Permissions…", () => chmod(which, one), "Ctrl+P");
    if (which === "local") add("Propriétés", () => properties(which, one));
  }
  add("Nouveau dossier", () => mkdir(which), "Ctrl+Alt+D");
  if (items.length) {
    m.appendChild(el("hr"));
    add("Supprimer", () => del(which), "Suppr");
  }
  m.appendChild(el("hr"));
  add("Copier le chemin", () => {
    const path = which === "local" ? (one ? dir(one.name) : PANES.local.path) : (one ? dir(one.name) : PANES.remote.path);
    navigator.clipboard?.writeText(path).catch(() => { });
    toast("Chemin copié : " + path, "ok");
  });
  if (which === "remote") add("Ouvrir dans un onglet", () => window.open("/api/remote/file?site=" + encodeURIComponent(S.remote.site) + "&path=" + encodeURIComponent(one ? dir(one.name) : PANES.remote.path), "_blank"));
  add("Actualiser", () => (which === "local" ? loadLocal() : loadRemote()), "F5");
  if (which === "local" && S.remote.site) add("Comparer avec le panneau distant", compareDialog, "Ctrl+Shift+C");
  add("Signer ce chemin", () => addBookmarkFor(which));
  m.style.left = Math.min(x, innerWidth - 250) + "px";
  m.style.top = Math.min(y, innerHeight - m.offsetHeight - 10) + "px";
  m.classList.remove("hidden");
}
function closeMenu() { $("#context").innerHTML = ""; $("#context").classList.add("hidden"); }
addEventListener("click", (e) => { if (!e.target.closest(".context")) closeMenu(); });
addEventListener("contextmenu", (e) => { if (!e.target.closest(".list")) closeMenu(); });

function properties(which, e) {
  const path = joinLocal(PANES.local.path, e.name);
  dialog("Propriétés", `<div class="field"><label>Emplacement</label><div class="mono">${esc(path)}</div></div>
    <div class="field"><label>Type</label><div>${esc(isDir(e) ? "répertoire" : isLink(e) ? "lien" : "fichier")}</div></div>
    <div class="field"><label>Taille</label><div>${esc(fmt.size(e.size))}</div></div>
    <div class="field"><label>Modifié</label><div>${esc(fmt.date(e.mtime))} (${e.mtime ? new Date(e.mtime * 1000).toLocaleString("fr-FR") : "—"})</div></div>
    <div class="field"><label>Permissions</label><div class="mono">${esc(e.mode != null ? perms(e.mode, isDir(e)) : "—")}</div></div>
    <div class="field"><label>Propriétaire</label><div>${esc((e.user || "?") + ":" + (e.group || "?"))}</div></div>`,
    [["Fermer", "close"]]);
}

/* ═══════════════════════════ dialogue générique ═══════════════════════════ */
let onClose = null;
function dialog(title, bodyHtml, buttons, onMount, width) {
  const host = $("#dialog");
  host.innerHTML = `<div class="dialog" ${width ? `style="max-width:${width}px"` : ""}>
    <div class="dh"><h3>${esc(title)}</h3><button class="x" data-x>×</button></div>
    <div class="db">${bodyHtml}</div>
    <div class="df">${(buttons || []).map((b, i) => `<button class="btn ${b[1] === "primary" ? "primary" : ""}" data-b="${i}">${esc(b[0])}</button>`).join("")}<span class="grow"></span></div>
  </div>`;
  $("#overlay").classList.remove("hidden");
  host.querySelectorAll("[data-x]").forEach((n) => (n.onclick = close));
  (buttons || []).forEach((b, i) => {
    const n = host.querySelector(`[data-b="${i}"]`);
    n.onclick = async () => {
      const r = b[2] ? b[2](host) : undefined;
      if (r !== false) close();
    };
  });
  if (typeof onMount === "function") onMount(host);
  return host;
}
function close() { $("#overlay").classList.add("hidden"); $("#dialog").innerHTML = ""; if (onClose) { const f = onClose; onClose = null; f(); } }
$("#overlay").addEventListener("mousedown", (e) => { if (e.target.id === "overlay") close(); });

async function promptBox(title, label, value = "") {
  return new Promise((res) => {
    let inp;
    dialog(title, `<div class="field"><label>${esc(label)}</label><input id="pv" value="${esc(value)}"></div>`,
      [["Annuler", ""], ["Valider", "primary", () => { res(inp.value.trim() || null); }]],
      (h) => { inp = h.querySelector("#pv"); inp.focus(); inp.select(); inp.onkeydown = (e) => { if (e.key === "Enter") h.querySelector('[data-b="1"]').click(); if (e.key === "Escape") res(null) || close(); }; },
      430);
  });
}
async function confirmBox(title, msg) {
  return new Promise((res) => {
    dialog(title, `<div style="padding:4px 2px;line-height:1.6">${esc(msg)}</div>`,
      [["Annuler", ""], ["Confirmer", "primary", () => res(true)]],
      (h) => h.querySelector('[data-b="1"]').focus(), 460);
  });
}

/* éditeur */
async function dialogEditor(path, which, e) {
  let text = "";
  try {
    const url = `/api/${which === "local" ? "local" : "remote"}/file?path=${encodeURIComponent(path)}` + (which === "remote" ? `&site=${encodeURIComponent(S.remote.site)}` : "");
    const r = await fetch(url);
    if (!r.ok) throw new Error(await r.text());
    text = await r.text();
  } catch (err) { return fail(err, "lecture du fichier"); }
  const ro = text.length > 400000;
  dialog(`Éditer — ${path.split("/").pop()}`,
    `<div class="mono dim" style="margin-bottom:6px">${esc(path)} · ${fmt.size(text.length)}${ro ? " · lecture seule (trop volumineux)" : ""}</div>
     <textarea class="editor" id="ed" spellcheck="false" ${ro ? "readonly" : ""}>${esc(text)}</textarea>`,
    [["Fermer", ""], ["Enregistrer", "primary", async (h) => {
      const v = h.querySelector("#ed").value;
      try {
        if (which === "local") await apiPost("/local/save", { path, content: v });
        else await apiPost("/remote/save", { site: S.remote.site, path, content: v });
        toast("Enregistré.", "ok");
        which === "local" ? loadLocal(S.local.path) : loadRemote(S.remote.path);
      } catch (err) { fail(err, "enregistrement"); return false; }
    }]],
    (h) => {
      const ed = h.querySelector("#ed");
      ed.addEventListener("keydown", (ev) => {
        if ((ev.ctrlKey || ev.metaKey) && ev.key === "s") { ev.preventDefault(); h.querySelector('[data-b="1"]').click(); }
      });
    }, "1100px");
}
function dialogImage(path, url) {
  dialog("Aperçu — " + path.split("/").pop(),
    `<div class="about"><img src="${url}" style="max-width:100%;max-height:68vh;border-radius:8px;border:1px solid var(--line)"></div>`,
    [["Fermer", ""]], null, "900px");
}
function dialogMedia(path, url, kind) {
  dialog("Lecture — " + path.split("/").pop(),
    `<div style="text-align:center"><${kind} src="${url}" controls style="max-width:100%"></${kind}></div>`,
    [["Fermer", ""]], null, "800px");
}

/* ═══════════════════════════ file de transferts ═══════════════════════════ */
function renderQueue(jobs) {
  if (jobs) S.queue.jobs = jobs;
  const list = $("#list-queue");
  const j = S.queue.jobs;
  const head = `<div class="col">Nom</div><div class="col">État</div><div class="col">Progression</div>`;
  $("#cols-queue").innerHTML = `<div class="col" style="grid-column:1">Nom</div><div class="col">État</div><div class="col">Progression</div>`;
  const rows = j.map((t) => {
    const pct = t.size ? Math.min(100, (t.transferred / t.size) * 100) : 0;
    const name = (t.remote || "").split("/").pop() || t.local.split("/").pop();
    const dir = t.direction === "up" ? "↑" : "↓";
    const st = t.state === "active" ? `${fmt.speed(t.speed)} · ${fmt.dur((Date.now() - t.started) / 1000)}` :
      t.state === "done" ? "terminé" : t.state === "error" ? "échec" : t.state === "paused" ? "pause" : t.state === "cancelled" ? "annulé" : "en attente";
    const cls = t.state === "done" ? "ok" : t.state === "error" ? "err" : "";
    return `<div class="r" data-id="${esc(t.id)}" title="${esc(t.remote)}">
      <div class="c name"><span>${dir}</span><span>${esc(name)}</span></div>
      <div class="c dim">${esc(st)}</div>
      <div class="c"><div class="bar ${cls}"><i style="width:${pct.toFixed(1)}%"></i></div>
      <div class="dim" style="font-size:10.5px">${fmt.size(t.transferred)} / ${fmt.size(t.size)}</div></div></div>`;
  }).join("");
  list.innerHTML = rows || `<div class="empty">File vide.<br>Sélectionnez puis Ctrl+Entrée.</div>`;
  const s = S.queue.stats || {};
  const badge = $("#queue-badge");
  badge.textContent = String((s.pending || 0) + (s.active || 0));
  badge.style.display = badge.textContent === "0" ? "none" : "";
  $("#q-stats").innerHTML = `${s.active || 0} en cours · ${s.pending || 0} en attente · ${s.done || 0} terminés · ${s.failed || 0} échoués` +
    (s.bytes_per_sec ? ` · <b>${fmt.speed(s.bytes_per_sec)}</b>` : "") +
    (s.paused ? ` · <span class="warn">en pause</span>` : "");
  $("#st-speed").innerHTML = s.bytes_per_sec ? `débit ${fmt.speed(s.bytes_per_sec)}` : "";
}
listQueueEvents();
function listQueueEvents() {
  $("#list-queue").addEventListener("dblclick", (ev) => {
    const row = ev.target.closest(".r"); if (!row) return;
    const t = S.queue.jobs.find((x) => x.id === row.dataset.id); if (!t) return;
    if (t.state === "error") toast(t.error, "err", "Échec du transfert");
    else goto(t.direction === "up" ? "local" : "remote", t.direction === "up" ? t.local : t.remote.replace(/\/[^/]*$/, ""));
  });
  $("#list-queue").addEventListener("contextmenu", (ev) => {
    ev.preventDefault();
    const row = ev.target.closest(".r");
    const m = $("#context"); m.innerHTML = "";
    const add = (label, fn) => m.appendChild(el("div", { class: "mi", onclick: () => { closeMenu(); fn(); } }, el("span", {}, label)));
    if (row) {
      const id = row.dataset.id;
      const t = S.queue.jobs.find((x) => x.id === id);
      if (t && t.state === "error") add("Réessayer", () => apiPost("/transfers/retry", { id }).then(refreshQueue).catch(fail));
      if (t && (t.state === "waiting")) add("Priorité haute", () => apiPost("/transfers/priority", { id, priority: "high" }).then(refreshQueue));
      if (t && t.state !== "done") add(t.state === "paused" ? "Reprendre" : "Mettre en pause", () => apiPost("/transfers/pause", { id }).then(refreshQueue));
      if (t && t.state !== "done") add("Annuler", () => apiPost("/transfers/cancel", { id }).then(refreshQueue));
    }
    add("Tout effacer (terminés)", () => apiPost("/transfers/clear", { what: "done" }).then(refreshQueue));
    add("Réessayer les échecs", () => apiPost("/transfers/retry", { id: "all" }).then(refreshQueue));
    m.style.left = Math.min(ev.clientX, innerWidth - 240) + "px";
    m.style.top = ev.clientY + "px";
    m.classList.remove("hidden");
  });
  $("#q-pause").onclick = () => apiPost("/transfers/pause-all", { paused: !(S.queue.stats && S.queue.stats.paused) }).then(refreshQueue).catch(fail);
  $("#q-clear").onclick = () => apiPost("/transfers/clear", { what: "done" }).then(refreshQueue).catch(fail);
  $("#q-more").onclick = queueDialog;
}
async function refreshQueue() {
  try { const d = await apiGet("/transfers"); renderQueue(d.jobs.filter((j) => j.state !== "cancelled")); S.queue.stats = d.stats; renderQueue(); } catch (e) { }
}
function queueDialog() {
  const cols = `<tr><th data-k="name">Nom</th><th data-k="dir">Sens</th><th data-k="state">État</th><th data-k="size">Taille</th><th data-k="prog">Progression</th><th data-k="spd">Vitesse</th><th data-k="site">Site</th></tr>`;
  dialog("File des transferts", `<div style="display:flex;gap:8px;margin-bottom:8px">
      <button class="btn mini" id="qz-pause">⏸ Pause globale</button>
      <button class="btn mini" id="qz-retry">↻ Réessayer les échecs</button>
      <button class="btn mini" id="qz-clear">✕ Effacer terminés</button>
      <span class="grow"></span><span class="dim" id="qz-sum"></span></div>
      <table class="tb" id="qz-tb">${cols}<tbody></tbody></table>`,
    [["Fermer", ""]], (h) => {
      const fill = () => {
        h.querySelector("#qz-tb tbody").innerHTML = S.queue.jobs.map((t) => `<tr>
          <td>${esc((t.remote || t.local).split("/").pop())}</td>
          <td>${t.direction === "up" ? "↑ envoi" : "↓ réception"}</td>
          <td class="${t.state === "error" ? "err" : t.state === "done" ? "ok" : ""}">${esc(t.state)}${t.error ? ` <span class="dim" title="${esc(t.error)}">ⓘ</span>` : ""}</td>
          <td class="num">${fmt.size(t.size)}</td>
          <td>${(t.size ? Math.min(100, t.transferred / t.size * 100) : 0).toFixed(0)}%</td>
          <td class="num">${t.state === "active" ? fmt.speed(t.speed) : ""}</td>
          <td class="dim">${esc(t.site_name || "")}</td></tr>`).join("") || `<tr><td colspan="7" class="dim">Aucun transfert.</td></tr>`;
        h.querySelector("#qz-sum").textContent = `actifs ${S.queue.stats.active || 0} · attente ${S.queue.stats.pending || 0} · terminés ${S.queue.stats.done || 0} · échecs ${S.queue.stats.failed || 0}`;
      };
      fill();
      onClose = () => { };
      h.querySelector("#qz-pause").onclick = () => apiPost("/transfers/pause-all", { paused: !(S.queue.stats && S.queue.stats.paused) }).then(refreshQueue).then(fill);
      h.querySelector("#qz-retry").onclick = () => apiPost("/transfers/retry", { id: "all" }).then(refreshQueue).then(fill);
      h.querySelector("#qz-clear").onclick = () => apiPost("/transfers/clear", { what: "done" }).then(refreshQueue).then(fill);
    }, "1040px");
}

/* ═══════════════════════════ connexions ═══════════════════════════ */
function connBadge() {
  const c = Object.values(S.connected).filter((x) => x.connected);
  if (!c.length) return `<span class="dim">aucune connexion</span>`;
  return c.map((x) => `<b>${esc(x.proto || "FTP")}</b> ${esc(x.host)} <span class="dim">${x.live}/${x.max}</span>`).join(" &nbsp;·&nbsp; ");
}
function renderConnections(map) {
  S.connected = map || S.connected;
  $("#st-conns").innerHTML = connBadge();
  if (map && S.remote.site) S.remote.connected = !!(map[S.remote.site] && map[S.remote.site].connected);
  renderSiteTree();
}
async function connectTo(siteId) {
  try {
    const d = await apiPost("/connect", { site_id: siteId });
    S.remote.site = siteId;
    S.remote.connected = true;
    toast("Connecté à " + (d.host || ""), "ok", "Connexion");
    await loadRemote(d.server_dir || "/");
    refreshStatus();
    return true;
  } catch (e) { fail(e, "connexion"); return false; }
}
async function quickConnect() {
  const host = $("#qc-host").value.trim();
  if (!host) return toast("Saisissez une adresse.", "warn");
  const body = {
    host, user: $("#qc-user").value, password: $("#qc-pass").value,
    port: $("#qc-port").value ? +$("#qc-port").value : null,
    proto: $("#qc-proto").value,
  };
  try {
    const d = await apiPost("/connect", body);
    S.remote.site = d.site_id;
    S.remote.connected = true;
    toast("Connecté.", "ok", "Connexion");
    await loadRemote("/");
    refreshStatus();
    loadRecents();
  } catch (e) { fail(e, "connexion rapide"); }
}
async function disconnectCurrent() {
  if (!S.remote.site) return;
  try {
    await apiPost("/disconnect", { id: S.remote.site });
    S.remote.connected = false;
    S.remote.entries = [];
    renderPane("remote");
    toast("Déconnecté.", "ok");
    refreshStatus();
  } catch (e) { fail(e); }
}
async function reconnect() {
  if (!S.remote.site) return;
  try { await apiPost("/connect", { site_id: S.remote.site }); toast("Reconnecté.", "ok"); await loadRemote(); } catch (e) { fail(e); }
}

/* ═══════════════════════════ gestionnaire de sites ═══════════════════════════ */
function renderSiteTree() {
  const tree = $("#sites-tree");
  if (!tree) return;
  const byGroup = {};
  for (const s of S.sites) {
    const g = s.group || "";
    (byGroup[g] = byGroup[g] || []).push(s);
  }
  const groups = Object.keys(byGroup).filter(Boolean).sort();
  const html = [];
  const rootNode = (s) => {
    const live = S.connected[s.id] && S.connected[s.id].connected;
    return `<div class="n ${S.selectedSite === s.id ? "sel" : ""} ${live ? "live" : ""}" data-site="${esc(s.id)}"><span class="dot"></span>${esc(s.name)}<span class="dim" style="margin-left:auto">${esc(s.proto || "FTP")}</span></div>`;
  };
  html.push(...(byGroup[""] || []).sort((a, b) => a.name.localeCompare(b.name)).map(rootNode));
  for (const g of groups) {
    const parts = g.split("/");
    html.push(`<div class="n g"><span>▾</span>${esc(parts[parts.length - 1])}</div>`);
    html.push(...byGroup[g].sort((a, b) => a.name.localeCompare(b.name)).map((s) => `<div class="n child ${S.selectedSite === s.id ? "sel" : ""} ${S.connected[s.id]?.connected ? "live" : ""}" data-site="${esc(s.id)}"><span class="dot"></span>${esc(s.name)}<span class="dim" style="margin-left:auto">${esc(s.proto || "FTP")}</span></div>`));
    const sub = groups.filter((x) => x.startsWith(g + "/"));
    for (const sg of sub) {
      const nm = sg.slice(g.length + 1).split("/")[0];
      html.push(`<div class="n g child"><span>▸</span>${esc(nm)}</div>`);
      html.push(...(byGroup[sg] || []).map((s) => `<div class="n child" style="padding-left:38px" data-site="${esc(s.id)}"><span class="dot"></span>${esc(s.name)}</div>`));
    }
  }
  tree.innerHTML = html.join("") || `<div class="empty">Aucun site. Créez-en un.</div>`;
  tree.querySelectorAll("[data-site]").forEach((n) => {
    n.onclick = () => { S.selectedSite = n.dataset.site; renderSiteTree(); siteForm(n.dataset.site); };
    n.ondblclick = () => connectTo(n.dataset.site);
  });
}
function siteForm(id) {
  const s = id ? (S.sites.find((x) => x.id === id) || {}) : { name: "", host: "", port: null, proto: "ftp", login: "normal", user: "", password: "", keyfile: "", remote_dir: "/", local_dir: "", tls: "if", max_connections: null, limit_connections: false, comment: "", disable_keepalive: false, group: "", virtual_host: "", pre_commands: [], post_commands: [], site_chmod: true, site_chown: false };
  const host = s.host || "";
  return `<div id="site-form">
    <fieldset><legend>Général</legend>
      <div class="field"><label>Nom</label><input name="name" value="${esc(s.name)}" placeholder="Mon serveur"></div>
      <div class="field"><label>Groupe</label><input name="group" value="${esc(s.group || "")}" placeholder="Clients/Pro (facultatif)"></div>
      <div class="field"><label>Hôte</label><input name="host" value="${esc(host)}" placeholder="ftp.example.org"></div>
      <div class="field"><label>Port</label><input name="port" value="${esc(s.port ?? "")}" placeholder="21"></div>
      <div class="field"><label>Protocole</label><select name="proto">
        ${["ftp", "ftpes", "ftps", "sftp"].map((p) => `<option value="${p}" ${s.proto === p ? "selected" : ""}>${p.toUpperCase()}</option>`).join("")}</select></div>
      <div class="field"><label>Chiffrement</label><select name="tls">
        ${[["plain", "Clair, sans TLS"], ["if", "TLS si disponible"], ["require", "Exiger un TLS explicite"], ["pfs", "Exiger TLS + secret parfait"]].map(([v, l]) => `<option value="${v}" ${s.tls === v ? "selected" : ""}>${l}</option>`).join("")}</select></div>
    </fieldset>
    <fieldset><legend>Authentification</legend>
      <div class="field"><label>Type</label><select name="login">
        ${[["normal", "Normale"], ["anonymous", "Anonyme"], ["interactive", "Interactive"], ["keyonly", "Clé seulement"]].map(([v, l]) => `<option value="${v}" ${(s.login || "normal") === v ? "selected" : ""}>${l}</option>`).join("")}</select></div>
      <div class="field"><label>Utilisateur</label><input name="user" value="${esc(s.user || "")}"></div>
      <div class="field"><label>Mot de passe</label><input name="password" type="text" value="${esc(s.password || "")}" autocomplete="off"></div>
      <div class="field"><label>Clé privée (SFTP)</label><input name="keyfile" value="${esc(s.keyfile || "")}" placeholder="~/.ssh/id_ed25519"><div class="hint">la passphrase n'est pas requise si la clé est sans mot de passe</div></div>
    </fieldset>
    <fieldset><legend>Répertoires</legend>
      <div class="field"><label>Répertoire distant</label><input name="remote_dir" value="${esc(s.remote_dir || "/")}"></div>
      <div class="field"><label>Répertoire local</label><input name="local_dir" value="${esc(s.local_dir || "")}"></div>
    </fieldset>
    <fieldset><legend>Avancé</legend>
      <div class="field"><label>Limiter les connexions</label><div class="inline"><input type="checkbox" name="limit_connections" ${s.limit_connections ? "checked" : ""}> <input name="max_connections" value="${esc(s.max_connections ?? 10)}" style="width:70px"> <span class="dim">max. simultanées</span></div></div>
      <div class="field"><label>Keep-alive</label><label><input type="checkbox" name="disable_keepalive" ${s.disable_keepalive ? "checked" : ""}> désactiver le NOOP périodique</label></div>
      <div class="field"><label>Hôte virtuel</label><input name="virtual_host" value="${esc(s.virtual_host || "")}" placeholder="ex. site.example.com (commande HOST)"></div>
      <div class="field"><label>chmod / chown via SITE</label><label><input type="checkbox" name="site_chmod" ${s.site_chmod !== false ? "checked" : ""}> chmod &nbsp; <input type="checkbox" name="site_chown" ${s.site_chown ? "checked" : ""}> chown</label></div>
      <div class="field wide"><label>Commandes avant / après connexion</label>
        <div class="two"><textarea name="pre" placeholder="COMMANDES PRÉ-ENVOI">${esc((s.pre_commands || []).join("\n"))}</textarea><textarea name="post" placeholder="COMMANDES POST-CONNEXION">${esc((s.post_commands || []).join("\n"))}</textarea></div></div>
      <div class="field wide"><label>Commentaire</label><textarea name="comment">${esc(s.comment || "")}</textarea></div>
    </fieldset>
    <div style="display:flex;gap:8px">
      <button class="btn primary" id="site-save">Enregistrer</button>
      <button class="btn" id="site-test">Tester la connexion</button>
      <span class="grow"></span>
      <button class="btn" id="site-dup">Dupliquer</button>
      <button class="btn" id="site-del">Supprimer</button>
    </div>
  </div>`;
}
function siteManagerDialog() {
  dialog("Gestionnaire de sites", `<div class="grid2">
      <div>
        <div style="display:flex;gap:4px;margin-bottom:6px">
          <button class="btn mini" id="ns">＋ Site</button>
          <button class="btn mini" id="ng">🗀 Groupe</button>
          <button class="btn mini" id="imp">Import XML</button>
          <button class="btn mini" id="exp">Export XML</button>
        </div>
        <div class="tree" id="sites-tree" style="height:340px"></div>
        <div class="dim" style="font-size:11.5px;margin-top:6px">double-clic = se connecter</div>
      </div>
      <div id="site-pane">${siteForm(null)}</div>
    </div>`,
    [["Fermer", ""], ["Se connecter", "primary", () => { S.selectedSite && connectTo(S.selectedSite); return false; }]],
    async (h) => {
      await loadSites();
      renderSiteTree();
      if (S.selectedSite) { const f = $("#site-pane"); f.innerHTML = siteForm(S.selectedSite); bindForm(f); }
      $("#ns").onclick = () => { S.selectedSite = null; $("#site-pane").innerHTML = siteForm(null); bindForm($("#site-pane")); };
      $("#ng").onclick = async () => {
        const g = await promptBox("Nouveau groupe", "Nom du groupe :");
        if (!g) return;
        const s = { id: "", name: "(nouveau)", group: g, host: "", proto: "ftp", login: "normal" };
        apiPost("/sites/save", s).then(() => { loadSites(); renderSiteTree(); });
      };
      $("#imp").onclick = () => importDialog();
      $("#exp").onclick = () => window.open("/api/sites/export", "_blank");
      $("#site-pane").addEventListener("submit", (e) => e.preventDefault());
      bindForm($("#site-pane"));
    }, "1000px");
}
function bindForm(root) {
  const g = (n) => root.querySelector(`[name="${n}"]`);
  const save = async () => {
    const site = {
      id: S.selectedSite || "",
      name: g("name")?.value.trim() || g("host")?.value.trim() || "sans nom",
      group: g("group")?.value.trim() || "",
      host: g("host")?.value.trim() || "",
      port: g("port")?.value ? +g("port").value : null,
      proto: g("proto")?.value || "ftp",
      login: g("login")?.value || "normal",
      user: g("user")?.value || "",
      password: g("password")?.value || "",
      keyfile: g("keyfile")?.value || "",
      remote_dir: g("remote_dir")?.value || "/",
      local_dir: g("local_dir")?.value || "",
      tls: g("tls")?.value || "if",
      limit_connections: !!g("limit_connections")?.checked,
      max_connections: g("max_connections")?.value ? +g("max_connections").value : null,
      disable_keepalive: !!g("disable_keepalive")?.checked,
      virtual_host: g("virtual_host")?.value || "",
      site_chmod: !!g("site_chmod")?.checked,
      site_chown: !!g("site_chown")?.checked,
      comment: g("comment")?.value || "",
      pre_commands: (g("pre")?.value || "").split("\n").map((x) => x.trim()).filter(Boolean),
      post_commands: (g("post")?.value || "").split("\n").map((x) => x.trim()).filter(Boolean),
    };
    if (!site.host) throw toast("Le nom d'hôte est obligatoire.", "warn");
    try {
      const d = await apiPost("/sites/save", site);
      S.selectedSite = d.id || S.selectedSite;
      await loadSites();
      renderSiteTree();
      toast("Site enregistré.", "ok");
    } catch (e) { fail(e); }
  };
  const sb = root.querySelector("#site-save"); if (sb) sb.onclick = save;
  const tb = root.querySelector("#site-test");
  if (tb) tb.onclick = async () => { await save(); if (S.selectedSite) connectTo(S.selectedSite); };
  const db = root.querySelector("#site-del");
  if (db) db.onclick = async () => {
    if (!S.selectedSite) return;
    if (!await confirmBox("Supprimer ce site ?", "La suppression est définitive.")) return;
    await apiPost("/sites/delete", { id: S.selectedSite });
    S.selectedSite = null;
    await loadSites(); renderSiteTree();
    $("#site-pane").innerHTML = siteForm(null); bindForm($("#site-pane"));
  };
  const bp = root.querySelector("#site-dup");
  if (bp) bp.onclick = async () => { if (!S.selectedSite) return; const d = await apiPost("/sites/copy", { id: S.selectedSite }); await loadSites(); renderSiteTree(); toast("Site dupliqué.", "ok"); };
}
async function loadSites() {
  try { const d = await apiGet("/sites"); S.sites = d.sites || []; const c = d.connected || []; S.connected = Object.fromEntries(c.map((id) => [id, { connected: true }])); } catch (e) { fail(e); }
}
function importDialog() {
  dialog("Importer un gestionnaire de sites", `<div class="field wide"><label>Fichier sitemanager.xml</label>
      <input type="file" id="fx" accept=".xml,.XML">
      <div class="hint">Le format natif de FileZilla est accepté tel quel (groupes, mots de passe encodés, clés).</div></div>
    <div class="field wide"><label>ou coller le XML</label><textarea id="fx2" class="editor" style="min-height:120px" placeholder="&lt;FileZilla3 ...&gt;"></textarea></div>`,
    [["Annuler", ""], ["Importer", "primary", async (h) => {
      const t2 = h.querySelector("#fx2").value.trim();
      const f = h.querySelector("#fx").files[0];
      let xml = t2;
      if (!xml && f) xml = await f.text();
      if (!xml) { toast("Aucun XML fourni.", "warn"); return false; }
      try { const d = await apiPost("/sites/import", { xml }); toast(`${d.imported} site(s) importé(s).`, "ok"); await loadSites(); renderSiteTree(); } catch (e) { fail(e, "import"); return false; }
    }]], (h) => { }, "620px");
}

/* ═══════════════════════════ réglages ═══════════════════════════ */
const SET_TABS = [
  ["general", "Général"], ["transfers", "Transferts"], ["ftp", "FTP"], ["net", "Réseau"], ["tls", "TLS/SFTP"], ["ui", "Interface"], ["logs", "Journaux"], ["proxy", "Proxies"], ["shortcuts", "Raccourcis"],
];
function settingsDialog(tab = "general") {
  const s = S.settings;
  const body = Object.fromEntries(SET_TABS.map(([k]) => [k, `<div class="tabbody ${k === tab ? "" : "hidden"}" data-body="${k}">${tabBody(k, s)}</div>`]));
  dialog("Réglages", `<div class="tabsx"><div class="tabbar">${SET_TABS.map(([k, l]) => `<button data-tab="${k}" class="${k === tab ? "active" : ""}">${l}</button>`).join("")}</div>
    <div style="min-height:420px">${Object.values(body).join("")}</div></div>`,
    [["Annuler", ""], ["Enregistrer", "primary", (h) => { saveSettings(h); }]],
    (h) => {
      h.querySelectorAll("[data-tab]").forEach((b) => b.onclick = () => {
        h.querySelectorAll("[data-tab]").forEach((x) => x.classList.remove("active"));
        b.classList.add("active");
        h.querySelectorAll("[data-body]").forEach((x) => x.classList.toggle("hidden", x.dataset.body !== b.dataset.tab));
      });
    }, "880px");
}
function tabBody(k, s) {
  const F = (label, name, value, type = "text", hint = "") => `<div class="field"><label>${esc(label)}</label><input name="${name}" type="${type}" value="${esc(value ?? "")}">${hint ? `<div class="hint">${hint}</div>` : ""}</div>`;
  const B = (label, name, value) => `<div class="field"><label>${esc(label)}</label><label><input type="checkbox" name="${name}" ${value ? "checked" : ""}> activé</label></div>`;
  const S1 = (label, name, value, opts) => `<div class="field"><label>${esc(label)}</label><select name="${name}">${opts.map(([v, l]) => `<option value="${v}" ${String(value) === String(v) ? "selected" : ""}>${l}</option>`).join("")}</select></div>`;
  const g = getPath(s, k);
  switch (k) {
    case "general":
      return `<fieldset><legend>Interface</legend>
        ${S1("Langue", "general.language", g?.language || "fr", [["fr", "Français"], ["en", "English"]])}
        ${S1("Thème", "general.theme", g?.theme || "dark", [["dark", "Sombre"], ["light", "Clair"]])}
        ${S1("Taille de police", "general.font_size", g?.font_size || 13, [[12, "Petit (12px)"], [13, "Normal (13px)"], [14, "Grand (14px)"], [15, "Très grand (15px)"]])}
        ${S1("Double-clic", "general.double_click", g?.double_click || "transfer", [["transfer", "Transférer"], ["view", "Voir / éditer"]])}
        ${B("Afficher les fichiers cachés (local)", "general.show_hidden_local", g?.show_hidden_local)}
        ${B("Confirmer avant suppression", "general.confirm_delete", g?.confirm_delete)}
        ${B("Notifier à la fin des transferts", "general.notify_finish", g?.notify_finish)}</fieldset>`;
    case "transfers":
      return `<fieldset><legend>File d'attente</legend>
        ${F("Transferts simultanés", "transfers.max_parallel", g?.max_parallel, "number")}
        ${S1("Si le fichier existe", "transfers.overwrite", g?.overwrite || "ask", [["ask", "Demander (remplacer)"], ["overwrite", "Toujours remplacer"], ["skip", "Ignorer"], ["newer", "Si plus récent"], ["rename", "Renommer + suffixe"]])}
        ${B("Reprendre automatiquement", "transfers.auto_resume", g?.auto_resume)}
        ${B("Conserver les horodatages", "transfers.preserve_mtime", g?.preserve_mtime)}
        ${F("Tentatives après échec", "transfers.retries", g?.retries, "number")}
        ${F("Délai avant nouvelle tentative (s)", "transfers.retry_delay", g?.retry_delay, "number")}
        ${F("Taille de tampon (Kio)", "transfers.chunk_size", g?.chunk_size, "number")}</fieldset>
        <fieldset><legend>Limites de débit (Kio/s, 0 = illimité)</legend>
        ${F("Global", "transfers.limit_total", g?.limit_total, "number")}
        ${F("Envoi", "transfers.limit_upload", g?.limit_upload, "number")}
        ${F("Réception", "transfers.limit_download", g?.limit_download, "number")}</fieldset>
        <fieldset><legend>Mode texte (ASCII)</legend>
        ${B("Detecter selon l'extension", "transfers.ascii_detect", g?.ascii_detect)}
        <div class="field wide"><label>Extensions ASCII</label><textarea name="transfers.ascii_extensions">${esc((g?.ascii_extensions || []).join(" "))}</textarea></div></fieldset>`;
    case "ftp":
      return `<fieldset><legend>Transfert de données</legend>
        ${B("Mode passif (recommandé)", "ftp.passive", g?.passive)}
        ${B("Utiliser EPSV/EPRT", "ftp.use_epsv", g?.use_epsv)}
        ${B("Utiliser MLSD si disponible", "ftp.use_mlsd", g?.use_mlsd)}
        ${B("Envoyer PRET", "ftp.use_pret", g?.use_pret)}
        ${F("Port actif minimum", "ftp.active_port_min", g?.active_port_min || 0, "number")}
        ${F("Port actif maximum", "ftp.active_port_max", g?.active_port_max || 0, "number", "les deux à 0 = attribué automatiquement")}
        ${F("Connexions max par site", "ftp.max_connections", g?.max_connections, "number")}</fieldset>
        <fieldset><legend>Conversation</legend>
        ${B("Envoyer CLNT", "ftp.send_clnt", g?.send_clnt)}
        ${F("Valeur de CLNT", "ftp.clnt_string", g?.clnt_string || "ZillaRS-0.1")}
        ${B("Opter pour UTF-8", "ftp.use_utf8", g?.use_utf8)}
        ${B("Commande HOST", "ftp.use_host", g?.use_host)}
        ${B("SIZE pour connaître la taille", "ftp.use_size", g?.use_size)}
        ${B("MDTM pour les dates", "ftp.use_mdtm", g?.use_mdtm)}
        ${B("SITE CHMOD", "ftp.site_chmod", g?.site_chmod)}</fieldset>`;
    case "net":
      return `<fieldset><legend>Délais</legend>
        ${F("Connexion (s)", "net.connect_timeout", g?.connect_timeout, "number")}
        ${F("Réponse serveur (s)", "net.response_timeout", g?.response_timeout, "number")}
        ${F("Inactivité de données (s, 0 = aucune)", "net.data_timeout", g?.data_timeout, "number")}
        ${F("Keep-alive après (s)", "net.keepalive_idle", g?.keepalive_idle, "number")}</fieldset>
        <fieldset><legend>Pile réseau</legend>
        ${S1("Famille d'adresses", "net.ip_family", g?.ip_family || "auto", [["auto", "Automatique"], ["v4", "IPv4 uniquement"], ["v6", "IPv6 uniquement"]])}
        ${F("IP externe (mode actif)", "net.external_ip", g?.external_ip || "", "text", "normalement inutile en mode passif")}</fieldset>`;
    case "tls":
      return `<fieldset><legend>TLS</legend>
        ${B("Vérifier les certificats", "tls.verify_cert", g?.verify_cert)}
        ${B("Autoriser le repli en clair", "tls.fallback", g?.fallback)}
        ${S1("Version minimale", "tls.min_version", g?.min_version || "1.2", [["1.2", "TLS 1.2"], ["1.3", "TLS 1.3 seulement"]])}
        </fieldset><fieldset><legend>SFTP</legend>
        ${F("Fichier d'identité par défaut", "sftp.identity_file", g?.identity_file || "", "text", "ex. ~/.ssh/id_ed25519")}
        ${B("Demander pour les clés hôtes inconnues", "sftp.ask_hostkey", s.sftp?.ask_hostkey)}
        ${B("Suivre les liens symboliques", "sftp.follow_symlinks", s.sftp?.follow_symlinks)}</fieldset>`;
    case "ui":
      return `<fieldset><legend>Panneaux</legend>
        ${B("Afficher les barres d'outils", "ui.show_toolbar", g?.show_toolbar)}
        ${B("Afficher le journal", "ui.show_message_pane", g?.show_message_pane)}
        ${B("Afficher le volet des transferts", "ui.show_transfer_pane", g?.show_transfer_pane)}
        ${B("Afficher la barre de statut", "ui.show_statusbar", g?.show_statusbar)}
        <div class="field wide"><label>Colonnes locales</label><input name="ui.local_columns" value="${esc((g?.local_columns || []).join(","))}" class="mono"></div>
        <div class="field wide"><label>Colonnes distantes</label><input name="ui.remote_columns" value="${esc((g?.remote_columns || []).join(","))}" class="mono"></div>
        <div class="hint">Colonnes possibles : name,size,type,modified,perm,user,group</div></fieldset>`;
    case "logs":
      return `<fieldset><legend>Journal</legend>
        ${S1("Niveau", "logs.level", g?.level || "normal", [["normal", "Normal"], ["verbose", "Détaillé (commandes)"]])}
        ${B("Écrire dans un fichier", "logs.to_file", g?.to_file)}
        ${F("Emplacement", "logs.file_path", g?.file_path || "", "text", "vide = <dossier de config>/logs/zilla.log")}
        ${F("Lignes conservées", "logs.memory_lines", g?.memory_lines, "number")}</fieldset>`;
    case "proxy":
      return `<fieldset><legend>Proxy</legend>
        ${B("Utiliser un proxy", "proxy.enabled", g?.enabled)}
        ${S1("Type", "proxy.kind", g?.kind || "none", [["none", "Aucun"], ["http", "HTTP/CONNECT"], ["socks5", "SOCKS5"]])}
        ${F("Hôte", "proxy.host", g?.host || "")}
        ${F("Port", "proxy.port", g?.port || "", "number")}
        ${F("Utilisateur", "proxy.user", g?.user || "")}
        ${F("Mot de passe", "proxy.password", g?.password || "", "password")}
        ${B("Résoudre les noms via le proxy", "proxy.resolve_dns", g?.resolve_dns)}
        <div class="hint">Le mode passif est forcé sur une session FTP via proxy HTTP.</div></fieldset>`;
    case "shortcuts":
      return `<fieldset><legend>Raccourcis disponibles</legend><div class="kb-list">
        ${Object.entries(KEYS).map(([k, v]) => `<div><span>${esc(v)}</span><span class="kbd">${esc(k)}</span></div>`).join("")}
      </div></fieldset>`;
  }
  return "";
}
function getPath(o, k) {
  return k === "sftp" ? o.sftp : k === "tls" ? o.tls : o[k];
}
function parsePatch(root) {
  const patch = {};
  root.querySelectorAll("input,select,textarea").forEach((i) => {
    if (!i.name) return;
    let v;
    if (i.type === "checkbox") v = i.checked;
    else if (i.type === "number") v = i.value === "" ? 0 : +i.value;
    else v = i.value;
    if (i.name === "transfers.ascii_extensions") v = v.split(/[\s,]+/).filter(Boolean);
    if (i.name.startsWith("ui.local_columns") || i.name.startsWith("ui.remote_columns")) v = v.split(",").map((x) => x.trim()).filter(Boolean);
    const parts = i.name.split(".");
    let cur = patch;
    for (let n = 0; n < parts.length - 1; n++) cur = cur[parts[n]] = cur[parts[n]] || {};
    cur[parts[parts.length - 1]] = v;
  });
  return patch;
}
async function saveSettings(root) {
  const patch = parsePatch(root);
  const cachésAvant = !!(S.settings.general || {}).show_hidden_local;
  try {
    S.settings = await apiPost("/settings", patch);
    applySettings(true);
    /* les cachés, ça ne se trie pas à l'écran : c'est le listing qui les fait apparaître */
    if (!!(S.settings.general || {}).show_hidden_local !== cachésAvant) await loadLocal(S.local.path).catch(() => { });
    toast("Réglages enregistrés.", "ok");
  } catch (e) { fail(e, "réglages"); return false; }
}
let visuel = null;
function visuelSig() {
  const g = S.settings.general || {}, u = S.settings.ui || {};
  return JSON.stringify([g.theme, g.font_size, g.show_hidden_local, g.show_hidden_remote,
    u.show_toolbar, u.show_message_pane, u.show_transfer_pane, u.show_statusbar, S.log.verbose, S.cols,
    S.local.sort, S.remote.sort, S.local.dirsFirst, S.remote.dirsFirst]);
}
function applySettings(force = false) {
  const g = S.settings.general || {}, u = S.settings.ui || {};
  document.body.className = (g.theme === "light" ? "theme-light" : "theme-dark");
  document.documentElement.style.setProperty("--fs", (g.font_size || 13) + "px");
  $("#logwrap").classList.toggle("hidden", u.show_message_pane === false);
  $("#toolbar").style.display = u.show_toolbar === false ? "none" : "";
  const q = $("#pane-queue");
  if (q) q.style.display = u.show_transfer_pane === false ? "none" : "";
  $("#st-speed").parentElement.style.display = u.show_statusbar === false ? "none" : "";
  S.log.verbose = S.settings.logs?.level === "verbose";
  $("#log-verbose").checked = S.log.verbose;
  /* Le reste se joue en CSS et dans les listes déjà sous la main : refaire
     les deux panneaux à chaque message du serveur (ou toutes les 12 s à
     la revue de statut) remplace les lignes sous le curseur, et un
     double-clic qui tombe là-dedans ne trouve plus sa proie. On ne
     rerend que si le visuel a réellement remué. */
  const sig = visuelSig();
  if (force || sig !== visuel) { visuel = sig; renderPane("local"); renderPane("remote"); }
}

/* ═══════════════════════════ recherche ═══════════════════════════ */
function searchDialog(scope = "local") {
  const root = scope === "local" ? S.local.path : S.remote.path;
  dialog("Rechercher", `<div style="display:flex;gap:8px;margin-bottom:10px">
      <select id="sc-scope" style="width:120px"><option value="local" ${scope === "local" ? "selected" : ""}>Local</option><option value="remote" ${scope === "remote" ? "selected" : ""}>Distant</option></select>
      <input id="sc-root" placeholder="racine" value="${esc(root)}" style="flex:1">
      <input id="sc-pat" placeholder="motif du nom…" style="flex:1">
      <button class="btn primary" id="sc-go">Chercher</button></div>
      <table class="tb" id="sc-tb"><tr><th>Nom</th><th>Taille</th><th>Date</th><th>Chemin</th></tr><tbody><tr><td colspan="4" class="dim">saisissez un motif…</td></tr></tbody></table>
      <div class="dim" id="sc-sum" style="margin-top:6px"></div>`,
    [["Fermer", ""], ["Ajouter à la file", "primary", addAll]], (h) => { }, "900px");
  const go = async () => {
    const sc = $("#sc-scope").value, r = $("#sc-root").value, p = $("#sc-pat").value;
    try {
      const found = sc === "local"
        ? await apiGet(`/local/search?root=${encodeURIComponent(r)}&pattern=${encodeURIComponent(p)}&max=400`)
        : await apiGet(`/remote/search?site=${encodeURIComponent(S.remote.site || "")}&root=${encodeURIComponent(r)}&pattern=${encodeURIComponent(p)}&max=400`);
      const d = normEntries(found || []);
      window._search = { data: d, scope: sc };
      const tb = $("#sc-tb tbody");
      tb.innerHTML = (d || []).map((e, i) => `<tr data-i="${i}"><td>${esc(e.name)}</td><td class="num">${fmt.size(e.size)}</td><td>${fmt.date(e.mtime)}</td><td class="dim mono">${esc(e.path || "")}</td></tr>`).join("") || `<tr><td colspan="4" class="dim">aucun résultat</td></tr>`;
      $("#sc-sum").textContent = `${(d || []).length} résultat(s)`;
      tb.querySelectorAll("[data-i]").forEach((tr) => {
        tr.ondblclick = () => { const e = window._search.data[+tr.dataset.i]; if (window._search.scope === "local") goto("local", e.path.replace(/\/[^/]*$/, "")); else goto("remote", (e.path || "").replace(/\/[^/]*$/, "")); close(); };
        tr.onclick = () => { tb.querySelectorAll("tr").forEach((x) => x.classList.remove("sel")); tr.classList.add("sel"); };
      });
    } catch (e) { fail(e, "recherche"); }
  };
  $("#sc-go").onclick = go;
  $("#sc-pat").onkeydown = (e) => { if (e.key === "Enter") go(); };
  $("#sc-scope").onchange = () => { $("#sc-root").value = $("#sc-scope").value === "local" ? S.local.path : S.remote.path; };
  function addAll() {
    const d = window._search?.data || [];
    if (!d.length) return toast("Rien à ajouter.", "warn");
    if (window._search.scope === "remote") upload(d.map((e) => e.path), S.local.path, false);
    else download(d.map((e) => e.path), S.remote.path, false);
  }
}

/* ═══════════════════════════ signets ═══════════════════════════ */
function addBookmarkFor(which) {
  const p = PANES[which];
  const label = which === "remote" ? `${(S.remote.site ? (S.sites.find((s) => s.id === S.remote.site)?.name || S.remote.site) : "")} — ${p.path}` : p.path;
  apiPost("/bookmarks", { id: "", scope: which, label, path: p.path, site_id: which === "remote" ? S.remote.site : "" })
    .then(() => { toast("Chemin ajouté aux signets.", "ok"); loadBookmarks(); })
    .catch(fail);
}
function bookmarksDialog() {
  dialog("Signets", `<table class="tb"><tr><th>Portée</th><th>Libellé</th><th>Chemin</th><th></th></tr>
    <tbody id="bm-body"></tbody></table>
    <div style="margin-top:10px;display:flex;gap:8px">
      <button class="btn mini" id="bm-add-local">＋ Signer le chemin local</button>
      <button class="btn mini" id="bm-add-remote">＋ Signer le chemin distant</button>
      <span class="grow"></span>
      <button class="btn mini" id="bm-import">importer un fichier</button>
    </div>`,
    [["Fermer", ""]], (h) => {
      const fill = () => {
        $("#bm-body").innerHTML = S.bookmarks.map((b) => `<tr><td>${esc(b.scope)}</td><td>${esc(b.label)}</td><td class="mono dim">${esc(b.path)}</td>
          <td><button class="btn mini" data-go="${esc(b.id)}">Ouvrir</button> <button class="btn mini" data-del="${esc(b.id)}">✕</button></td></tr>`).join("") || `<tr><td colspan="4" class="dim">Aucun signet.</td></tr>`;
        $$("[data-go]", h).forEach((b) => b.onclick = () => { const x = S.bookmarks.find((y) => y.id === b.dataset.go); goto(x.scope, x.path); close(); });
        $$("[data-del]", h).forEach((b) => b.onclick = () => apiPost("/bookmarks/delete", { id: b.dataset.del }).then(() => { loadBookmarks(); fill(); }));
      };
      fill();
      $("#bm-add-local").onclick = () => { addBookmarkFor("local"); setTimeout(fill, 120); };
      $("#bm-add-remote").onclick = () => { addBookmarkFor("remote"); setTimeout(fill, 120); };
    }, "780px");
}
async function loadBookmarks() { try { S.bookmarks = await apiGet("/bookmarks"); } catch { } }

/* ═══════════════════════════ comparaison de dossiers ═══════════════════════ */
// ------------------------------------------------------------- comparaison
function compareDialog() {
  if (!S.remote.site) return toast("La comparaison demande une connexion.", "warn");
  const local = S.local.path, remote = S.remote.path;
  let d = null;
  const sel = new Set();
  let mode = "diff";
  const MX = 400;

  const label = (k) =>
    ({
      RemoteOnly: "manquant ici",
      LocalOnly: "absent du serveur",
      Different: "contenu différent",
    }[k] || k);

  const seen = (i) =>
    mode === "all"
      ? true
      : mode === "diff"
      ? i.kind !== "Same"
      : mode === "RemoteOnly"
      ? i.kind === "RemoteOnly" || i.kind === "Different"
      : i.kind === "LocalOnly" || i.kind === "Different";

  const gap = (i) =>
    i.kind === "Different"
      ? `<span class="dim">${i.remote_size > i.local_size ? "↓" : "↑"} ${fmt.size(
          Math.abs(i.remote_size - i.local_size)
        )}</span>`
      : i.kind === "RemoteOnly"
      ? "<span class='dim'>↓</span>"
      : i.kind === "LocalOnly"
      ? "<span class='dim'>↑</span>"
      : "";

  function paint(host) {
    const all = d.items || [];
    const diff = all.filter((i) => i.kind !== "Same");
    $("#cmp-same", host).textContent = d.same;
    $("#cmp-diff", host).textContent = d.different;
    $("#cmp-down", host).textContent = d.remote_only;
    $("#cmp-up", host).textContent = d.local_only;
    $("#cmp-bytes", host).innerHTML = `<span class="dim">${fmt.size(
      d.bytes_to_download
    )} à descendre · ${fmt.size(d.bytes_to_upload)} à monter</span>`;
    const list = diff.filter(seen);
    const shown = list.slice(0, MX);
    $("#cmp-b", host).innerHTML =
      shown
        .map(
          (i, n) => `<tr class="${sel.has(i.path) ? "sel" : ""}" data-n="${n}">
       <td><input type="checkbox" ${sel.has(i.path) ? "checked" : ""}></td>
       <td class="mono">${esc(i.path)}${i.dir ? " /" : ""}</td>
       <td class="num mono">${esc(fmt.size(i.local_size))}</td>
       <td class="num mono">${esc(fmt.size(i.remote_size))}</td>
       <td>${gap(i)} <span class="dim">${label(i.kind)}</span></td></tr>`
        )
        .join("") ||
      `<tr><td colspan="5" class="dim">Aucune différence : les deux arborescences concordent.</td></tr>`;
    if (list.length > MX)
      $("#cmp-b", host).innerHTML += `<tr><td colspan="5" class="dim">+ ${
        list.length - MX
      } ligne(s) de plus — filtrez pour trier.</td></tr>`;
    $$("#cmp-b tr", host).forEach((tr) => {
      const cb = $("input", tr);
      if (!cb) return;
      const it = shown[+tr.dataset.n];
      cb.onchange = () => {
        if (cb.checked) sel.add(it.path);
        else sel.delete(it.path);
        tr.classList.toggle("sel", cb.checked);
      };
      tr.onclick = (ev) => {
        if (ev.target.tagName !== "INPUT") {
          cb.checked = !cb.checked;
          cb.onchange();
        }
      };
    });
  }

  async function load(host, quiet) {
    try {
      d = await apiGet(
        "/compare?site=" +
          encodeURIComponent(S.remote.site) +
          "&local=" +
          encodeURIComponent(local) +
          "&remote=" +
          encodeURIComponent(remote)
      );
    } catch (e) {
      return fail(e, "comparaison");
    }
    sel.clear();
    if (!quiet) toast("Comparaison relancée.", "ok", "Comparaison");
    paint(host);
  }

  const pick = () => {
    const l = (d.items || []).filter((i) => i.kind !== "Same").filter(seen);
    return sel.size ? l.filter((i) => sel.has(i.path)) : l;
  };

  const go = async (host, dir) => {
    const items = pick();
    if (!items.length) {
      toast("Rien à transférer.", "warn");
      return false;
    }
    try {
      if (dir === "down") {
        const paths = items.filter((i) => i.kind !== "LocalOnly").map((i) => joinRemote(remote, i.path));
        if (!paths.length) {
          toast("Aucun élément à télécharger.", "warn");
          return false;
        }
        await apiPost("/download", { site: S.remote.site, paths, dir: local, dirs: true, base: remote });
      } else {
        const paths = items.filter((i) => i.kind !== "RemoteOnly").map((i) => joinLocal(local, i.path));
        if (!paths.length) {
          toast("Aucun élément à envoyer.", "warn");
          return false;
        }
        await apiPost("/upload", { site: S.remote.site, paths, dir: remote, dirs: true, base: local });
      }
      toast(`${items.length} ligne(s) ajoutée(s) à la file.`, "ok", "Comparaison");
      refreshQueue();
      load(host, true);
    } catch (e) {
      fail(e, "transfert");
    }
    return false;
  };

  dialog(
    "Comparaison des dossiers",
    `<div class="hint">Local <span class="mono">${esc(local)}</span> ↔ distant
       <span class="mono">${esc(remote)}</span> — on compare les deux arboresences,
      tailles et dates comprises.</div>
     <div class="sum">
       <span><b id="cmp-same">…</b> identiques</span>
       <span><b id="cmp-diff">…</b> différents</span>
       <span><b id="cmp-down">…</b> à télécharger</span>
       <span><b id="cmp-up">…</b> à envoyer</span>
       <span class="grow"></span>
       <span id="cmp-bytes"></span>
     </div>
     <div class="fil">Filtrer :
       <label><input type="radio" name="cmpf" value="diff" checked> différences</label>
       <label><input type="radio" name="cmpf" value="RemoteOnly"> à télécharger</label>
       <label><input type="radio" name="cmpf" value="LocalOnly"> à envoyer</label>
       <label><input type="radio" name="cmpf" value="all"> tout</label>
       <span class="grow"></span>
       <label><input type="checkbox" id="cmp-auto"> relire après transfert</label>
     </div>
     <table class="tb"><tr><th></th><th>chemin</th><th>locale</th><th>distante</th><th>écart</th></tr>
     <tbody id="cmp-b"></tbody></table>`,
    [
      ["Fermer", ""],
      ["Relire", "", (h) => (load(h, false), false)],
      ["Télécharger les manquants", "primary", (h) => go(h, "down")],
      ["Envoyer les manquants", "", (h) => go(h, "up")],
    ],
    (host) => {
      load(host, true);
      $$('[name="cmpf"]').forEach((r) => (r.onchange = () => ((mode = r.value), paint(host))));
    },
    900
  );
}

/* ═══════════════════════════ aide ═══════════════════════════ */
function helpDialog() {
  dialog("Aide & raccourcis", `<div style="line-height:1.7">
    <p><b>Zilla-RS</b> est un client FTP/FTPS/SFTP qui fonctionne entièrement dans le navigateur.
    Le moteur est écrit en Rust ; aucune ressource externe n'est chargée.</p>
    <h4 style="margin-bottom:4px">Prise en main</h4>
    <ul style="margin:4px 0 12px 18px;padding:0">
      <li>Saisissez une adresse dans la barre de connexion rapide, ou choisissez un site dans le gestionnaire (<span class="kbd">Ctrl+S</span>).</li>
      <li>La <b>moitié gauche</b> est votre disque, la <b>moitié droite</b> le serveur. Glissez d'un panneau à l'autre pour transférer.</li>
      <li><span class="kbd">Ctrl+Entrée</span> met la sélection dans la file, <span class="kbd">F5</span> actualise les deux volets.</li>
    </ul>
    <h4 style="margin-bottom:4px">Raccourcis clavier</h4>
    <div class="kb-list">${Object.entries(KEYS).map(([k, v]) => `<div><span>${esc(v)}</span><span class="kbd">${esc(k)}</span></div>`).join("")}</div>
    <h4 style="margin:12px 0 4px">Formats compatibles</h4>
    <p style="margin:0">Le gestionnaire de sites lit et écrit le <span class="mono">sitemanager.xml</span> d'origine de FileZilla. Les réglages sont stockés en JSON dans le dossier de configuration.</p>
    </div>`, [["Fermer", ""]], null, "860px");
}
function aboutDialog() {
  dialog("À propos", `<div class="about">
    <img src="/icon.svg" width="56" height="56">
    <h2>Zilla-RS ${esc(S.version)}</h2>
    <p>Client FTP / FTPES / FTPS / SFTP, entièrement web, écrit en Rust.</p>
    <div class="badges">
      <span class="badge">aucune dépendance web</span>
      <span class="badge">sitemanager.xml compatible</span>
      <span class="badge">mode passif + EPSV</span>
      <span class="badge">reprise sur REST</span>
    </div>
    <p class="dim" style="font-size:12px">${Object.keys(S.connected).filter((k) => S.connected[k]?.connected).length} site(s) connecté(s) · uptime ${fmt.dur((Date.now() - bootTime) / 1000)}</p>
    <div style="margin-top:14px;display:flex;gap:8px;justify-content:center">
      <button class="btn" id="ab-check">Vérifier le client (selftest)</button>
    </div>
    <div id="ab-res" class="dim" style="margin-top:10px;font-size:12px"></div>
    </div>`, [["Fermer", ""]], (h) => {
      $("#ab-check").onclick = async () => {
        $("#ab-res").innerHTML = `<span class="spin"></span> tests en cours…`;
        try {
          const d = await apiGet("/selftest");
          $("#ab-res").innerHTML = (d.checks || []).map((c) => `<div class="${c.ok ? "ok" : "err"}">${c.ok ? "✓" : "✗"} ${esc(c.name)} ${c.ok ? "" : "— " + esc(c.note || "")}</div>`).join("")
            + `<div style="margin-top:6px">${d.passes}/${(d.checks || []).length} réussis en ${d.ms} ms</div>`;
        } catch (e) { $("#ab-res").innerHTML = `<span class="err">${esc(e.message)}</span>`; }
      };
    }, "640px");
}

/* ═══════════════════════════ palette ═══════════════════════════ */
const CMDS = [
  ["Ouvrir le gestionnaire de sites", () => siteManagerDialog(), "Ctrl+S"],
  ["Se connecter", () => S.selectedSite ? connectTo(S.selectedSite) : quickConnect(), ""],
  ["Se déconnecter", disconnectCurrent, "Ctrl+Shift+D"],
  ["Se reconnecter", reconnect, "Ctrl+Shift+R"],
  ["Actualiser les deux panneaux", refreshAll, "F5"],
  ["Rechercher des fichiers", () => searchDialog(S.active), "Ctrl+F"],
  ["Rechercher une commande", () => palette(), "Ctrl+K"],
  ["Réglages", () => settingsDialog(), "Ctrl+,"],
  ["File des transferts", queueDialog, "Ctrl+Alt+T"],
  ["Signets", bookmarksDialog, ""],
  ["Comparer les dossiers local/distant", compareDialog, "Ctrl+Shift+C"],
  ["Télécharger la sélection", () => { const s = selected("remote"); if (s.length) download(s.map((e) => joinRemote(S.remote.path, e.name)), S.local.path, true); }, "Ctrl+D"],
  ["Envoyer la sélection", () => { const s = selected("local"); if (s.length) upload(s.map((e) => joinLocal(S.local.path, e.name)), S.remote.path, true); }, "Ctrl+U"],
  ["Dossier nouveau (panneau actif)", () => mkdir(S.active), "Ctrl+Alt+D"],
  ["Supprimer la sélection", () => del(S.active), "Suppr"],
  ["Journal détaillé", () => { S.log.verbose = !S.log.verbose; renderLog(); }, ""],
  ["Thème clair/sombre", () => { const g = S.settings.general.theme = S.settings.general.theme === "dark" ? "light" : "dark"; applySettings(); apiPost("/settings", { general: { theme: g } }); }, ""],
  ["Aide", helpDialog, "F1"],
  ["À propos", aboutDialog, ""],
];
function palette() {
  dialog("Recherche de commandes", `<div class="palette"><input id="pl" placeholder="Que voulez-vous faire ?"><div class="items" id="pl-i"></div></div>`,
    [["Fermer", ""]], (h) => {
      let sel = 0;
      const fill = (q = "") => {
        const list = CMDS.filter((c) => !q || c[0].toLowerCase().includes(q.toLowerCase()));
        sel = 0;
        $("#pl-i").innerHTML = list.map((c, i) => `<div class="it ${i === 0 ? "sel" : ""}" data-i="${i}"><span>${esc(c[0])}</span><span class="k">${esc(c[2] || "")}</span></div>`).join("");
        $$("#pl-i .it").forEach((n) => { n.onclick = () => { close(); setTimeout(list[+n.dataset.i][1], 5); }; n.onmouseenter = () => { sel = +n.dataset.i; paint(); }; });
      };
      const paint = () => $$("#pl-i .it").forEach((n, i) => n.classList.toggle("sel", i === sel));
      fill();
      const inp = $("#pl");
      inp.focus();
      inp.oninput = () => fill(inp.value);
      inp.onkeydown = (e) => {
        const n = $$("#pl-i .it");
        if (e.key === "ArrowDown") { sel = Math.min(n.length - 1, sel + 1); paint(); e.preventDefault(); }
        else if (e.key === "ArrowUp") { sel = Math.max(0, sel - 1); paint(); e.preventDefault(); }
        else if (e.key === "Enter") { e.preventDefault(); const c = CMDS.filter((c) => !inp.value || c[0].toLowerCase().includes(inp.value.toLowerCase())); close(); setTimeout(c[sel]?.[1], 5); }
      };
    }, "660px");
}

/* ═══════════════════════════ raccourcis ═══════════════════════════ */
const KEYS = {
  "Ctrl+S": "Gestionnaire de sites",
  "Ctrl+F": "Recherche de fichiers",
  "Ctrl+K": "Recherche de commandes",
  "Ctrl+I": "Voir / éditer le fichier",
  "Ctrl+P": "Permissions du fichier",
  "Ctrl+D": "Télécharger la sélection",
  "Ctrl+U": "Envoyer la sélection",
  "Ctrl+Entrée": "Transférer la sélection",
  "F2": "Renommer",
  "F5": "Actualiser",
  "Suppr": "Supprimer",
  "Alt+1 / Alt+2": "Focus local / distant",
  "Alt+PageUp/Down": "Monter d'un cran",
  "Ctrl+Alt+T": "File des transferts",
  "Ctrl+Shift+C": "Comparer les dossiers local/distant",
  "Ctrl+Shift+R": "Reconnecter",
  "Ctrl+Shift+D": "Déconnecter",
  "Ctrl+,": "Réglages",
  "F1": "Aide",
};
function keyMatch(e, spec) {
  const parts = spec.split("+");
  const key = parts.pop().toLowerCase();
  const ctrl = parts.includes("Ctrl") || (parts.includes("Cmd"));
  const shift = parts.includes("Shift");
  const alt = parts.includes("Alt");
  return !!ctrl === (e.ctrlKey || e.metaKey) && !!shift === e.shiftKey && !!alt === e.altKey &&
    e.key.toLowerCase() === key;
}
const SHORTCUTS = Object.entries(KEYS);
const KEYMAP = [
  ["F1", () => helpDialog()],
  ["F5", () => refreshAll()],
  ["Ctrl+S", () => siteManagerDialog(), { noType: true }],
  ["Ctrl+F", () => searchDialog(S.active)],
  ["Ctrl+K", () => palette()],
  ["Ctrl+,", () => settingsDialog()],
  ["Ctrl+I", () => { const s = selected(S.active)[0]; if (s) view(S.active, s); }, { noType: true }],
  ["Ctrl+P", () => { const s = selected(S.active)[0]; if (s) chmod(S.active, s); }, { noType: true }],
  ["Ctrl+D", () => { const s = selected("remote"); if (s.length) download(s.map((x) => joinRemote(S.remote.path, x.name)), S.local.path, true); }, { noType: true }],
  ["Ctrl+U", () => { const s = selected("local"); if (s.length) upload(s.map((x) => joinLocal(S.local.path, x.name)), S.remote.path, true); }, { noType: true }],
  ["Ctrl+Enter", () => {
    const s = selected(S.active);
    if (!s.length) return;
    if (S.active === "remote") download(s.map((x) => joinRemote(S.remote.path, x.name)), S.local.path, true);
    else upload(s.map((x) => joinLocal(S.local.path, x.name)), S.remote.path, true);
  }, { noType: true }],
  ["Ctrl+Shift+R", () => reconnect()],
  ["Ctrl+Shift+D", () => disconnectCurrent()],
  ["Ctrl+Shift+C", () => compareDialog()],
  ["Ctrl+Alt+T", () => queueDialog()],
  ["Delete", () => del(S.active), { noType: true }],
  ["F2", () => { const s = selected(S.active)[0]; if (s) rename(S.active, s); }, { noType: true }],
  ["Enter", () => { const s = selected(S.active)[0]; if (s) open(S.active, s); }, { noType: true }],
  ["Alt+1", () => focusPane("local")],
  ["Alt+2", () => focusPane("remote")],
  ["Alt+ArrowUp", () => goto(S.active, parentOf(PANES[S.active].path))],
  ["Alt+ArrowLeft", () => goto(S.active, parentOf(PANES[S.active].path))],
  ["Alt+ArrowRight", () => historyGo(S.active, 1)],
];
function focusPane(w) { S.active = w; $("#pane-" + w).focus(); }
function normalizeKey(e) {
  const mods = [];
  if (e.ctrlKey || e.metaKey) mods.push("Ctrl");
  if (e.shiftKey) mods.push("Shift");
  if (e.altKey) mods.push("Alt");
  let k = e.key;
  if (k === " ") k = "Space";
  else if (k.length === 1) k = k.toUpperCase();
  mods.push(k);
  return mods.join("+");
}
addEventListener("keydown", (e) => {
  const t = document.activeElement || {};
  const typing = /INPUT|TEXTAREA|SELECT/.test(t.tagName || "") || t.isContentEditable;
  if (e.key === "Escape") { if (!$("#overlay").classList.contains("hidden")) close(); else closeMenu(); return; }
  if (e.ctrlKey && e.key.toLowerCase() === "w") return;
  const spec = normalizeKey(e);
  if (spec === "Ctrl+K" && $("#overlay").classList.contains("hidden")) { e.preventDefault(); palette(); return; }
  for (const [k, fn, opt] of KEYMAP) {
    if (k !== spec) continue;
    if (opt && opt.noType && typing) continue;
    if (e.key === "Enter" && typing) continue;
    if (k.startsWith("Alt") && !e.altKey) continue;
    if ((k.startsWith("Ctrl+") || k.startsWith("Shift+Ctrl")) && !(e.ctrlKey || e.metaKey)) continue;
    if (!e.ctrlKey && !e.altKey && !e.shiftKey && k.length > 1 && !["F2", "F5", "Delete", "Enter", "Escape"].includes(k)) continue;
    e.preventDefault();
    fn();
    return;
  }
});

/* ═══════════════════════════ rafraîchissements ═══════════════════════════ */
async function refreshAll() {
  await Promise.all([loadLocal().catch(() => { }), S.remote.connected ? loadRemote().catch(() => { }) : Promise.resolve()]);
  refreshStatus();
}
async function refreshStatus() {
  try {
    const d = await apiGet("/status");
    S.version = d.version || S.version;
    S.settings = d.settings || S.settings;
    const conns = {};
    for (const c of d.connections || []) conns[c.site_id] = c;
    S.connected = conns;
    S.queue.stats = d.transfers || S.queue.stats;
    renderConnections();
    renderQueue();
    applySettings();
    if (!S.remote.site && d.connections?.length) S.remote.site = d.connections[0].site_id;
  } catch (e) { }
}
async function loadRecents() {
  try {
    const r = await apiGet("/recents");
    S.recents = r;
    $("#recent-hosts").innerHTML = (r.hosts || []).map((h) => `<option value="${esc(h)}">`).join("");
  } catch { }
}

/* ═══════════════════════════ websocket ═══════════════════════════ */
function wsConnect() {
  const p = location.protocol === "https:" ? "wss:" : "ws:";
  const url = `${p}//${location.host}/_ws`;
  let ws;
  try { ws = new WebSocket(url); } catch { }
  if (!ws) return setTimeout(wsConnect, 4000);
  S.ws = ws;
  ws.onmessage = (ev) => {
    let m;
    try { m = JSON.parse(ev.data); } catch { return; }
    if (m.kind === "hello") {
      S.log.lines = m.lines || [];
      renderLog();
      if (m.status) {
        S.settings = m.status.settings || S.settings;
        S.version = m.status.version || S.version;
        renderConnections();
        renderQueue();
        applySettings();
      }
    } else if (m.kind === "log") {
      pushLog(m.data);
    } else if (m.kind === "dirty") {
      if (m.data.what === "queue") refreshQueue();
      else if (m.data.what === "connections") refreshStatus();
      else if (m.data.what === "sites") loadSites().then(renderSiteTree);
      else if (m.data.what === "local") loadLocal(S.local.path);
      else if (m.data.what === "remote") if (S.remote.connected) loadRemote(S.remote.path, true);
    } else if (m.kind === "status") {
      const d = m.status || m.data;
      if (d) { S.queue.stats = d.transfers || S.queue.stats; S.connected = Object.fromEntries((d.connections || []).map((c) => [c.site_id, c])); renderQueue(); renderConnections(); }
    } else if (m.kind === "toast") {
      toast(m.data.text, m.data.level === "error" ? "err" : "ok");
    } else if (m.kind === "job") {
      if (m.data.ok) toast(m.data.text || "Transfert terminé", "ok");
      else toast(m.data.text || "Transfert en échec", "err");
      refreshQueue();
    }
  };
  ws.onclose = () => { clearTimeout(S.wsT); S.wsT = setTimeout(wsConnect, 2500); };
  ws.onerror = () => { try { ws.close(); } catch { } };
  ws.onopen = () => { };
}

/* ═══════════════════════════ gouttières / taille ═══════════════════════════ */
function bindGutters() {
  const panes = $("#panes");
  [["#g1", "g1"], ["#g2", "g2"]].forEach(([id]) => {
    const g = $(id);
    g.addEventListener("mousedown", (ev) => {
      ev.preventDefault();
      g.classList.add("drag");
      const mv = (e) => {
        const rects = [...document.querySelectorAll(".pane")].map((p) => p.getBoundingClientRect().left);
        // arbitre entre les panneaux
        const x = e.clientX;
        const lp = $("#pane-local"), rp = $("#pane-remote");
        if (id === "#g1") {
          const w = Math.max(160, Math.min(panes.clientWidth - 460, x - rects[0]));
          lp.style.flex = `0 0 ${w}px`;
        } else {
          const w = Math.max(180, Math.min(panes.clientWidth - 460, x - lp.getBoundingClientRect().right - (168 + 10)));
          $("#pane-queue").style.flex = `0 0 ${w}px`;
        }
      };
      const up = () => { g.classList.remove("drag"); removeEventListener("mousemove", mv); removeEventListener("mouseup", up); store.set("panes", paneSizes()); };
      addEventListener("mousemove", mv);
      addEventListener("mouseup", up);
    });
  });
}
function paneSizes() {
  return { l: $("#pane-local").style.flex, q: $("#pane-queue").style.flex };
}
function applyPaneSizes() {
  const p = store.get("panes", null);
  if (!p) return;
  if (p.l) $("#pane-local").style.flex = p.l;
  if (p.q) $("#pane-queue").style.flex = p.q;
}
function bindLogResize() {
  const g = $("#log-grip"), w = $("#logwrap");
  g.addEventListener("mousedown", (e) => {
    e.preventDefault();
    const y0 = e.clientY, h0 = w.offsetHeight;
    const mv = (ev) => { w.style.height = Math.max(60, innerHeight - 120, h0 - (ev.clientY - y0)) + "px"; };
    const up = () => { removeEventListener("mousemove", mv); removeEventListener("mouseup", up); store.set("logh", w.style.height); };
    addEventListener("mousemove", mv);
    addEventListener("mouseup", up);
  });
  const h = store.get("logh", null);
  if (h) w.style.height = h;
}

/* ═══════════════════════════ démarrage ═══════════════════════════ */
const bootTime = Date.now();
function bindChrome() {
  $("#btn-sites").onclick = siteManagerDialog;
  $("#btn-connect").onclick = () => (S.selectedSite ? connectTo(S.selectedSite) : quickConnect());
  $("#btn-disconnect").onclick = disconnectCurrent;
  $("#btn-reconnect").onclick = reconnect;
  $("#btn-both-refresh").onclick = refreshAll;
  $("#btn-queue").onclick = queueDialog;
  $("#btn-search").onclick = () => searchDialog(S.active);
  $("#btn-bookmarks").onclick = bookmarksDialog;
  $("#btn-palette").onclick = palette;
  $("#btn-settings").onclick = () => settingsDialog();
  $("#btn-help").onclick = helpDialog;
  $("#brand").onclick = aboutDialog;
  $("#qc-go").onclick = quickConnect;
  $("#qc-host").addEventListener("keydown", (e) => { if (e.key === "Enter") quickConnect(); });
  $("#qc-pass").addEventListener("keydown", (e) => { if (e.key === "Enter") quickConnect(); });
  $("#btn-theme").onclick = () => {
    const cur = S.settings.general.theme === "light" ? "dark" : "light";
    S.settings.general.theme = cur;
    applySettings();
    apiPost("/settings", { general: { theme: cur } });
    $("#btn-theme").textContent = cur === "light" ? "☀" : "☾";
  };
  $("#btn-log-pane").onclick = () => {
    const w = $("#logwrap");
    w.classList.toggle("hidden");
    S.settings.ui.show_message_pane = !w.classList.contains("hidden");
    apiPost("/settings", { ui: { show_message_pane: S.settings.ui.show_message_pane } });
  };
  $("#log-clear").onclick = () => { apiPost("/log/clear", {}); S.log.lines = []; renderLog(); };
  $("#log-save").onclick = () => window.open("/api/log/save", "_blank");
  $("#log-verbose").onchange = (e) => { S.log.verbose = e.target.checked; renderLog(); };
  $$(".logtabs .tab").forEach((t) => t.onclick = () => { $$(".logtabs .tab").forEach((x) => x.classList.remove("active")); t.classList.add("active"); S.log.cat = t.dataset.log; renderLog(); });
  $("#log").addEventListener("wheel", (e) => { $("#log").dataset.dir = e.deltaY > 0 ? "down" : "up"; }, { passive: true });
  $("#st-info").onclick = async () => {
    const d = await apiGet("/connections");
    dialog("Informations de connexion", (d || []).map((i) => `<fieldset><legend>${esc(i.host)}</legend>
      <div class="field"><label>Protocole</label><div>${esc(i.proto)}${i.tls ? ` — ${esc(i.tls_version)} ${esc(i.cipher)}` : " <span class='warn'>non chiffré</span>"}</div></div>
      <div class="field"><label>Utilisateur</label><div>${esc(i.user || "—")}</div></div>
      <div class="field"><label>Bannière</label><div class="mono" style="font-size:11.5px">${esc(i.banner)}</div></div>
      <div class="field"><label>Locaux → distants</label><div class="mono" style="font-size:11.5px">${esc(i.local_addr)} → ${esc(i.remote_addr)}</div></div>
      <div class="field"><label>Extensions serveur</label><div class="mono" style="font-size:11px">${esc((i.features || []).join(" "))}</div></div>
      </fieldset>`).join("") || `<div class="empty">Aucune connexion active.</div>`, [["Fermer", ""]], null, "700px");
  };
}

async function boot() {
  try {
    const st = await apiGet("/status");
    S.settings = st.settings || {};
    S.version = st.version || "";
    S.remote.path = "/";
    applySettings();
    applyPaneSizes();
    let home = "/";
    try { const rr = await apiGet("/local/roots"); home = rr.home || (rr.roots[0] && rr.roots[0].path) || "/"; } catch { }
    await loadLocal(home);
    await Promise.all([loadSites(), loadBookmarks(), loadRecents()]);
    refreshQueue();
    renderConnections();
    renderPane("local"); renderPane("remote");
    bindChrome(); bindGutters(); bindLogResize();
    ["local", "remote"].forEach(bindPane);
    wsConnect();
    // Journal : on le récupère même si la websocket tarde.
    try {
      const lg = await apiGet("/log?since=0");
      if (lg && lg.lines && lg.lines.length) {
        S.log.lines = lg.lines;
        renderLog();
      }
    } catch { }
    setInterval(() => { if (!document.hidden) { refreshQueue(); } }, 3500);
    setInterval(() => { if (!document.hidden) refreshStatus(); }, 12000);
  } catch (e) {
    document.body.insertAdjacentHTML("beforeend", `<div style="position:fixed;inset:auto 12px 12px;padding:14px;background:var(--panel);border:1px solid var(--err);border-radius:10px">Le serveur ne répond pas : ${esc(e.message)}</div>`);
  }
}
boot();
