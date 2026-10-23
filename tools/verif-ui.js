/* Faute de navigateur, on fait tourner app.js dans un DOM de fortune :
   de quoi vérifier que la page se monte, que l'onglet « Transferts » bascule
   bien sur le tableau de file, et qu'une ligne terminée en sort.

     node tools/verif-ui.js        un silence est un succès
     TRACE=1 node tools/verif-ui.js   montre chaque appel HTTP

   C'est un harnais de câblage, pas un test : il dit que les raccords tiennent,
   pas que la mise en page est belle. Ce qu'il trouve tient donc pour deux —
   une faute dans app.js, ou une faute dans son propre DOM de fortune. */
"use strict";
const fs = require("fs");
const path = require("path");

const RACINE = path.join(__dirname, "..");
const HTML = fs.readFileSync(path.join(RACINE, "web/index.html"), "utf8");
const JS = fs.readFileSync(path.join(RACINE, "web/app.js"), "utf8");

class ClassList {
  constructor(node, initial) { this.node = node; this._ = new Set((initial || "").split(/\s+/).filter(Boolean)); }
  add(...c) { c.forEach((x) => this._.add(x)); this.node._className = [...this._].join(" "); }
  remove(...c) { c.forEach((x) => this._.delete(x)); this.node._className = [...this._].join(" "); }
  contains(c) { return this._.has(c); }
  toggle(c, on) { if (on === undefined) on = !this._.has(c); on ? this.add(c) : this.remove(c); return on; }
  get value() { return [...this._].join(" "); }
}

let Racine = null;
class Node {
  constructor(tag, attrs = {}, parent = null) {
    this.tagName = (tag || "div").toUpperCase();
    this.attributes = { ...attrs };
    this.children = [];
    this.parent = parent;
    Object.defineProperty(this, "parentElement", { get: () => Racine || this.parent || this });
    this.style = {};
    this.style.setProperty = (k, v) => { this.style[k] = v; };
    this.style.getPropertyValue = (k) => this.style[k] || "";
    this.dataset = {};
    this._className = attrs.class || "";
    this._listeners = {};
    this._innerHTML = "";
    this.textContent = "";
    this.value = "";
    this.checked = false;
    this.scrollTop = 0; this.scrollHeight = 0; this.clientHeight = 0;
    this.classList = new ClassList(this, attrs.class || "");
  }
  get className() { return this._className; }
  set className(v) { this._className = v; this.classList = new ClassList(this, v); }
  get innerHTML() { return this._innerHTML; }
  set innerHTML(v) {
    this._innerHTML = v;
    // on garde les ids écrits dans la chair pour les recherches futures
    for (const m of String(v).matchAll(/id="([^"]+)"/g)) {
      const e = new Node("div", { id: m[1] });
      e.parent = this;
      if (!registry.has(m[1])) registry.set(m[1], e); else registry.set(m[1], e);
    }
  }
  addEventListener(type, fn) { (this._listeners[type] = this._listeners[type] || []).push(fn); }
  removeEventListener(type, fn) { this._listeners[type] = (this._listeners[type] || []).filter((f) => f !== fn); }
  dispatch(type, ev = {}) { for (const fn of this._listeners[type] || []) fn({ target: this, preventDefault() { }, ...ev }); }
  click() { if (this.onclick) this.onclick({ target: this, preventDefault() { } }); this.dispatch("click"); }
  focus() { } blur() { } remove() { }
  appendChild(c) { this.children.push(c); if (c && c.attributes && c.attributes.id) registry.set(c.attributes.id, c); return c; }
  append(...c) { c.forEach((x) => this.appendChild(x)); }
  querySelector(s) { return document.querySelector(s); }
  querySelectorAll(s) { return document.querySelectorAll(s); }
  insertAdjacentHTML(where, html) { this.innerHTML = this.innerHTML + (html || ""); }
  getBoundingClientRect() { return { left: 0, right: 100, top: 0, bottom: 100 }; }
  closest() { return null; }
  contains() { return true; }
  before(...c) { if (this.parent) this.parent.appendChild(c[0]); }
  setAttribute(k, v) { this.attributes[k] = v; }
  getAttribute(k) { return this.attributes[k]; }
}

const registry = new Map();
const root = new Node("body");
Racine = root;

// tout ce que la page annonce par id, on le monte d'avance, avec ses classes
for (const m of HTML.matchAll(/<([a-zA-Z]+)([^>]*)>/g)) {
  const attributs = m[2];
  const id = (attributs.match(/\bid="([^"]+)"/) || [])[1];
  if (!id) continue;
  const classe = (attributs.match(/\bclass="([^"]+)"/) || [""])[1];
  const e = new Node(m[1], { id, class: classe });
  registry.set(id, e);
  root.appendChild(e);
}
// les onglets, tels qu'ils sont écrits
const tabs = [];
for (const m of HTML.matchAll(/<button class="tab[^"]*" data-log="([^"]+)"/g)) {
  const t = new Node("button", { class: "tab", "data-log": m[1] });
  t.dataset.log = m[1];
  tabs.push(t);
  registry.set("tab:" + m[1], t);
}

global.document = {
  hidden: false,
  body: root,
  documentElement: new Node("html"),
  createElement: (t) => new Node(t),
  createTextNode: (t) => ({ textContent: t }),
  querySelector(sel) {
    if (sel.startsWith("#")) {
      const id = sel.slice(1);
      if (registry.has(id)) return registry.get(id);
      const n = new Node("div"); n.attributes.id = id; registry.set(id, n); return n;
    }
    if (sel === ".logtabs .tab") return tabs;
    if (sel === ".pane") return [registry.get("pane-local"), registry.get("pane-remote")];
    // tout autre sélecteur : un faux élément pour ne pas broncher
    const cle = "@" + sel;
    if (!registry.has(cle)) registry.set(cle, new Node("div", { id: cle }));
    return registry.get(cle);
  },
  querySelectorAll(sel) {
    if (sel === ".logtabs .tab") return tabs;
    if (sel === ".pane") return [registry.get("pane-local"), registry.get("pane-remote")];
    return [];
  },
  addEventListener() { }, removeEventListener() { },
};
global.window = global;
global.localStorage = {
  _d: {},
  getItem(k) { return k in this._d ? this._d[k] : null; },
  setItem(k, v) { this._d[k] = String(v); },
  removeItem(k) { delete this._d[k]; },
};
global.location = { protocol: "http:", host: "localhost:8734" };
global.addEventListener = () => { };
global.removeEventListener = () => { };
global.WebSocket = function () { this.onmessage = null; this.close = () => { }; };
try { Object.defineProperty(global, "navigator", { value: { userAgent: "node" }, configurable: true }); } catch { }
global.getComputedStyle = () => ({ getPropertyValue: () => "" });

/* la file qu'on joue est en mémoire : rien ne traîne par-dessus les runs */
const ligne = (etat) => ({
  id: "j1", site_id: "s1", site_name: "démo", direction: "down", state: etat,
  remote: "/atelier/fiche.txt", local: "/root/fiche.txt", size: 1000, transferred: 320,
  speed: 204800, started: 1, finished: 0, error: "", attempts: 0, last_step: 1,
  priority: "normal", kind: "file",
});
let etat = {
  jobs: [ligne("active")],
  stats: { pending: 1, active: 1, done: 0, failed: 0, paused: false, bytes_per_sec: 204800, total_bytes: 1000, done_bytes: 0 },
};
const copie = () => JSON.parse(JSON.stringify(etat));
let appels = 0;
global.fetch = async (u, o) => {
  appels++;
  const chemin = String(u).replace("/api", "");
  if (process.env.TRACE) console.log("   HTTP", (o && o.method) || "GET", chemin);
  let donnees;
  if (chemin.startsWith("/status")) {
    donnees = {
      version: "test", settings: {
        general: { theme: "dark", font_size: 13, show_hidden_local: true, double_click: "transfer", confirm_delete: true, confirm_transfers: true, language: "fr", use_trash: false, notify_finish: false },
        ui: { show_toolbar: true, show_message_pane: true, show_transfer_pane: true, show_statusbar: true, local_columns: ["name", "size"], remote_columns: ["name", "size"] },
        logs: { level: "normal" }, transfers: { max_parallel: 2 }, net: {}, ftp: {},
      },
    };
  } else if (chemin.startsWith("/transfers")) {
    donnees = copie();
  } else if (chemin.startsWith("/local/roots")) {
    donnees = { home: "/root", roots: [{ path: "/root" }] };
  } else if (chemin.startsWith("/local/list")) {
    donnees = { path: "/root", entries: [{ name: "un.txt", kind: "file", size: 3 }], };
  } else if (chemin.startsWith("/sites")) {
    donnees = { sites: [], connected: [] };
  } else if (chemin.startsWith("/bookmarks")) {
    donnees = [];
  } else if (chemin.startsWith("/recents")) {
    donnees = { hosts: [], local: [], remote: [] };
  } else if (chemin.startsWith("/log")) {
    donnees = { lines: [] };
  } else {
    donnees = {};
  }
  return { ok: true, status: 200, json: async () => ({ ok: true, data: donnees }), text: async () => "" };
};

// app.js veut être lu comme du module ordinaire
const timer = new Set();
const _setTimeout = setTimeout;
global.setTimeout = (fn, ms) => { const t = _setTimeout(() => { timer.delete(t); fn(); }, Math.min(ms || 0, 5)); timer.add(t); return t; };

(async () => {
  new Function(JS + "\n;global.__api = { boot, S, montrerFile, voletTransferts, renderQueue, refreshQueue };")();
  const A = global.__api;
  await new Promise((r) => _setTimeout(r, 200));
  // si boot() a heurté, il l'écrit dans la page : on le laisse voir
  const bob = (root.innerHTML + root.children.map((c) => c.innerHTML).join("")).match(/Le serveur ne répond pas : [^<]*/);
  if (bob) console.log("⚠ boot a heurté :", bob[0]);

  const tq = registry.get("transfer-view");
  const journal = registry.get("log");
  const outils = registry.get("tq-tools");
  const liste = registry.get("list-queue");
  const compte = registry.get("tq-count");
  const stats = registry.get("q-stats");

  console.log("1) à l'ouverture   : volet caché =", tq.classList.contains("hidden"), "| journal caché =", journal.style.display === "none");

  const onglet = tabs.find((t) => t.dataset.log === "transfers");
  onglet.click();
  await new Promise((r) => _setTimeout(r, 30));
  console.log("2) onglet ouvert   : volet caché =", tq.classList.contains("hidden"),
    "| journal caché =", journal.style.display === "none",
    "| outils cachés =", outils.classList.contains("hidden"));

  console.log("3) une ligne en cours →", /class="r"/.test(liste.innerHTML) ? "ligne affichée" : "RIEN",
    "| une barre de progression ", /class="bar/.test(liste.innerHTML) ? "dedans" : "absente",
    "| badge onglet =", compte.textContent);

  etat = { jobs: [ligne("done")], stats: { pending: 0, active: 0, done: 1, failed: 0, paused: false, bytes_per_sec: 0, total_bytes: 1000, done_bytes: 1000 } };
  await A.refreshQueue();
  const encore = /class="r"/.test(liste.innerHTML);
  console.log("4) ligne finie     : encore affichée =", encore, "| décompte =", String(stats.innerHTML).match(/\d+ terminés/)[0],
    "| lien masqués =", /q-show-done/.test(stats.innerHTML));

  console.log("5) bouton du bandeau :");
  A.montrerFile();
  console.log("   volet caché =", registry.get("transfer-view").classList.contains("hidden"));
  console.log("\nrequêtes HTTP simulées :", appels);
  process.exit(0);
})().catch((e) => { console.error("CA CASSE:", e); process.exit(1); });
