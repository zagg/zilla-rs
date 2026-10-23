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

/* la file qu'on joue est en mémoire : rien ne traîne par-dessus les runs.
   l'état s'écrit comme le moteur le lâche aujourd'hui sur le fil — minuscule,
   « done », « down » — et `ligne(...)` habille la même rangée en « Done »,
   « Down » quand elle dort dans `transferqueue.json` depuis avant la mise à
   jour : la page est censue lire les deux, et c'est bien là que la faute
   s'était logée. */
const ligne = (etat, anciennement = false) => ({
  id: "j1", site_id: "s1", site_name: "démo",
  direction: anciennement ? "Down" : "down",
  state: anciennement ? etat[0].toUpperCase() + etat.slice(1) : etat,
  remote: "/atelier/fiche.txt", local: "/root/fiche.txt", size: 1000, transferred: 320,
  speed: 204800, started: 1, finished: 0, error: "", attempts: 0, last_step: 1,
  priority: anciennement ? "Normal" : "normal", kind: "file",
});
let etat = {
  jobs: [ligne("active")],
  stats: { pending: 1, active: 1, done: 0, failed: 0, paused: false, bytes_per_sec: 204800, total_bytes: 1000, done_bytes: 0 },
};
const copie = () => JSON.parse(JSON.stringify(etat));
/* le portrait des connexions tel que le poste le rend : `connected`, `live`,
   `max` — c'est sur ces mots-là que la page regarde l'état du fil */
let connexions = [];
let appels = 0;
global.fetch = async (u, o) => {
  appels++;
  const chemin = String(u).replace("/api", "");
  if (process.env.TRACE) console.log("   HTTP", (o && o.method) || "GET", chemin);
  let donnees;
  if (chemin.startsWith("/status")) {
    donnees = {
      version: "test", connections: connexions, transfers: etat.stats, settings: {
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
  new Function(JS + "\n;global.__api = { boot, S, montrerFile, voletTransferts, renderQueue, refreshQueue, refreshStatus, majConnexion, rendreFavoris, rendrePastille, viderFile };")();
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
  const barre = registry.get("fav-bar");
  const boutonConnecter = registry.get("btn-connect");
  const boutonDeconnecter = registry.get("btn-disconnect");
  const pastille = registry.get("conn-dot");

  console.log("1) à l'ouverture   : volet caché =", tq.classList.contains("hidden"), "| journal caché =", journal.style.display === "none");

  const onglet = tabs.find((t) => t.dataset.log === "transfers");
  onglet.click();
  await new Promise((r) => _setTimeout(r, 30));
  console.log("2) onglet ouvert   : volet caché =", tq.classList.contains("hidden"),
    "| journal caché =", journal.style.display === "none",
    "| outils cachés =", outils.classList.contains("hidden"));

  etat.jobs.push(Object.assign(ligne("active", true), { id: "j2", direction: "Up", remote: "/atelier/en-voi.txt" }));
  await A.refreshQueue();
  console.log("3) une ligne en cours →", /class="r"/.test(liste.innerHTML) ? "ligne affichée" : "RIEN",
    "| une barre de progression ", /class="bar/.test(liste.innerHTML) ? "dedans" : "absente",
    "| le sens lu sur le fil =", /\u2193/.test(liste.innerHTML) && /\u2191/.test(liste.innerHTML) ? "les deux, comme écrit" : "MAL TRADUIT",
    "| badge onglet =", compte.textContent);

  etat = { jobs: [ligne("waiting")], stats: { pending: 1, active: 0, done: 0, failed: 0, paused: false, bytes_per_sec: 0, total_bytes: 1000, done_bytes: 0 } };
  await A.refreshQueue();
  console.log("4) en attente      : une ligne debout =", /class="r"/.test(liste.innerHTML),
    "| libellé =", /en attente/.test(liste.innerHTML) ? "\u00ab en attente \u00bb" : "MAL TRADUIT");

  etat = { jobs: [ligne("done"), Object.assign(ligne("done", true), { id: "j3" })], stats: { pending: 0, active: 0, done: 2, failed: 0, paused: false, bytes_per_sec: 0, total_bytes: 2000, done_bytes: 2000 } };
  await A.refreshQueue();
  const encore = /class="r"/.test(liste.innerHTML);
  console.log("5) ligne finie     : encore affichée =", encore,
    "(qu'elle dise « done » ou « Done », la rangée est partie)",
    "| décompte =", String(stats.innerHTML).match(/\d+ terminés/)[0],
    "| lien masqués =", /q-show-done/.test(stats.innerHTML));

  console.log("6) bouton du bandeau :");
  A.montrerFile();
  console.log("   volet caché =", registry.get("transfer-view").classList.contains("hidden"));

  console.log("7) état de la connexion, tel que le poste le rend :");
  connexions = [{ site_id: "s1", host: "demo.local", proto: "FTP", name: "anonymous@demo.local", connected: true, live: 1, max: 10 }];
  await A.refreshStatus();
  console.log("   un fil ouvert  : « Se connecter » figé =", !!boutonConnecter.disabled,
    "| « Déconnecter » libre =", !boutonDeconnecter.disabled,
    "| pastille allumée =", pastille.classList.contains("on"),
    "| elle nomme le tenu =", /demo.local/.test(pastille.innerHTML));
  connexions = [];
  await A.refreshStatus();
  console.log("   plus de fil    : « Se connecter » libre =", !boutonConnecter.disabled,
    "| « Déconnecter » figé =", !!boutonDeconnecter.disabled,
    "| pastille éteinte =", !pastille.classList.contains("on"));

  console.log("   et le même état rendu par le seul compte des fils (live) :");
  connexions = [{ site_id: "s1", host: "demo.local", proto: "SFTP", connections: 2, max_connections: 4 }];
  await A.refreshStatus();
  console.log("   vu ouvert      : « Déconnecter » libre =", !boutonDeconnecter.disabled,
    "| pastille allumée =", pastille.classList.contains("on"));
  connexions = [{ site_id: "s1", host: "demo.local", proto: "SFTP", connections: 0, max_connections: 4 }];
  await A.refreshStatus();
  console.log("   vu fermé       : « Se connecter » libre =", !boutonConnecter.disabled,
    "| pastille éteinte =", !pastille.classList.contains("on"));

  console.log("8) favoris du gestionnaire :");
  connexions = [];
  A.S.sites = [{ id: "s1", name: "démo", host: "demo.local", proto: "ftp", favorite: true },
               { id: "s9", name: "oublié", host: "autre", proto: "ftp" }];
  A.rendreFavoris();
  const favori = registry.get("fav-s1");
  const nom = favori ? favori.children.map((c) => c.textContent || "").join("") : "(aucun)";
  console.log("   un favori sur deux → un bouton dans la barre =",
    !!favori, "| il dit le nom du site =", JSON.stringify(nom),
    "| le non-favori n'y est pas =", !registry.has("fav-s9"));
  A.S.sites = [A.S.sites[1]];
  A.rendreFavoris();
  console.log("   plus aucun favori : la barre reste ouverte et tend la perche =",
    !!registry.get("fav-aucun"));
  console.log("\nrequêtes HTTP simulées :", appels);
  process.exit(0);
})().catch((e) => { console.error("CA CASSE:", e); process.exit(1); });
