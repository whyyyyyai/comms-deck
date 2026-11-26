//! Everything the deck shows, read from what the comms-deck service already
//! writes. Read-only: nothing here writes a file or sends a note.

use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::scrub::scrub;

/// Where the service keeps its files ($COMMS_DECK_HOME, as in comms-deck).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("COMMS_DECK_HOME") {
        return PathBuf::from(d);
    }
    home().join(".local/share/comms-deck")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// The comms-deck program ($COMMS_DECK_BIN, else the one on PATH).
pub fn program() -> String {
    std::env::var("COMMS_DECK_BIN").unwrap_or_else(|_| "comms-deck".into())
}

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Clone, Debug, Default)]
pub struct Seat {
    pub name: String,
    pub kind: String,
    pub state: String,
    /// The word the deck shows: ready, busy, stuck, spent, away, ...
    pub shown: String,
    pub waiting: u64,
    pub since_s: f64,
    pub can_wake: bool,
    pub spent_until: Option<f64>,
    pub hidden: bool,
    pub doing: String,
}

#[derive(Clone, Debug, Default)]
pub struct Note {
    pub from: String,
    pub to: String,
    pub time: String,
    pub text: String,
    pub quiet: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Activity {
    pub time: f64,
    pub seat: String,
    pub state: String,
    pub doing: String,
}

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub model: String,
    pub context_pct: Option<f64>,
    pub five_hour_pct: Option<f64>,
    pub week_pct: Option<f64>,
    pub week_resets: Option<f64>,
    /// Tokens in context now, as comms-deck's context_split() counts them.
    pub context_tokens: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub total_kb: u64,
    pub available_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

impl Memory {
    pub fn available_pct(&self) -> f64 {
        if self.total_kb == 0 {
            return 0.0;
        }
        100.0 * self.available_kb as f64 / self.total_kb as f64
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub time: f64,
    pub seats: Vec<Seat>,
    pub notes: Vec<Note>,
    pub activity: Vec<Activity>,
    pub usage: BTreeMap<String, Usage>,
    pub memory: Memory,
    /// workspace name -> window titles on it
    pub workspaces: BTreeMap<String, Vec<String>>,
    /// Shown when a source could not be read, so a blank panel says why.
    pub problems: Vec<String>,
    /// What only the nav view reads (navdata.rs).
    pub nav: crate::navdata::NavData,
    /// The status call's own time, for NAV-TIME.
    pub status_time: f64,
    /// The epoch of the newest note that fires a warp jump — one to `all`,
    /// or one announcing a merge — if it is fresh (2 minutes). None when the
    /// log holds no such note. The App jumps once, then remembers it.
    pub jump_trigger: Option<f64>,
}

impl Snapshot {
    pub fn seat(&self, name: &str) -> Option<&Seat> {
        self.seats.iter().find(|s| s.name == name)
    }

    /// Notes sent in the last hour: the warp core's speed.
    pub fn notes_last_hour(&self) -> usize {
        let cutoff = self.time - 3600.0;
        self.activity.iter().filter(|a| a.time >= cutoff && a.state == "busy").count()
    }

    /// The epoch of the newest jump-worthy note (one to `all`, or one
    /// announcing a merge), or None. Only notes newer than `fresh` seconds
    /// count, so a restart doesn't replay an old broadcast as a jump.
    pub fn newest_jump_note(&self, fresh: f64) -> Option<f64> {
        self.notes
            .iter()
            .filter(|n| n.to == "all" || n.text.to_lowercase().contains("merged #"))
            .filter_map(|n| {
                let t = crate::navdata::event_time(Some(&Value::String(n.time.clone())));
                (t > 0.0 && self.time - t <= fresh).then_some(t)
            })
            .fold(None, |acc: Option<f64>, t| Some(acc.map_or(t, |a: f64| a.max(t))))
    }
}

/// Read everything once. Each source fails on its own: a missing file is a
/// line in `problems`, never a crash.
pub fn load() -> Snapshot {
    let dir = data_dir();
    let mut snap = Snapshot { time: now(), ..Default::default() };
    match status_json() {
        Ok(v) => {
            snap.status_time = v.get("time").and_then(Value::as_f64).unwrap_or(0.0);
            snap.seats = parse_status(&v);
        }
        Err(e) => snap.problems.push(format!("status: {e}")),
    }
    snap.notes = tail_jsonl(&dir.join("log.jsonl"), 256 * 1024).iter().filter_map(parse_note).collect();
    snap.activity = tail_jsonl(&dir.join("activity.jsonl"), 128 * 1024).iter().filter_map(parse_activity).collect();
    // The newest activity line per seat is what it is doing now.
    for a in &snap.activity {
        if let Some(s) = snap.seats.iter_mut().find(|s| s.name == a.seat) {
            s.doing = if a.state == "busy" { a.doing.clone() } else { String::new() };
        }
    }
    snap.usage = load_usage(&dir.join("usage"));
    snap.memory = read_meminfo(Path::new("/proc/meminfo"));
    snap.workspaces = hypr_workspaces();
    let waiting = snap.seats.iter().map(|s| s.waiting).sum();
    snap.nav = crate::navdata::load(waiting, 0.0, snap.status_time);
    snap.jump_trigger = snap.newest_jump_note(120.0);
    snap
}

fn status_json() -> Result<Value, String> {
    let out = Command::new(program())
        .args(["status", "--json"])
        .output()
        .map_err(|e| format!("couldn't run {}: {e}", program()))?;
    if !out.status.success() {
        return Err(format!("{} status --json failed", program()));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("bad json: {e}"))
}

pub fn parse_status(v: &Value) -> Vec<Seat> {
    let mut seats: Vec<(Vec<i64>, Seat)> = Vec::new();
    if let Some(map) = v.get("seats").and_then(Value::as_object) {
        for (name, s) in map {
            let rank = s
                .get("rank")
                .and_then(Value::as_array)
                .map(|r| r.iter().map(|x| x.as_i64().unwrap_or(99)).collect())
                .unwrap_or_else(|| vec![99]);
            seats.push((
                rank,
                Seat {
                    name: scrub(name),
                    kind: str_of(s, "kind"),
                    state: str_of(s, "state"),
                    shown: str_of(s, "shown"),
                    waiting: s.get("waiting").and_then(Value::as_u64).unwrap_or(0),
                    since_s: s.get("since_s").and_then(Value::as_f64).unwrap_or(0.0),
                    can_wake: s.get("can_wake").and_then(Value::as_bool).unwrap_or(false),
                    spent_until: s.get("spent_until").and_then(Value::as_f64),
                    hidden: s.get("hidden").and_then(Value::as_bool).unwrap_or(false),
                    doing: String::new(),
                },
            ));
        }
    }
    // The service's own ready order: same rank, same order as `comms-deck status`.
    seats.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.name.cmp(&b.1.name)));
    seats.into_iter().map(|(_, s)| s).filter(|s| !s.hidden).collect()
}

fn str_of(v: &Value, key: &str) -> String {
    scrub(v.get(key).and_then(Value::as_str).unwrap_or(""))
}

fn parse_note(v: &Value) -> Option<Note> {
    if v.get("kind").and_then(Value::as_str) != Some("message") {
        return None;
    }
    if v.get("private").and_then(Value::as_bool) == Some(true) {
        return None; // private notes stay out of the screen, as in the chat
    }
    Some(Note {
        from: str_of(v, "from"),
        to: str_of(v, "to"),
        time: str_of(v, "time"),
        text: str_of(v, "text"),
        quiet: v.get("quiet").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn parse_activity(v: &Value) -> Option<Activity> {
    Some(Activity {
        time: v.get("time")?.as_f64()?,
        seat: str_of(v, "seat"),
        state: str_of(v, "state"),
        doing: str_of(v, "doing"),
    })
}

/// The last `max` bytes of a jsonl file, as parsed lines (a torn first line
/// is dropped).
pub fn tail_jsonl(path: &Path, max: u64) -> Vec<Value> {
    let Ok(mut f) = File::open(path) else { return Vec::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max);
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
    lines.filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn load_usage(dir: &Path) -> BTreeMap<String, Usage> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(seat) = p.file_stem().and_then(|x| x.to_str()).map(String::from) else { continue };
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        out.insert(seat, parse_usage(v.get("data").unwrap_or(&Value::Null)));
    }
    out
}

pub fn parse_usage(d: &Value) -> Usage {
    let pct = |v: Option<&Value>| v.and_then(|x| x.get("used_percentage")).and_then(Value::as_f64);
    let limits = d.get("rate_limits");
    Usage {
        model: scrub(d.pointer("/model/display_name").and_then(Value::as_str).unwrap_or("")),
        context_pct: pct(d.get("context_window")),
        five_hour_pct: pct(limits.and_then(|l| l.get("five_hour"))),
        week_pct: pct(limits.and_then(|l| l.get("seven_day"))),
        week_resets: limits
            .and_then(|l| l.pointer("/seven_day/resets_at"))
            .and_then(Value::as_f64),
        context_tokens: context_tokens(d),
    }
}

/// context_tokens when the seat reports it, else used% x window size; None
/// when neither is there (left out of the split, never counted as zero).
fn context_tokens(d: &Value) -> Option<f64> {
    let cw = d.get("context_window")?;
    if let Some(t) = cw.get("context_tokens").and_then(Value::as_f64).filter(|t| *t > 0.0) {
        return Some(t);
    }
    let pct = cw.get("used_percentage").and_then(Value::as_f64).filter(|p| *p > 0.0)?;
    let size = cw.get("context_window_size").and_then(Value::as_f64).filter(|s| *s > 0.0)?;
    Some(size * pct / 100.0)
}

impl Snapshot {
    /// [(seat, tokens)] for each seat with a live context figure.
    pub fn context_split(&self) -> Vec<(String, f64)> {
        self.usage
            .iter()
            .filter_map(|(seat, u)| u.context_tokens.map(|t| (seat.clone(), t)))
            .collect()
    }
}

pub fn read_meminfo(path: &Path) -> Memory {
    let mut m = Memory::default();
    let Ok(text) = std::fs::read_to_string(path) else { return m };
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(key), Some(val)) = (parts.next(), parts.next()) else { continue };
        let val: u64 = val.parse().unwrap_or(0);
        match key {
            "MemTotal:" => m.total_kb = val,
            "MemAvailable:" => m.available_kb = val,
            "SwapTotal:" => m.swap_total_kb = val,
            "SwapFree:" => m.swap_free_kb = val,
            _ => {}
        }
    }
    m
}

fn hypr_workspaces() -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let Ok(o) = Command::new("hyprctl").args(["clients", "-j"]).output() else { return out };
    let Ok(v) = serde_json::from_slice::<Value>(&o.stdout) else { return out };
    for c in v.as_array().into_iter().flatten() {
        let ws = c.pointer("/workspace/name").and_then(Value::as_str).unwrap_or("?");
        let title = c.get("title").and_then(Value::as_str).unwrap_or("");
        out.entry(scrub(ws)).or_default().push(scrub(title));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_keeps_the_service_order_and_hides_hidden_seats() {
        let v: Value = serde_json::from_str(
            r#"{"seats": {
                "grok": {"kind":"agent","state":"idle","shown":"ready","rank":[0,0],"waiting":0},
                "claude": {"kind":"agent","state":"busy","shown":"busy","rank":[1,0],"waiting":2},
                "lumo": {"kind":"app","state":"not set up","shown":"not set up","rank":[9,0],"hidden":true}
            }}"#,
        )
        .unwrap();
        let s = parse_status(&v);
        assert_eq!(s.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["grok", "claude"]);
        assert_eq!(s[1].waiting, 2);
    }

    #[test]
    fn usage_reads_the_statusline_shape() {
        let v: Value = serde_json::from_str(
            r#"{"model":{"display_name":"Opus 5.5"},"context_window":{"used_percentage":13},
               "rate_limits":{"five_hour":{"used_percentage":11},"seven_day":{"used_percentage":86,"resets_at":1795744800}}}"#,
        )
        .unwrap();
        let u = parse_usage(&v);
        assert_eq!(u.model, "Opus 5.5");
        assert_eq!(u.context_pct, Some(13.0));
        assert_eq!(u.week_pct, Some(86.0));
        assert_eq!(u.week_resets, Some(1795744800.0));
        assert_eq!(u.context_tokens, None, "no window size: not in the split");
        let v: Value = serde_json::from_str(r#"{"context_window":{"used_percentage":10,"context_window_size":200000}}"#).unwrap();
        assert_eq!(parse_usage(&v).context_tokens, Some(20000.0));
    }

    #[test]
    fn private_notes_never_reach_the_screen() {
        let v: Value = serde_json::from_str(r#"{"kind":"message","from":"a","to":"b","text":"x","private":true}"#).unwrap();
        assert!(parse_note(&v).is_none());
    }
}
