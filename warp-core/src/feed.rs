//! What the warp core shows, read from what the comms-deck service already
//! writes: `comms-deck status --json` for the seats, and new lines of
//! log.jsonl for the notes flying between them. Read-only, and on its own
//! thread, so a slow `comms-deck` call never drops a frame.
//!
//! Note text reaches the screen only through the chat panel, under the
//! terminal deck's rules: messages only, private ones never, and scrubbed of
//! anything a renderer might act on. Comets carry who and when, not text.
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
    /// Tokens in the seat's context window, for the stars' split.
    pub ctx_tokens: Option<f64>,
    /// The session's cost as the seat reports it (an estimate).
    pub cost_usd: Option<f64>,
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

/// A note for the chat panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatNote {
    pub from: String,
    /// As written: a seat, "all", "vibe, devin", "all -grok".
    pub to: String,
    /// "2026-11-27 03:04:16", the service's local time.
    pub time: String,
    pub text: String,
    pub quiet: bool,
}

/// A change of a seat's state (activity.jsonl), as the chat screen's ACTIVITY.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Activity {
    pub time: f64,
    pub seat: String,
    pub state: String,
    pub doing: String,
}

/// A desktop notification (notifications.jsonl), as the chat screen shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notice {
    pub time: f64,
    pub text: String,
}

/// One task from the load balancer's board (tasks/<id>.json): who asked for
/// it and who holds it. The summary never leaves the feed, like note text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskInfo {
    pub id: String,
    /// The seat that asked for it: the ring colours tasks by this.
    pub from: String,
    /// The seat carrying it, if any (None while it orbits unassigned).
    pub holder: Option<String>,
    pub open: bool,
}

pub enum Msg {
    Seats(Vec<SeatInfo>),
    /// `true` when it just happened (it ripples the ring), false from the backlog.
    Activity(Activity, bool),
    Notice(Notice),
    Note(NoteInfo),
    Chat(ChatNote),
    Tasks(Vec<TaskInfo>),
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

/// Text from agents, remote seats and Twitch strangers: drop C0/C1 controls,
/// DEL, zero-width and bidi marks and the BOM (the deck's scrub.rs). Tabs
/// and newlines become spaces.
pub fn scrub(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|&c| {
            let u = c as u32;
            !(u < 0x20
                || (0x7f..=0x9f).contains(&u)
                || (0x200b..=0x200f).contains(&u)
                || (0x202a..=0x202e).contains(&u)
                || (0x2060..=0x2069).contains(&u)
                || u == 0xfeff)
        })
        .collect()
}

/// A log line for the chat panel: the deck's rules (messages, not private).
pub fn parse_chat(v: &Value) -> Option<ChatNote> {
    if v.get("kind").and_then(Value::as_str) != Some("message") {
        return None;
    }
    if v.get("private").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let s = |k: &str| scrub(v.get(k).and_then(Value::as_str).unwrap_or(""));
    Some(ChatNote {
        from: seat_id(&s("from")),
        to: s("to"),
        time: s("time"),
        text: s("text"),
        quiet: v.get("quiet").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// The last `n` chat notes already in the log, read from its tail, so the
/// panel opens with the conversation instead of empty.
pub fn backlog(path: &std::path::Path, n: usize) -> Vec<ChatNote> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(512 * 1024);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // likely cut in half
    }
    let notes: Vec<ChatNote> =
        lines.filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| parse_chat(&v)).collect();
    notes[notes.len().saturating_sub(n)..].to_vec()
}

pub fn parse_activity(v: &Value) -> Option<Activity> {
    Some(Activity {
        time: v.get("time")?.as_f64()?,
        seat: seat_id(&scrub(v.get("seat")?.as_str()?)),
        state: scrub(v.get("state").and_then(Value::as_str).unwrap_or("")),
        doing: scrub(v.get("doing").and_then(Value::as_str).unwrap_or("")),
    })
}

pub fn parse_notice(v: &Value) -> Option<Notice> {
    Some(Notice { time: v.get("time")?.as_f64()?, text: scrub(v.get("text")?.as_str()?) })
}

/// The last `n` parsed lines of a jsonl file's tail.
fn tail_lines<T>(path: &std::path::Path, n: usize, parse: fn(&Value) -> Option<T>) -> Vec<T> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(256 * 1024);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next();
    }
    let all: Vec<T> = lines.filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter_map(|v| parse(&v)).collect();
    let skip = all.len().saturating_sub(n);
    all.into_iter().skip(skip).collect()
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

/// Tokens in context: context_tokens when the seat reports it, else used%
/// x window size (the deck's context_tokens); None when neither is there.
pub fn context_tokens(v: &Value) -> Option<f64> {
    let cw = v.get("data")?.get("context_window")?;
    if let Some(t) = cw.get("context_tokens").and_then(Value::as_f64).filter(|t| *t > 0.0) {
        return Some(t);
    }
    let pct = cw.get("used_percentage").and_then(Value::as_f64).filter(|p| *p > 0.0)?;
    let size = cw.get("context_window_size").and_then(Value::as_f64).filter(|s| *s > 0.0)?;
    Some(size * pct / 100.0)
}

fn add_usage(seats: &mut [SeatInfo], dir: &std::path::Path) {
    for seat in seats {
        let path = dir.join(format!("{}.json", seat.name));
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        (seat.ctx, seat.five_hour, seat.week) = parse_usage(&v);
        seat.ctx_tokens = context_tokens(&v);
        seat.cost_usd = v.pointer("/data/cost/total_cost_usd").and_then(Value::as_f64);
    }
}

/// One tasks/<id>.json from the load balancer: t1, its assigner and holder.
/// Done tasks and unreadable files are skipped; a task with no holder is
/// open (orbiting), one with a holder is carried (a bolt to the core).
pub fn parse_task(v: &Value) -> Option<TaskInfo> {
    let id = v.get("id").and_then(Value::as_str)?.to_string();
    if v.get("state").and_then(Value::as_str) == Some("done") {
        return None;
    }
    let from = seat_id(v.get("from").and_then(Value::as_str).unwrap_or("owner"));
    let holder = v.get("holder").and_then(Value::as_str).filter(|h| !h.is_empty()).map(seat_id);
    Some(TaskInfo { id, from, open: holder.is_none(), holder })
}

/// Every task on the board, in id order. The board is the balancer's own
/// state dir, so this shows exactly what `load-balancer tasks` shows.
pub fn read_tasks(dir: &std::path::Path) -> Vec<TaskInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut tasks: Vec<TaskInfo> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .filter_map(|v| parse_task(&v))
        .collect();
    tasks.sort_by(|a, b| nat_id(&a.id).cmp(&nat_id(&b.id)));
    tasks
}

/// "t2" < "t10": the board's ids sort by their number, not byte-wise.
fn nat_id(id: &str) -> (String, u64) {
    let digits: String = id.chars().filter(|c| c.is_ascii_digit()).collect();
    (id.trim_end_matches(|c: char| c.is_ascii_digit()).to_string(), digits.parse().unwrap_or(0))
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
    let log = data_dir().join("log.jsonl");
    let mut tail = LogTail::new(log.clone());
    for note in backlog(&log, 150) {
        if tx.send(Msg::Chat(note)).is_err() {
            return;
        }
    }
    let act_path = data_dir().join("activity.jsonl");
    let note_path = data_dir().join("notifications.jsonl");
    for a in tail_lines(&act_path, 80, parse_activity) {
        if tx.send(Msg::Activity(a, false)).is_err() {
            return;
        }
    }
    for n in tail_lines(&note_path, 40, parse_notice) {
        if tx.send(Msg::Notice(n)).is_err() {
            return;
        }
    }
    let mut act_tail = LogTail::new(act_path);
    let mut note_tail = LogTail::new(note_path);
    let mut last_status = Instant::now() - Duration::from_secs(10);
    let tasks_dir = data_dir().join("tasks");
    let mut last_tasks = Vec::new();
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
            // The board only matters when it changes, so it is read on the
            // same slow clock and compared before it is sent.
            let tasks = read_tasks(&tasks_dir);
            if tasks != last_tasks && tx.send(Msg::Tasks(tasks.clone())).is_err() {
                return;
            }
            last_tasks = tasks;
        }
        for v in tail.read_new() {
            if let Some(chat) = parse_chat(&v) {
                if tx.send(Msg::Chat(chat)).is_err() {
                    return;
                }
            }
            if let Some(note) = parse_note(&v) {
                if tx.send(Msg::Note(note)).is_err() {
                    return;
                }
            }
        }
        for v in act_tail.read_new() {
            if let Some(a) = parse_activity(&v) {
                if tx.send(Msg::Activity(a, true)).is_err() {
                    return;
                }
            }
        }
        for v in note_tail.read_new() {
            if let Some(n) = parse_notice(&v) {
                if tx.send(Msg::Notice(n)).is_err() {
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
    fn the_chat_follows_the_deck_rules() {
        let n = parse_chat(&json!({"kind": "message", "from": "Y", "to": "vibe", "time": "2026-11-27 03:04:16",
            "text": "hi\x1b[31m\nthere\u{202e}"})).unwrap();
        assert_eq!((n.from.as_str(), n.text.as_str()), ("owner", "hi[31m there"));
        assert!(parse_chat(&json!({"kind": "message", "from": "a", "to": "b", "private": true})).is_none());
        assert!(parse_chat(&json!({"kind": "commentary", "from": "a", "to": "b"})).is_none());
    }

    #[test]
    fn the_backlog_is_the_last_notes_in_order() {
        let dir = std::env::temp_dir().join(format!("warp-core-backlog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        let lines: String = (0..10)
            .map(|i| format!("{{\"kind\":\"message\",\"from\":\"a\",\"to\":\"b\",\"text\":\"{i}\"}}\n"))
            .collect();
        std::fs::write(&path, lines).unwrap();
        let got: Vec<String> = backlog(&path, 3).into_iter().map(|n| n.text).collect();
        assert_eq!(got, ["7", "8", "9"]);
        assert!(backlog(&dir.join("missing"), 3).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn context_tokens_from_either_shape() {
        assert_eq!(context_tokens(&json!({"data": {"context_window": {"context_tokens": 5000.0}}})), Some(5000.0));
        assert_eq!(
            context_tokens(&json!({"data": {"context_window": {"used_percentage": 10.0, "context_window_size": 200000.0}}})),
            Some(20000.0)
        );
        assert_eq!(context_tokens(&json!({"data": {}})), None);
    }

    #[test]
    fn activity_and_notices_parse_and_scrub() {
        let a = parse_activity(&json!({"time": 5.0, "seat": "Y", "state": "busy", "doing": "a\x1bb"})).unwrap();
        assert_eq!((a.seat.as_str(), a.doing.as_str()), ("owner", "ab"));
        assert!(parse_activity(&json!({"seat": "x"})).is_none(), "no time, no row");
        let n = parse_notice(&json!({"time": 1.0, "text": "Note from claude"})).unwrap();
        assert_eq!(n.text, "Note from claude");
    }

    #[test]
    fn private_notes_and_commentary_never_fly() {
        assert!(parse_note(&json!({"kind": "message", "from": "a", "to": "b", "private": true})).is_none());
        assert!(parse_note(&json!({"kind": "commentary", "from": "a", "to": "b"})).is_none());
    }

    #[test]
    fn a_task_keeps_its_assigner_and_holder() {
        let t = parse_task(&json!({
            "id": "t1", "from": "owner", "state": "held", "holder": "vibe",
            "summary": "an we also have, while tasks are orbiting, the bolts…"
        })).unwrap();
        assert_eq!((t.id.as_str(), t.from.as_str()), ("t1", "owner"));
        assert_eq!(t.holder.as_deref(), Some("vibe"));
        assert!(!t.open);
        assert!(parse_task(&json!({"id": "t2", "from": "Y", "state": "open"})).unwrap().open, "no holder: open");
        assert_eq!(parse_task(&json!({"id": "t3", "from": "owner", "state": "done"})), None, "done tasks are gone");
        assert!(parse_task(&json!({"state": "open"})).is_none(), "no id, no task");
        assert_eq!(parse_task(&json!({"id": "t4", "from": "y", "state": "held", "holder": ""})).unwrap().from, "owner");
    }

    #[test]
    fn the_board_reads_in_id_order_and_skips_junk() {
        let dir = std::env::temp_dir().join(format!("warp-core-tasks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t10.json"), r#"{"id": "t10", "from": "owner", "state": "open"}"#).unwrap();
        std::fs::write(dir.join("t2.json"), r#"{"id": "t2", "from": "Y", "state": "held", "holder": "claude"}"#).unwrap();
        std::fs::write(dir.join("t1.json"), r#"{"id": "t1", "from": "owner", "state": "done"}"#).unwrap();
        std::fs::write(dir.join("notes.txt"), "not a task").unwrap();
        std::fs::write(dir.join("bad.json"), "{oops").unwrap();
        let got: Vec<String> = read_tasks(&dir).into_iter().map(|t| t.id).collect();
        assert_eq!(got, ["t2", "t10"], "the board's numeric order, done and junk left out");
        std::fs::remove_dir_all(&dir).ok();
        assert!(read_tasks(&dir.join("missing")).is_empty());
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
