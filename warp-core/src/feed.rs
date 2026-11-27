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
    /// What the seat is really doing: busy, idle, away, not set up, ...
    /// (`shown` can say "spent" for a seat whose state is away.)
    pub state: String,
    pub waiting: u64,
    pub waiting_normal: u64,
    pub oldest_waiting_s: f64,
    pub since_s: f64,
    pub held_by_draft: bool,
    pub paused: bool,
    /// The seat's meters from its usage file, as percentages.
    pub ctx: Option<f64>,
    pub five_hour: Option<f64>,
    pub week: Option<f64>,
}

impl SeatInfo {
    /// The fullest of the three meters, if the seat reports any.
    pub fn fullest(&self) -> Option<f64> {
        [self.ctx, self.five_hour, self.week].into_iter().flatten().reduce(f64::max)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteInfo {
    pub from: String,
    /// The seats it goes to; empty with `all` set means everyone.
    pub to: Vec<String>,
    pub all: bool,
    pub quiet: bool,
    /// How long the note is, in characters. The text itself never leaves
    /// the feed: the screen shows that notes fly, not what they say.
    pub len: usize,
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
            let state = s.get("state").and_then(Value::as_str).unwrap_or("away").to_string();
            let shown = s.get("shown").and_then(Value::as_str).unwrap_or(&state).to_string();
            let num = |k: &str| s.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            let flag = |k: &str| s.get(k).and_then(Value::as_bool).unwrap_or(false);
            let seat = SeatInfo {
                name: seat_id(name),
                shown,
                state,
                waiting: s.get("waiting").and_then(Value::as_u64).unwrap_or(0),
                waiting_normal: s.get("waiting_normal").and_then(Value::as_u64).unwrap_or(0),
                oldest_waiting_s: num("oldest_waiting_s"),
                since_s: num("since_s"),
                held_by_draft: flag("held_by_draft"),
                paused: flag("paused"),
                ..Default::default()
            };
            (rank, seat)
        })
        .collect();
    seats.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    seats.into_iter().map(|(_, s)| s).collect()
}

/// One log line, if it is a note the screen may show (private ones never are).
impl SeatInfo {
    /// Is the agent running at all? Away, not set up and paused seats are
    /// not on the warp core; everyone else is, even if spent or stuck.
    pub fn online(&self) -> bool {
        !matches!(self.state.as_str(), "away" | "not set up" | "paused" | "offline" | "")
    }
}

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
    let quiet = v.get("quiet").and_then(Value::as_bool).unwrap_or(false);
    let len = v.get("text").and_then(Value::as_str).map(|t| t.chars().count()).unwrap_or(0);
    Some(NoteInfo { from, to, all, quiet, len })
}

/// A seat's meters from usage/<seat>.json (the statusline shape the
/// terminal deck reads): context, 5-hour and weekly used percentages.
pub fn parse_usage(v: &Value) -> (Option<f64>, Option<f64>, Option<f64>) {
    let d = v.get("data").unwrap_or(&Value::Null);
    let pct = |x: Option<&Value>| x.and_then(|x| x.get("used_percentage")).and_then(Value::as_f64);
    let limits = d.get("rate_limits");
    (
        pct(d.get("context_window")),
        pct(limits.and_then(|l| l.get("five_hour"))),
        pct(limits.and_then(|l| l.get("seven_day"))),
    )
}

fn add_usage(seats: &mut [SeatInfo], dir: &std::path::Path) {
    for seat in seats {
        let path = dir.join(format!("{}.json", seat.name));
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        (seat.ctx, seat.five_hour, seat.week) = parse_usage(&v);
    }
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
                Ok(mut seats) => {
                    add_usage(&mut seats, &data_dir().join("usage"));
                    Msg::Seats(seats)
                }
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
    fn only_running_seats_are_online() {
        let v = json!({"seats": {
            "claude": {"shown": "busy", "state": "busy", "rank": [1]},
            "vibe": {"shown": "stuck", "state": "idle", "rank": [2]},
            "codex": {"shown": "spent", "state": "away", "rank": [3]},
            "lumo": {"shown": "not set up", "state": "not set up", "rank": [4]}
        }});
        let online: Vec<_> = parse_status(&v).into_iter().filter(|s| s.online()).map(|s| s.name).collect();
        assert_eq!(online, ["claude", "vibe"], "spent-but-away codex stays off");
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
    fn queues_and_meters_come_through() {
        let v = json!({"seats": {"codex": {"state": "away", "shown": "spent", "rank": [1],
            "waiting": 103, "oldest_waiting_s": 241499.0, "held_by_draft": true}}});
        let s = &parse_status(&v)[0];
        assert_eq!((s.waiting, s.oldest_waiting_s, s.held_by_draft), (103, 241499.0, true));
        let u = json!({"data": {"context_window": {"used_percentage": 45.0},
            "rate_limits": {"five_hour": {"used_percentage": 30.0}, "seven_day": {"used_percentage": 92.5}}}});
        assert_eq!(parse_usage(&u), (Some(45.0), Some(30.0), Some(92.5)));
        let seat = SeatInfo { ctx: Some(45.0), week: Some(92.5), ..Default::default() };
        assert_eq!(seat.fullest(), Some(92.5));
        assert_eq!(SeatInfo::default().fullest(), None, "no meters is unknown, not zero");
    }

    #[test]
    fn a_note_carries_its_length_not_its_text() {
        let n = parse_note(&json!({"kind": "message", "from": "a", "to": "b", "text": "héllo", "quiet": true})).unwrap();
        assert_eq!((n.len, n.quiet), (5, true));
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
