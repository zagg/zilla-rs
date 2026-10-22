//! Diffuseur de journal + bus d'événements temps réel.

use crate::models::LogLine;
use crate::util::{now_ms, stamp};
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;

/// Événement temps réel envoyé au(x) client(s) web.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub kind: String,
    pub data: Value,
}

impl Event {
    pub fn log(line: &LogLine) -> Event {
        Event {
            kind: "log".into(),
            data: serde_json::to_value(line).unwrap_or(Value::Null),
        }
    }
    pub fn dirty(what: &str) -> Event {
        Event {
            kind: "dirty".into(),
            data: serde_json::json!({ "what": what }),
        }
    }
    pub fn status(v: Value) -> Event {
        Event {
            kind: "status".into(),
            data: v,
        }
    }
    pub fn toast(level: &str, text: &str) -> Event {
        Event {
            kind: "toast".into(),
            data: serde_json::json!({ "level": level, "text": text }),
        }
    }
    pub fn job_done(id: &str, ok: bool, text: &str) -> Event {
        Event {
            kind: "job".into(),
            data: serde_json::json!({ "id": id, "ok": ok, "text": text }),
        }
    }
}

#[derive(Clone)]
pub struct LogHub {
    tx: broadcast::Sender<Event>,
    ring: Arc<RwLock<VecDeque<LogLine>>>,
    counter: Arc<AtomicU64>,
    seq: Arc<AtomicI64>,
    file: Arc<RwLock<Option<(PathBuf, std::fs::File)>>>,
    verbose: Arc<std::sync::atomic::AtomicBool>,
}

impl Default for LogHub {
    fn default() -> Self {
        Self::new(5000)
    }
}

impl LogHub {
    pub fn new(_capacity: usize) -> LogHub {
        let (tx, _) = broadcast::channel(2048);
        LogHub {
            tx,
            ring: Arc::new(RwLock::new(VecDeque::new())),
            counter: Arc::new(AtomicU64::new(0)),
            seq: Arc::new(AtomicI64::new(0)),
            file: Arc::new(RwLock::new(None)),
            verbose: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn set_capacity(&self, capacity: usize) {
        let mut r = self.ring.write().unwrap();
        let _ = capacity;
        while r.len() > 4000 {
            r.pop_front();
        }
    }

    pub fn set_verbose(&self, on: bool) {
        self.verbose.store(on, Ordering::Relaxed);
    }

    pub fn verbose(&self) -> bool {
        self.verbose.load(Ordering::Relaxed)
    }

    /// Ouvre (ou bascule vers) un fichier journal.
    pub fn set_file(&self, path: Option<&Path>) {
        let mut f = self.file.write().unwrap();
        *f = match path {
            Some(p) => {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match OpenOptions::new().create(true).append(true).open(p) {
                    Ok(file) => Some((p.to_path_buf(), file)),
                    Err(_) => None,
                }
            }
            None => None,
        };
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    pub fn publish(&self, ev: Event) {
        let _ = self.tx.send(ev);
    }

    /// Nombre de lignes en mémoire.
    pub fn len(&self) -> usize {
        self.ring.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Journal en mémoire, `since` = index de la dernière ligne vue.
    pub fn snapshot(&self, since: i64, limit: usize) -> (i64, Vec<LogLine>) {
        let r = self.ring.read().unwrap();
        let cur = self.seq.load(Ordering::Relaxed);
        let start = if since <= 0 {
            0
        } else if since >= cur {
            return (cur, Vec::new());
        } else {
            (r.len() as i64 - (cur - since)).max(0) as usize
        };
        let take = limit.min(r.len().saturating_sub(start));
        let from = r.len() - take;
        (cur, r.iter().skip(from).cloned().collect())
    }

    /// Tout le contenu, pour le bouton « enregistrer le journal ».
    pub fn dump(&self) -> Vec<LogLine> {
        self.ring.read().unwrap().iter().cloned().collect()
    }

    pub fn log(&self, line: LogLine) {
        let trace = line.level == "trace";
        if trace && !self.verbose() {
            return;
        }
        if self.file.read().unwrap().is_some() && (!trace || self.verbose()) {
            let mut w = self.file.write().unwrap();
            if let Some((_, f)) = w.as_mut() {
                let _ = writeln!(f, "[{}] {:>7} {}", stamp(), line.level, line.text);
            }
        }
        {
            let mut r = self.ring.write().unwrap();
            let seq = self.seq.load(Ordering::Relaxed) + 1;
            self.seq.store(seq, Ordering::Relaxed);
            r.push_back(line.clone());
            while r.len() > 5000 {
                r.pop_front();
            }
            let _ = seq;
        }
        self.counter.fetch_add(1, Ordering::Relaxed);
        self.publish(Event::log(&line));
    }

    pub fn clear(&self) {
        let mut r = self.ring.write().unwrap();
        r.clear();
    }

    fn push(&self, cat: &str, level: &str, text: impl Into<String>) {
        self.log(LogLine::new(cat, level, text));
    }

    pub fn info(&self, cat: &str, text: impl Into<String>) {
        self.push(cat, "info", text);
    }
    pub fn success(&self, cat: &str, text: impl Into<String>) {
        self.push(cat, "success", text);
    }
    pub fn warn(&self, cat: &str, text: impl Into<String>) {
        self.push(cat, "warn", text);
    }
    pub fn err(&self, cat: &str, text: impl Into<String>) {
        self.push(cat, "error", text);
    }
    pub fn status(&self, text: impl Into<String>) {
        self.push("status", "status", text);
    }
    pub fn trace(&self, cat: &str, text: impl Into<String>) {
        self.push(cat, "trace", text);
    }

    /// Une ligne « statut » avec un compteur de connexions.
    pub fn status_line(&self, conns: usize) {
        self.push("status", "status", format!("Nombre de connexions actives : {conns}"));
    }

    pub fn total_lines(&self) -> u64 {
        self.counter.load(Ordering::Relaxed)
    }
}

impl LogHub {
    /// Écrire un en-tête au démarrage, comme le fait FileZilla.
    pub fn banner(&self, version: &str, config_dir: &Path) {
        self.info(
            "general",
            format!("Zilla-RS {version} démarré (config : {})", config_dir.display()),
        );
    }
}

/// Petit utilitaire : horodatage relatif.
pub fn age(ts: i64) -> i64 {
    (now_ms() - ts).max(0)
}
