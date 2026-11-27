//! What the warp core shows, read from what the comms-deck service already
//! writes: `comms-deck status --json` for the seats, and new lines of
//! log.jsonl for the notes flying between them. Read-only, and on its own
//! thread, so a slow `comms-deck` call never drops a frame.
//!
//! Written by: claude

use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeatInfo {
    pub name: String,
    /// What the deck shows: busy, ready, idle, stuck, spent, away, ...
    pub shown: String,
    pub waiting: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoteInfo {
    pub from: String,
    /// The seats it goes to; empty with `all` set means everyone.
    pub to: Vec<String>,
    pub all: bool,
}

pub enum Msg {
    Seats(Vec<SeatInfo>),
    Note(NoteInfo),
    Problem(String),
}

/// Where the service keeps its files ($COMMS_DECK_HOME, as in comms-deck).
fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("COMMS_DECK_HOME") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    PathBuf::from(home).join(".local/share/comms-deck")
}

fn program() -> String {
    std::env::var("COMMS_DECK_BIN").unwrap_or_else(|_| "comms-deck".into())
}

/// The owner seat answers to owner, y and stox; the screen calls it Y.
pub fn seat_id(name: &str) -> String {
    match name.trim().to_lowercase().as_str() {
        "owner" | "y" | "stox" => "owner".into(),
        other => other.into(),
    }
}

/// Seats in the service's order (its `rank`), hidden ones left out.
pub fn parse_status(v: &Value) -> Vec<SeatInfo> {
    let Some(map) = v.get("seats").and_then(Value::as_object) else { return Vec::new() };
    let mut seats: Vec<(Vec<i64>, SeatInfo)> = map
        .iter()
        .filter(|(_, s)| s.get("hidden").and_then(Value::as_bool) != Some(true))
        .map(|(name, s)| {
            let rank = s
                .get("rank")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            let shown = s
                .get("shown")
                .or_else(|| s.get("state"))
                .and_then(Value::as_str)
                .unwrap_or("away")
                .to_string();
            let waiting = s.get("waiting").and_then(Value::as_u64).unwrap_or(0);
            (rank, SeatInfo { name: seat_id(name), shown, waiting })
        })
        .collect();
    seats.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    seats.into_iter().map(|(_, s)| s).collect()
}

/// One log line, if it is a note the screen may show (private ones never are).
pub fn parse_note(v: &Value) -> Option<NoteInfo> {
    if v.get("kind").and_then(Value::as_str) != Some("message") {
        return None;
    }
    if v.get("private").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let from = seat_id(v.get("from")?.as_str()?);
    let to_text = v.get("to").and_then(Value::as_str).unwrap_or("");
    let mut all = false;
    let mut to = Vec::new();
    for part in to_text.split(|c: char| c == ',' || c == '+' || c.is_whitespace()) {
        let part = part.trim();
        if part.is_empty() || part.starts_with('-') {
            continue;
        }
        if part == "all" {
            all = true;
        } else {
            let id = seat_id(part);
            if !to.contains(&id) {
                to.push(id);
            }
        }
    }
    Some(NoteInfo { from, to, all })
}

fn status() -> Result<Vec<SeatInfo>, String> {
    let out = Command::new(program())
        .args(["status", "--json"])
        .output()
        .map_err(|e| format!("couldn't run {}: {e}", program()))?;
    if !out.status.success() {
        return Err(format!("{} status --json failed", program()));
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("bad json: {e}"))?;
    Ok(parse_status(&v))
}

/// Follows log.jsonl from its end: only notes sent after the warp core
/// started fly, so a restart never replays the past.
struct LogTail {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl LogTail {
    fn new(path: PathBuf) -> Self {
        let offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        LogTail { path, offset, partial: String::new() }
    }

    fn read_new(&mut self) -> Vec<Value> {
        let Ok(mut f) = File::open(&self.path) else { return Vec::new() };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // The log was rotated or truncated: start again from its top.
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = Vec::new();
        if f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        self.offset += buf.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&buf));
        let mut lines: Vec<String> = self.partial.split('\n').map(String::from).collect();
        // The last piece has no newline yet: keep it for the next read.
        self.partial = lines.pop().unwrap_or_default();
        lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }
}

/// Runs forever on its own thread; stops when the window has gone.
pub fn run(tx: Sender<Msg>) {
    let mut tail = LogTail::new(data_dir().join("log.jsonl"));
    let mut last_status = Instant::now() - Duration::from_secs(10);
    loop {
        if last_status.elapsed() >= Duration::from_secs(2) {
            last_status = Instant::now();
            let msg = match status() {
                Ok(seats) => Msg::Seats(seats),
                Err(e) => Msg::Problem(e),
            };
            if tx.send(msg).is_err() {
                return;
            }
        }
        for v in tail.read_new() {
            if let Some(note) = parse_note(&v) {
                if tx.send(Msg::Note(note)).is_err() {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_keeps_rank_order_and_drops_hidden_seats() {
        let v = json!({"seats": {
            "grok": {"shown": "away", "rank": [5, 12], "waiting": 12},
            "claude": {"shown": "busy", "rank": [1, 0], "waiting": 0},
            "codex": {"shown": "away", "rank": [6, 0], "hidden": true},
            "owner": {"state": "ready", "rank": [0, 0]}
        }});
        let seats = parse_status(&v);
        let names: Vec<_> = seats.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["owner", "claude", "grok"]);
        assert_eq!(seats[0].shown, "ready", "state stands in when shown is missing");
        assert_eq!(seats[2].waiting, 12);
    }

    #[test]
    fn notes_split_their_recipients() {
        let n = parse_note(&json!({"kind": "message", "from": "claude", "to": "devin, vibe"})).unwrap();
        assert_eq!(n.to, ["devin", "vibe"]);
        let n = parse_note(&json!({"kind": "message", "from": "claude", "to": "devin+vibe"})).unwrap();
        assert_eq!(n.to, ["devin", "vibe"]);
        let n = parse_note(&json!({"kind": "message", "from": "Y", "to": "all -grok"})).unwrap();
        assert!(n.all && n.to.is_empty());
        assert_eq!(n.from, "owner");
    }

    #[test]
    fn private_notes_and_commentary_never_fly() {
        assert!(parse_note(&json!({"kind": "message", "from": "a", "to": "b", "private": true})).is_none());
        assert!(parse_note(&json!({"kind": "commentary", "from": "a", "to": "b"})).is_none());
    }

    #[test]
    fn the_tail_reads_only_new_whole_lines() {
        let dir = std::env::temp_dir().join(format!("warp-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        std::fs::write(&path, "{\"kind\":\"message\",\"from\":\"old\",\"to\":\"x\"}\n").unwrap();
        let mut tail = LogTail::new(path.clone());
        assert!(tail.read_new().is_empty(), "the past is not replayed");
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        write!(f, "{{\"kind\":\"message\",\"from\":\"a\",\"to\":\"b\"}}\n{{\"kind\":").unwrap();
        assert_eq!(tail.read_new().len(), 1, "a half-written line waits");
        writeln!(f, "\"message\",\"from\":\"c\",\"to\":\"d\"}}").unwrap();
        assert_eq!(tail.read_new().len(), 1, "and arrives once finished");
        std::fs::remove_dir_all(&dir).ok();
    }
}
