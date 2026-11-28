//! What only the nav view needs, ported from desktop/nav-deck: today's
//! note hours and busy time, the memory history (nav keeps its own, one
//! small file per day in navigation/), load, stream processes, the Twitch
//! chat tail and NAV-TIME's freshness checks.
//!
//! Read-only, except record_memory(), which appends to nav's own history
//! file exactly as nav-deck does (several decks writing one file is fine:
//! readers collapse the 5 s buckets).

use chrono::{Local, NaiveDateTime, TimeZone, Timelike};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use crate::data::{data_dir, now, tail_jsonl};
use crate::scrub::scrub;

pub const SPARK_POINTS: usize = 100; // 5 s apart: the last ~8 minutes
pub const MEM_FLOOR: u64 = 512 * 1024 * 1024; // below this the vitals row shouts

#[derive(Clone, Debug, Default)]
pub struct ChatLine {
    pub time: f64,
    pub user: String,
    pub text: String,
    pub broadcaster: bool,
    pub moderator: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Surface {
    pub name: &'static str,
    pub age: Option<f64>,
    pub detail: String,
}

#[derive(Clone, Debug, Default)]
pub struct NavData {
    /// notes per hour today (0..24)
    pub hours: [u32; 24],
    /// seconds busy today, per seat
    pub busy: BTreeMap<String, f64>,
    /// (time, bytes available), oldest first, the last SPARK_POINTS
    pub memory: Vec<(f64, u64)>,
    /// samples under MEM_FLOOR in the last two days
    pub low_samples: usize,
    pub load: f64,
    pub streams: Vec<String>,
    pub chat: Vec<ChatLine>,
    pub chat_state: String,
    pub surfaces: Vec<Surface>,
    /// note -> inbox lags, seconds, newest 12 inbox files
    pub latency: Vec<f64>,
}

/// A deck timestamp as epoch: "YYYY-MM-DD HH:MM:SS" (local) or a number.
pub fn event_time(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::String(s)) => NaiveDateTime::parse_from_str(s.get(..19).unwrap_or(s), "%Y-%m-%d %H:%M:%S")
            .ok()
            .and_then(|t| Local.from_local_datetime(&t).single())
            .map(|t| t.timestamp() as f64)
            .unwrap_or(0.0),
        Some(v) => v.as_f64().filter(|t| t.is_finite()).unwrap_or(0.0),
        None => 0.0,
    }
}

/// "HH:MM" for an epoch, or None for an impossible time (nan, inf, huge).
pub fn clock(t: f64) -> Option<String> {
    if !t.is_finite() || t <= 0.0 || t > 1.0e11 {
        return None;
    }
    Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%H:%M").to_string())
}

fn day_start() -> f64 {
    let n = Local::now();
    n.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|d| Local.from_local_datetime(&d).single())
        .map(|d| d.timestamp() as f64)
        .unwrap_or(0.0)
}

pub fn load(waiting: u64, oldest_waiting: f64, status_time: f64) -> NavData {
    let dir = data_dir();
    let t0 = day_start();
    let log = tail_jsonl(&dir.join("log.jsonl"), 2 << 20);
    let act = tail_jsonl(&dir.join("activity.jsonl"), 1 << 20);
    let (memory, low_samples) = memory_history(&dir.join("navigation"), now());
    let (chat, chat_state) = twitch_chat(&dir);
    let (surfaces, latency) = timing(&dir, &log, &act, status_time, waiting, oldest_waiting);
    NavData {
        hours: hours_today(&log, t0),
        busy: busy_today(&act, t0, now()),
        memory,
        low_samples,
        load: loadavg(),
        streams: streams(),
        chat,
        chat_state,
        surfaces,
        latency,
    }
}

pub fn hours_today(log: &[Value], t0: f64) -> [u32; 24] {
    let mut h = [0u32; 24];
    for r in log {
        let t = event_time(r.get("time"));
        if t < t0 || t <= 0.0 {
            continue;
        }
        if let Some(d) = Local.timestamp_opt(t as i64, 0).single() {
            h[d.hour() as usize] += 1;
        }
    }
    h
}

pub fn busy_today(act: &[Value], t0: f64, now: f64) -> BTreeMap<String, f64> {
    let mut busy: BTreeMap<String, f64> = BTreeMap::new();
    let mut since: BTreeMap<String, f64> = BTreeMap::new();
    for r in act {
        let t = r.get("time").and_then(Value::as_f64).unwrap_or(0.0);
        let seat = r.get("seat").and_then(Value::as_str).unwrap_or("");
        if t < t0 || seat.is_empty() {
            continue;
        }
        if let Some(s) = since.remove(seat) {
            *busy.entry(seat.to_string()).or_default() += t.min(now) - s;
        }
        if r.get("state").and_then(Value::as_str) == Some("busy") {
            since.insert(seat.to_string(), t);
        }
    }
    for (seat, s) in since {
        *busy.entry(seat).or_default() += (now - s).max(0.0);
    }
    busy
}

pub fn memory_history(dir: &Path, now: f64) -> (Vec<(f64, u64)>, usize) {
    let mut recent = Vec::new();
    let mut low = 0;
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    files.retain(|p: &std::path::PathBuf| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("memory-") && n.ends_with(".jsonl"))
    });
    files.sort();
    for f in files {
        for r in tail_jsonl(&f, 4 << 20) {
            let t = r.get("t").and_then(Value::as_f64).unwrap_or(0.0);
            let v = r.get("avail").and_then(Value::as_u64).unwrap_or(0);
            if v == 0 {
                continue;
            }
            if v < MEM_FLOOR {
                low += 1;
            }
            if t >= now - (SPARK_POINTS as f64) * 5.0 {
                recent.push((t, v));
            }
        }
    }
    recent.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let keep = recent.len().saturating_sub(SPARK_POINTS);
    (recent.split_off(keep), low)
}

/// Append one memory sample to today's history (nav-deck's record_memory).
/// The caller keeps it to one per 5 s.
pub fn record_memory(available_bytes: u64, at: f64) {
    if available_bytes == 0 {
        return;
    }
    let dir = data_dir().join("navigation");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let name = dir.join(format!("memory-{}.jsonl", Local::now().format("%Y-%m-%d")));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(name) {
        let _ = writeln!(f, "{}", serde_json::json!({"t": at, "avail": available_bytes}));
    }
    // keep two days, like nav-deck
    let cutoff = (Local::now() - chrono::Duration::days(1)).format("%Y-%m-%d").to_string();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if let Some(day) = n.strip_prefix("memory-").and_then(|d| d.strip_suffix(".jsonl")) {
                if day < cutoff.as_str() {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn loadavg() -> f64 {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|x| x.parse().ok()))
        .unwrap_or(0.0)
}

/// obs / ffmpeg running on this box (VITALS' stream row).
fn streams() -> Vec<String> {
    let mut found = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else { return found };
    for e in rd.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(cmd) = std::fs::read(e.path().join("cmdline")) else { continue };
        let first = cmd.split(|&b| b == 0).next().unwrap_or(&[]);
        let base = String::from_utf8_lossy(first).rsplit('/').next().unwrap_or("").to_string();
        if base == "obs" || base == "ffmpeg" {
            found.push(base);
            if found.len() >= 4 {
                break;
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// 10 COCKPIT-CHAT: the newest lines devin's twitch-chat daemon wrote, and
/// the reader's own word about itself (live / connecting / auth-failed / config).
pub fn twitch_chat(dir: &Path) -> (Vec<ChatLine>, String) {
    let lines = tail_jsonl(&dir.join("twitch-chat.jsonl"), 32 << 10)
        .iter()
        .filter(|r| r.is_object())
        .map(|r| {
            let badges = scrub(r.get("badges").and_then(Value::as_str).unwrap_or(""));
            ChatLine {
                time: r.get("time").and_then(Value::as_f64).unwrap_or(0.0),
                user: scrub(r.get("user").and_then(Value::as_str).unwrap_or("?")).chars().take(32).collect(),
                text: scrub(r.get("text").and_then(Value::as_str).unwrap_or("")),
                broadcaster: badges.contains("broadcaster"),
                moderator: badges.contains("moderator"),
            }
        })
        .collect();
    let state = std::fs::read_to_string(dir.join("twitch-chat.status.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("state").and_then(Value::as_str).map(scrub))
        .unwrap_or_default();
    (lines, state)
}

fn mtime(p: &Path) -> f64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// NAV-TIME: when each surface last spoke, and note -> inbox latency.
fn timing(dir: &Path, log: &[Value], act: &[Value], status_time: f64, _waiting: u64, _oldest: f64) -> (Vec<Surface>, Vec<f64>) {
    let n = now();
    let age = |ts: f64| if ts > 0.0 { Some(n - ts) } else { None };
    let mut out = vec![Surface { name: "status", age: age(status_time), detail: "cli round-trip".into() }];
    for (name, path, recs) in [("log.jsonl", dir.join("log.jsonl"), log), ("activity.jsonl", dir.join("activity.jsonl"), act)] {
        let ts = recs.last().map(|r| event_time(r.get("time"))).unwrap_or(0.0);
        let detail = if ts > 0.0 { format!("write lag {}", human_dur((mtime(&path) - ts).max(0.0))) } else { String::new() };
        out.push(Surface { name, age: age(ts), detail });
    }
    let mut newest_usage: f64 = 0.0;
    if let Ok(rd) = std::fs::read_dir(dir.join("usage")) {
        for e in rd.flatten() {
            if let Ok(s) = std::fs::read_to_string(e.path()) {
                if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    newest_usage = newest_usage.max(v.get("time").and_then(Value::as_f64).unwrap_or(0.0));
                }
            }
        }
    }
    out.push(Surface { name: "usage/*.json", age: age(newest_usage), detail: "newest statusline".into() });
    let mut stamped: Vec<(f64, std::path::PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("inbox")) {
        for seat in rd.flatten() {
            if let Ok(files) = std::fs::read_dir(seat.path()) {
                for f in files.flatten() {
                    let p = f.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("json") {
                        stamped.push((mtime(&p), p));
                    }
                }
            }
        }
    }
    stamped.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out.push(Surface {
        name: "inbox",
        age: stamped.first().and_then(|x| age(x.0)),
        detail: "newest note landed".into(),
    });
    let mut latency = Vec::new();
    for (mt, p) in stamped.iter().take(12) {
        if let Some(v) = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
            let ts = event_time(v.get("time"));
            if ts > 0.0 {
                latency.push(mt - ts);
            }
        }
    }
    (out, latency)
}

/// "42s", "17m", "3h05m", "2d4h": nav-deck's human_dur.
pub fn human_dur(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86400 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d{}h", s / 86400, (s % 86400) / 3600)
    }
}

/// Warp factor from the deck's own traffic, 1.0..9.9 (nav-deck's warp_factor).
pub fn warp_factor(busy: usize, waiting: u64, hours: &[u32; 24], load: f64) -> f64 {
    let h = Local::now().hour() as usize;
    let recent = hours[h] as f64 + 0.7 * hours[(h + 23) % 24] as f64;
    (1.0 + 0.06 * recent + 0.4 * busy as f64 + 0.012 * waiting as f64 + load.min(4.0)).clamp(1.0, 9.9)
}

/// The workspace a deck label names (nav-deck's DESKS / goto_target).
pub fn goto_target(label: &str) -> String {
    match label {
        "comms" => "1".into(),
        "nav" => "2".into(),
        "mail" => "3".into(),
        d if d.len() == 1 && d.chars().all(|c| c.is_ascii_digit()) => {
            // Super+3..0 reach workspaces 4..11
            let k: u32 = d.parse().unwrap_or(0);
            let k = if k == 0 { 10 } else { k };
            if k >= 3 { (k + 1).to_string() } else { format!("\"name:{d}\"") }
        }
        other => format!("\"name:{}\"", other.replace(['\\', '"'], "")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn busy_time_adds_up_and_counts_the_open_stretch() {
        let act = vec![
            json!({"time": 100.0, "seat": "claude", "state": "busy"}),
            json!({"time": 160.0, "seat": "claude", "state": "idle"}),
            json!({"time": 200.0, "seat": "grok", "state": "busy"}),
        ];
        let b = busy_today(&act, 0.0, 260.0);
        assert_eq!(b["claude"], 60.0);
        assert_eq!(b["grok"], 60.0);
    }

    #[test]
    fn impossible_times_never_draw() {
        assert!(clock(f64::NAN).is_none());
        assert!(clock(f64::INFINITY).is_none());
        assert!(clock(1e30).is_none());
        assert!(clock(1795713000.0).is_some());
        assert_eq!(event_time(Some(&json!(f64::MAX))), f64::MAX);
        assert_eq!(event_time(Some(&json!("not a time"))), 0.0);
    }

    #[test]
    fn goto_matches_the_numbered_decks() {
        assert_eq!(goto_target("comms"), "1");
        assert_eq!(goto_target("nav"), "2");
        assert_eq!(goto_target("3"), "4");
        assert_eq!(goto_target("0"), "11");
        assert_eq!(goto_target("odd\"name"), "\"name:oddname\"");
    }

    #[test]
    fn durations_read_like_nav_deck() {
        assert_eq!(human_dur(42.0), "42s");
        assert_eq!(human_dur(185.0 * 60.0), "3h05m");
        assert_eq!(human_dur(2.0 * 86400.0 + 4.0 * 3600.0), "2d4h");
    }
}
