/* Harnais vivant : le même DOM de fortune, mais le HTTP vient d'un vrai poste
   démarré par `tests/_pont.rs`. Ce que la page lit vraiment ne se discute plus
   avec des promesses écrites à la main.

     cargo test --test _pont -- --nocapture &     (un pont sur 127.0.0.1:8745)
     node tools/harnais-vivant.js                  un silence est un succès */
"use strict";
const fs = require("fs");
const path = require("path");
const http = require("http");

const RACINE = path.join(__dirname, "..");
const PIED = process.env.ZILLA_PIED || "http://127.0.0.1:8745";
const JS = fs.readFileSync(path.join(RACINE, "web/app.js"), "utf8");

/* le DOM de fortune est emprunté au harnais muet : mêmes ficelles, mêmes faux
   panneaux, pour ne pas avoir à en entretenir deux. Sa propre amorce (fs,
   path, html, js) est épongée puisqu'elle est déjà au-dessus, et sa racine
   tenue pour l'occasion. */
const harnais = fs.readFileSync(path.join(__dirname, "verif-ui.js"), "utf8");
const charnière = harnais.indexOf("/* la file qu'on joue est en mémoire");
if (charnière < 0) { console.error("charnière introuvable dans verif-ui.js"); process.exit(1); }
const amorce = harnais
  .slice(0, charnière)
  .replace(/const RACINE =[^\n]*;/, "const RACINE = " + JSON.stringify(RACINE) + ";");
new Function("require", "__dirname", amorce + "\n;global.registry = registry;")(require, __dirname);

const joindre = (chemin, methode, corps) =>
  new Promise((res, rej) => {
    const corpsBuf = corps ? Buffer.from(corps) : null;
    const r = http.request(PIED + "/api" + chemin, {
      method: methode || "GET",
      headers: corpsBuf ? { "content-type": "application/json", "content-length": corpsBuf.length } : {},
    }, (rep) => {
      let tout = "";
      rep.on("data", (b) => (tout += b));
      rep.on("end", () => res({ ok: rep.statusCode === 200, status: rep.statusCode, json: async () => JSON.parse(tout) }));
    });
    r.on("error", rej);
    if (corpsBuf) r.write(corpsBuf);
    r.end();
  });

/* la page cherche "/api/…" : on lui rend la main à joindre, qui tient le pont */
global.fetch = (u, o) => joindre(String(u).replace(/^\/api/, ""), o && o.method, o && o.body);

(async () => {
  /* une prise avant tout : sans pont, ce harnais ne dit rien de ce qu'il croit dire */
  try { await joindre("/status"); } catch {
    console.error("point sur : " + PIED + "\n  le tenir ouvert : cargo test --test pont -- --ignored --nocapture &\n  puis relancer : node tools/harnais-vivant.js");
    process.exit(1);
  }  new Function(JS + "\n;global.__api = { boot, S, apiGet, apiPost, majConnexion, rendreFavoris, rendrePastille, refreshStatus };")();
  const A = global.__api;
  await new Promise((r) => setTimeout(r, 900));

  const dans = registry.get("btn-connect");
  const hors = registry.get("btn-disconnect");
  const pastille = registry.get("conn-dot");
  const barre = registry.get("fav-bar");
  const rappeller = registry.get("btn-reconnect");
  let faute = 0;
  const verif = (lu, attendu, mot) => {
    const juste = lu === attendu;
    if (!juste) faute++;
    console.log(`   ${juste ? "✓" : "✗"} ${mot}${juste ? "" : `  — attendu ${attendu}, lu ${lu}`}`);
  };

  console.log("1) à vide          :");
  verif(!dans.disabled, true, "« Se connecter » libre");
  verif(hors.disabled, true, "« Déconnecter » figé");
  verif(!pastille.classList.contains("on"), true, "pastille éteinte");
  verif(rappeller.disabled, true, "« Rappeler » figé, faute de site en main");

  console.log("2) sites et raccourcis :");
  const sites = await A.apiGet("/sites");
  console.log("   sites vus        :", sites.sites.map((s) => s.name + (s.favorite ? "★" : "")).join(", "));
  const favori = sites.sites.find((s) => s.favorite) || {};
  const raccourci = registry.get("fav-" + favori.id);
  verif(!!raccourci, true, "un site favori tient son bouton dans la barre");

  console.log("3) la connexion est faite :");
  await A.apiPost("/connect", { site_id: sites.sites[0].id });
  await A.refreshStatus();
  verif(dans.disabled, true, "« Se connecter » figé");
  verif(!hors.disabled, true, "« Déconnecter » libre");
  verif(pastille.classList.contains("on"), true, "pastille allumée");
  const tenu = registry.get("fav-" + favori.id);
  if (tenu) verif(tenu.classList.contains("live"), true, "le raccourci prend la couleur de l'état");

  console.log("4) la connexion rend les armes :");
  await A.apiPost("/disconnect", { id: sites.sites[0].id });
  await A.refreshStatus();
  verif(!dans.disabled, true, "« Se connecter » libre");
  verif(hors.disabled, true, "« Déconnecter » figé");
  verif(!pastille.classList.contains("on"), true, "pastille éteinte");
  verif(!rappeller.disabled, true, "« Rappeler » reste la main courante, même à vide");

  console.log("5) la page se réveille sur un fil déjà ouvert :");
  await A.apiPost("/connect", { site_id: sites.sites[0].id });
  A.S.connected = {};                       // elle sort de lit, sans savoir
  const portrait = await A.apiGet("/status");
  A.S.ws.onmessage({ data: JSON.stringify({ kind: "hello", lines: [], status: portrait }) });
  verif(dans.disabled, true, "« Se connecter » se fige dès le message d'accueil");
  verif(!hors.disabled, true, "« Déconnecter » reprend la main");
  verif(pastille.classList.contains("on"), true, "pastille allumée sans rien demander");
  await A.apiPost("/disconnect", { id: sites.sites[0].id });
  await A.refreshStatus();

  console.log(faute ? `\n${faute} faute(s)` : "\nsilence : le poste et la page se ressemblent");
  process.exit(faute ? 1 : 0);
})().catch((e) => { console.error("CA CASSE :", e); process.exit(1); });
