//! API HTTP + websocket du client.

use crate::app::App;
use crate::models::RemoteOp;
use crate::hub::Event;
use crate::models::{Bookmark, Priority, Site};
use axum::extract::ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;

const INDEX: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const APP_CSS: &str = include_str!("../web/app.css");
const ICON: &str = include_str!("../web/icon.svg");

pub fn new_token() -> String {
    let mut t = format!("{:x}", rand::random::<u64>());
    t.push_str(&format!("{:x}", rand::random::<u64>()));
    t
}

pub fn router(app: Arc<App>, public: bool) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/settings", get(get_settings).post(update_settings))
        .route("/log", get(get_log))
        .route("/log/clear", post(clear_log))
        .route("/log/save", get(save_log))
        .route("/sites", get(get_sites))
        .route("/sites/save", post(save_site))
        .route("/sites/delete", post(delete_site))
        .route("/sites/copy", post(copy_site))
        .route("/sites/import", post(import_sites))
        .route("/sites/export", get(export_sites))
        .route("/recents", get(get_recents))
        .route("/bookmarks", get(get_bookmarks).post(add_bookmark))
        .route("/bookmarks/delete", post(delete_bookmark))
        .route("/connect", post(connect))
        .route("/disconnect", post(disconnect))
        .route("/connections", get(connections))
        .route("/remote/list", get(remote_list))
        .route("/remote/op", post(remote_op))
        .route("/remote/file", get(remote_file))
        .route("/remote/save", post(remote_save))
        .route("/remote/search", get(remote_search))
        .route("/remote/exists", get(remote_exists))
        .route("/local/list", get(local_list))
        .route("/local/op", post(local_op))
        .route("/local/file", get(local_file))
        .route("/local/save", post(local_save))
        .route("/local/search", get(local_search))
        .route("/local/roots", get(local_roots))
        .route("/local/tree", get(local_tree))
        .route("/download", post(enqueue_download))
        .route("/upload", post(enqueue_upload))
        .route("/transfers", get(transfers))
        .route("/transfers/cancel", post(cancel_job))
        .route("/transfers/pause", post(pause_job))
        .route("/transfers/priority", post(priority_job))
        .route("/transfers/retry", post(retry_job))
        .route("/transfers/clear", post(clear_jobs))
        .route("/transfers/pause-all", post(pause_all))
        .route("/selftest", get(selftest));
    let api = Router::new().nest("/api", api);

    let token_for_guard = app.token.clone();
    let pages = Router::new()
        .route("/", get(index))
        .route("/app.css", get(css))
        .route("/app.js", get(js))
        .route("/icon.svg", get(icon))
        .route("/favicon.ico", get(icon))
        .route("/_ws", get(ws_handler))
        .layer(axum::middleware::from_fn(move |req, next| guard(req, next, token_for_guard.clone(), public)));

    pages.merge(api).with_state(app.clone()).fallback(not_found)
}

async fn guard(
    req: axum::extract::Request,
    next: axum::middleware::Next,
    token: String,
    public: bool,
) -> Response {
    let path = req.uri().path().to_string();
    if path.starts_with("/api/") && public {
        let ok = req
            .headers()
            .get("x-zilla-token")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == token)
            .unwrap_or(false);
        if !ok {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error": "jeter requis — passez --bind 127.0.0.1 ou fournissez x-zilla-token"})),
            )
                .into_response();
        }
    }
    next.run(req).await
}

fn err(e: impl std::fmt::Display) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": e.to_string() })),
    )
        .into_response()
}

fn ok<T: serde::Serialize>(v: T) -> Response {
    Json(json!({ "ok": true, "data": v })).into_response()
}

async fn index() -> Response {
    Html(INDEX).into_response()
}
async fn css() -> Response {
    css_resp(APP_CSS)
}
async fn js() -> Response {
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-cache"),
        ],
        APP_JS,
    )
        .into_response()
}
async fn icon() -> Response {
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "image/svg+xml")],
        ICON,
    )
        .into_response()
}
fn css_resp(s: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (axum::http::header::CACHE_CONTROL, "no-cache"),
        ],
        s,
    )
        .into_response()
}
async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"ok": false, "error": "introuvable"})),
    )
        .into_response()
}

// ---- endpoints -----------------------------------------------------------

async fn status(State(app): State<Arc<App>>) -> Response {
    ok(app.status_json())
}

async fn get_settings(State(app): State<Arc<App>>) -> Response {
    ok(serde_json::to_value(app.settings()).unwrap_or_default())
}

async fn update_settings(State(app): State<Arc<App>>, Json(patch): Json<Value>) -> Response {
    match app.store.write() {
        Ok(mut s) => match s.update_settings(&patch) {
            Ok(()) => {
                let s2 = s.settings.clone();
                app.hub.set_verbose(s2.logs.level == "verbose");
                if s2.logs.to_file {
                    let p = if s2.logs.file_path.is_empty() {
                        app.dir.join("logs").join("zilla.log")
                    } else {
                        PathBuf::from(&s2.logs.file_path)
                    };
                    app.hub.set_file(Some(&p));
                } else {
                    app.hub.set_file(None);
                }
                app.hub.success("general", "Réglages enregistrés.");
                app.hub.publish(Event::dirty("settings"));
                ok(serde_json::to_value(s2).unwrap_or_default())
            }
            Err(e) => err(e),
        },
        Err(e) => err(e),
    }
}

async fn get_log(
    State(app): State<Arc<App>>,
    Query(q): Query<Since>,
) -> Response {
    let (seq, lines) = app.logs(q.since.unwrap_or(0));
    ok(json!({ "seq": seq, "lines": lines }))
}

async fn save_log(State(app): State<Arc<App>>) -> Response {
    let dump = app.hub.dump();
    let txt: String = dump
        .iter()
        .map(|l| format!("[{}] {:>7} {}\n", l.ts, l.level, l.text))
        .collect();
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"zilla.log\"",
            ),
        ],
        txt,
    )
        .into_response()
}

async fn clear_log(State(app): State<Arc<App>>) -> Response {
    app.hub.clear();
    ok(json!(true))
}

#[derive(Deserialize)]
struct Since {
    since: Option<i64>,
}

#[derive(Deserialize)]
struct ListQ {
    site: Option<String>,
    path: Option<String>,
    hidden: Option<bool>,
}

async fn get_sites(State(app): State<Arc<App>>) -> Response {
    let sites = app.sites();
    let connected: Vec<String> = app
        .pools
        .list_connected()
        .into_iter()
        .filter(|(_, l, _)| *l > 0)
        .map(|(i, _, _)| i)
        .collect();
    ok(json!({ "sites": sites, "connected": connected }))
}

async fn save_site(State(app): State<Arc<App>>, Json(site): Json<Site>) -> Response {
    if site.host.trim().is_empty() {
        return err("le nom d'hôte est obligatoire");
    }
    let s = app.upsert_site(site);
    ok(s)
}

async fn delete_site(State(app): State<Arc<App>>, Json(q): Json<Idq>) -> Response {
    app.disconnect(&q.id);
    app.delete_site(&q.id);
    ok(json!(true))
}

async fn copy_site(State(app): State<Arc<App>>, Json(q): Json<Idq>) -> Response {
    match app.copy_site(&q.id) {
        Some(s) => ok(s),
        None => err("site introuvable"),
    }
}

async fn import_sites(State(app): State<Arc<App>>, Json(b): Json<XmlBody>) -> Response {
    match app.import_sites(&b.xml) {
        Ok(n) => {
            app.hub.success("general", format!("{n} site(s) importé(s)."));
            ok(json!({ "imported": n }))
        }
        Err(e) => err(e),
    }
}

async fn export_sites(State(app): State<Arc<App>>) -> Response {
    let xml = app.export_sites();
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"sitemanager.xml\"",
            ),
        ],
        xml,
    )
        .into_response()
}

async fn get_recents(State(app): State<Arc<App>>) -> Response {
    let s = app.store.read().unwrap();
    ok(json!({
        "hosts": s.recent_hosts,
        "local": s.recent_local,
        "remote": s.recent_remote,
    }))
}

async fn get_bookmarks(State(app): State<Arc<App>>) -> Response {
    ok(app.bookmarks())
}

async fn add_bookmark(State(app): State<Arc<App>>, Json(b): Json<Bookmark>) -> Response {
    ok(app.add_bookmark(b))
}

async fn delete_bookmark(State(app): State<Arc<App>>, Json(q): Json<Idq>) -> Response {
    app.remove_bookmark(&q.id);
    ok(json!(true))
}

#[derive(Deserialize)]
struct Idq {
    id: String,
}

#[derive(Deserialize)]
struct XmlBody {
    xml: String,
}

#[derive(Deserialize)]
struct ConnectBody {
    site_id: Option<String>,
    host: Option<String>,
    user: Option<String>,
    password: Option<String>,
    port: Option<u16>,
    proto: Option<String>,
}

async fn connect(State(app): State<Arc<App>>, Json(b): Json<ConnectBody>) -> Response {
    if let Some(id) = &b.site_id {
        return match app.site(id) {
            Some(site) => match app.connect(&site).await {
                Ok(info) => ok(info),
                Err(e) => err(e),
            },
            None => err("site introuvable"),
        };
    }
    let host = match &b.host {
        Some(h) if !h.trim().is_empty() => h.clone(),
        _ => return err("hôte ou site_id attendu"),
    };
    match app
        .quickconnect(
            &host,
            b.user.as_deref().unwrap_or(""),
            b.password.as_deref().unwrap_or(""),
            b.port,
            b.proto.as_deref().unwrap_or("ftp"),
        )
        .await
    {
        Ok((id, info)) => ok(json!({ "site_id": id, "info": info })),
        Err(e) => err(e),
    }
}

async fn disconnect(State(app): State<Arc<App>>, Json(q): Json<Idq>) -> Response {
    app.disconnect(&q.id);
    ok(json!(true))
}

async fn connections(State(app): State<Arc<App>>) -> Response {
    let infos = app.site_infos().await;
    ok(infos)
}

async fn remote_list(State(app): State<Arc<App>>, Query(q): Query<ListQ>) -> Response {
    let site = q.site.unwrap_or_default();
    let path = q.path.unwrap_or_else(|| "/".into());
    match app.remote_list(&site, &path).await {
        Ok(l) => ok(l),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct RemoteOpBody {
    site: String,
    op: RemoteOp,
}

async fn remote_op(State(app): State<Arc<App>>, Json(b): Json<RemoteOpBody>) -> Response {
    match app.remote_op(&b.site, b.op).await {
        Ok(()) => ok(json!(true)),
        Err(e) => err(e),
    }
}

async fn remote_exists(State(app): State<Arc<App>>, Query(q): Query<ListQ>) -> Response {
    let site = q.site.clone().unwrap_or_default();
    let path = q.path.clone().unwrap_or_default();
    match app.site(&site) {
        Some(s) => {
            let st = app.settings();
            match app.pool_for(&s, &st).await {
                Ok(p) => match p.lease(std::time::Duration::from_secs(15)).await {
                    Ok(l) => {
                        let e = l.fs().stat(&path).await;
                        ok(json!({ "exists": e.is_ok(), "entry": e.ok() }))
                    }
                    Err(e) => err(e),
                },
                Err(e) => err(e),
            }
        }
        None => err("site introuvable"),
    }
}

async fn remote_file(State(app): State<Arc<App>>, Query(q): Query<ListQ>) -> Response {
    let site = q.site.clone().unwrap_or_default();
    let path = q.path.clone().unwrap_or_default();
    let s = match app.site(&site) {
        Some(s) => s,
        None => return err("site introuvable"),
    };
    let st = app.settings();
    let pool = match app.pool_for(&s, &st).await {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let lease = match pool.lease(std::time::Duration::from_secs(30)).await {
        Ok(l) => l,
        Err(e) => return err(e),
    };
    let fs = lease.fs().clone();
    let size = match fs.stat(&path).await {
        Ok(e) => e.size,
        Err(e) => return err(e),
    };
    let mut src = match fs.open_read(&path, 0).await {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    let mut buf = Vec::with_capacity(size.min(32 * 1024 * 1024) as usize);
    let mut tmp = vec![0u8; 65536];
    loop {
        match src.read(&mut tmp).await {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(e) => return err(e),
        }
        if buf.len() > 40 * 1024 * 1024 {
            break;
        }
    }
    let _ = src.close().await;
    drop(lease);
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    (
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, mime.to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}\"", crate::util::remote_base(&path)),
            ),
        ],
        buf,
    )
        .into_response()
}

#[derive(Deserialize)]
struct RemoteSave {
    site: String,
    path: String,
    content: String,
    #[serde(default)]
    base64: bool,
}

async fn remote_save(State(app): State<Arc<App>>, Json(b): Json<RemoteSave>) -> Response {
    let site = match app.site(&b.site) {
        Some(s) => s,
        None => return err("site introuvable"),
    };
    let st = app.settings();
    let pool = match app.pool_for(&site, &st).await {
        Ok(p) => p,
        Err(e) => return err(e),
    };
    let lease = match pool.lease(std::time::Duration::from_secs(30)).await {
        Ok(l) => l,
        Err(e) => return err(e),
    };
    let data = if b.base64 {
        match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b.content) {
            Ok(d) => d,
            Err(e) => return err(e),
        }
    } else {
        b.content.as_bytes().to_vec()
    };
    let mut sink = match lease.fs().open_write(&b.path, 0, false).await {
        Ok(s) => s,
        Err(e) => return err(e),
    };
    if let Err(e) = sink.write(&data).await {
        return err(e);
    }
    if let Err(e) = sink.finish().await {
        return err(e);
    }
    app.hub.success("file", format!("Enregistré sur le serveur : {}", b.path));
    drop(lease);
    app.hub.publish(Event::dirty("remote"));
    ok(json!(true))
}

async fn remote_search(State(app): State<Arc<App>>, Query(q): Query<SearchQ>) -> Response {
    let site = q.site.unwrap_or_default();
    let root = q.root.unwrap_or_else(|| "/".into());
    match app
        .remote_search(&site, &root, &q.pattern, q.max.unwrap_or(300))
        .await
    {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

async fn local_list(State(app): State<Arc<App>>, Query(q): Query<ListQ>) -> Response {
    let path = q.path.unwrap_or_else(|| "/".into());
    let hidden = q.hidden.unwrap_or(false);
    match app.local_list(&path, hidden) {
        Ok(l) => ok(l),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct LocalOpBody {
    op: LocalOp,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum LocalOp {
    Mkdir { path: String, #[serde(default)] recursive: bool },
    Remove { path: String, #[serde(default)] recursive: bool },
    Rename { from: String, to: String },
    Chmod { path: String, mode: u32 },
    Touch { path: String },
    SetTimes { path: String, mtime: i64 },
}

async fn local_op(State(app): State<Arc<App>>, Json(b): Json<LocalOpBody>) -> Response {
    let r = match b.op {
        LocalOp::Mkdir { path, recursive } => {
            app.hub.info("file", format!("Création du dossier local {path}"));
            crate::local::mkdir(Path::new(&path), recursive).await
        }
        LocalOp::Remove { path, recursive } => {
            app.hub.info("file", format!("Suppression locale {path}"));
            crate::local::remove(Path::new(&path), recursive).await
        }
        LocalOp::Rename { from, to } => {
            app.hub.info("file", format!("Renommage local {from} → {to}"));
            crate::local::rename(Path::new(&from), Path::new(&to)).await
        }
        LocalOp::Chmod { path, mode } => {
            app.hub.info("file", format!("Permissions locales de {path} : {mode:o}"));
            crate::local::chmod(Path::new(&path), mode).await
        }
        LocalOp::Touch { path } => crate::local::touch(Path::new(&path)).await,
        LocalOp::SetTimes { path, mtime } => crate::local::set_mtime(Path::new(&path), mtime),
    };
    match r {
        Ok(()) => {
            app.hub.publish(Event::dirty("local"));
            ok(json!(true))
        }
        Err(e) => err(e),
    }
}

async fn local_file(State(_app): State<Arc<App>>, Query(q): Query<ListQ>) -> Response {
    let path = q.path.unwrap_or_default();
    let p = PathBuf::from(&path);
    let meta = match tokio::fs::metadata(&p).await {
        Ok(m) => m,
        Err(e) => return err(e),
    };
    if meta.len() > 40 * 1024 * 1024 {
        return err("fichier trop volumineux pour l'aperçu");
    }
    let bytes = match tokio::fs::read(&p).await {
        Ok(b) => b,
        Err(e) => return err(e),
    };
    let mime = mime_guess::from_path(&p).first_or_octet_stream();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, mime.to_string())],
        bytes,
    )
        .into_response()
}

#[derive(Deserialize)]
struct SaveBody {
    path: String,
    content: String,
    #[serde(default)]
    base64: bool,
}

async fn local_save(State(app): State<Arc<App>>, Json(b): Json<SaveBody>) -> Response {
    let data = if b.base64 {
        match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b.content) {
            Ok(d) => d,
            Err(e) => return err(e),
        }
    } else {
        b.content.as_bytes().to_vec()
    };
    let p = PathBuf::from(&b.path);
    if let Some(par) = p.parent() {
        let _ = tokio::fs::create_dir_all(par).await;
    }
    match tokio::fs::write(&p, data).await {
        Ok(()) => {
            app.hub.success("file", format!("Enregistré : {}", p.display()));
            app.hub.publish(Event::dirty("local"));
            ok(json!(true))
        }
        Err(e) => err(e),
    }
}

async fn local_search(State(app): State<Arc<App>>, Query(q): Query<SearchQ>) -> Response {
    let root = q.root.unwrap_or_else(|| "/".into());
    let v = app.local_search(&root, &q.pattern, q.max.unwrap_or(300));
    ok(v)
}

#[derive(Deserialize)]
struct SearchQ {
    site: Option<String>,
    root: Option<String>,
    pattern: String,
    max: Option<usize>,
}

async fn local_roots(State(_app): State<Arc<App>>) -> Response {
    let v = crate::local::roots();
    let infos: Vec<Value> = v
        .iter()
        .map(|r| {
            let (t, f) = crate::local::disk_info(Path::new(r)).unwrap_or((0, 0));
            json!({ "path": r, "total": t, "free": f })
        })
        .collect();
    let home = crate::util::home_dir().map(|h| h.to_string_lossy().to_string());
    ok(json!({ "roots": infos, "home": home }))
}

async fn local_tree(Query(q): Query<ListQ>) -> Response {
    let path = q.path.unwrap_or_else(|| "/".into());
    ok(crate::local::tree(Path::new(&path), 1))
}

#[derive(Deserialize)]
struct EnqueueBody {
    site: String,
    paths: Vec<String>,
    dir: String,
    #[serde(default)]
    dirs: bool,
}

async fn enqueue_download(State(app): State<Arc<App>>, Json(b): Json<EnqueueBody>) -> Response {
    match app.enqueue_download(&b.site, &b.paths, &b.dir, b.dirs).await {
        Ok(jobs) => ok(json!({ "added": b.paths.len(), "jobs": jobs })),
        Err(e) => err(e),
    }
}

async fn enqueue_upload(State(app): State<Arc<App>>, Json(b): Json<EnqueueBody>) -> Response {
    match app.enqueue_upload(&b.site, &b.paths, &b.dir, b.dirs).await {
        Ok(jobs) => ok(json!({ "added": b.paths.len(), "jobs": jobs })),
        Err(e) => err(e),
    }
}

async fn transfers(State(app): State<Arc<App>>) -> Response {
    let jobs = app.transfers.snapshot();
    ok(json!({ "jobs": jobs, "stats": app.transfers.stats() }))
}

#[derive(Deserialize)]
struct Jobq {
    id: String,
}

async fn cancel_job(State(app): State<Arc<App>>, Json(q): Json<Jobq>) -> Response {
    app.transfers.cancel(&q.id);
    app.transfers.notify();
    app.hub.publish(Event::dirty("queue"));
    ok(json!(true))
}

async fn pause_job(State(app): State<Arc<App>>, Json(q): Json<Jobq>) -> Response {
    app.transfers.pause(&q.id);
    app.hub.publish(Event::dirty("queue"));
    ok(json!(true))
}

#[derive(Deserialize)]
struct Prioq {
    id: String,
    priority: String,
}

async fn priority_job(State(app): State<Arc<App>>, Json(q): Json<Prioq>) -> Response {
    let p = match q.priority.as_str() {
        "low" => Priority::Low,
        "high" => Priority::High,
        _ => Priority::Normal,
    };
    ok(json!(app.transfers.set_priority(&q.id, p)))
}

async fn retry_job(State(app): State<Arc<App>>, Json(q): Json<Jobq>) -> Response {
    let n = if q.id == "all" {
        app.transfers.retry_all_failed()
    } else {
        if app.transfers.retry(&q.id) {
            1
        } else {
            0
        }
    };
    app.transfers.notify();
    ok(json!({ "retried": n }))
}

#[derive(Deserialize)]
struct ClearBody {
    #[serde(default = "default_what")]
    what: String,
}

fn default_what() -> String {
    "done".into()
}

async fn clear_jobs(State(app): State<Arc<App>>, Json(b): Json<ClearBody>) -> Response {
    let n = match b.what.as_str() {
        "all" => {
            app.transfers.clear_all();
            usize::MAX
        }
        "failed" => app.transfers.clear_failed(),
        _ => app.transfers.clear_done(),
    };
    app.hub.publish(Event::dirty("queue"));
    ok(json!({ "cleared": n }))
}

async fn pause_all(State(app): State<Arc<App>>, Json(b): Json<Value>) -> Response {
    let on = b.get("paused").and_then(|v| v.as_bool()).unwrap_or(true);
    app.transfers.set_paused(on);
    app.hub.status(if on { "File d'attente mise en pause." } else { "File d'attente reprise." });
    ok(json!({ "paused": on }))
}

async fn selftest(State(app): State<Arc<App>>) -> Response {
    let r = crate::selftest::run(app.version).await;
    ok(r)
}

// ---- websocket -----------------------------------------------------------

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(app): State<Arc<App>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws(socket, app))
}

async fn handle_ws(mut socket: WebSocket, app: Arc<App>) {
    let mut rx = app.hub.subscribe();
    let (_seq, lines) = app.logs(0);
    let hello = json!({
        "kind": "hello",
        "lines": lines,
        "status": app.status_json(),
    });
    let _ = socket
        .send(Message::Text(Utf8Bytes::from(hello.to_string())))
        .await;
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(4));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            ev = rx.recv() => {
                match ev {
                    Ok(e) => {
                        let s = serde_json::to_string(&e).unwrap_or_default();
                        if socket.send(Message::Text(Utf8Bytes::from(s))).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = ticker.tick() => {
                let msg = json!({ "kind": "ping", "status": app.status_json() });
                if socket.send(Message::Text(Utf8Bytes::from(msg.to_string()))).await.is_err() {
                    break;
                }
            }
            m = socket.recv() => {
                match m {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(_))) => {
                        let _ = socket.send(Message::Pong(axum::body::Bytes::new())).await;
                    }
                    Some(Ok(Message::Text(t))) => {
                        if t.contains("refresh") {
                            let s = json!({ "kind": "status", "data": app.status_json() });
                            let _ = socket.send(Message::Text(Utf8Bytes::from(s.to_string()))).await;
                        }
                    }
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

// Petites entrées typées manquantes pour les requêtes.
#[derive(Deserialize)]
#[allow(dead_code)]
struct LocalPath {
    path: String,
}
