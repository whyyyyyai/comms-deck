//! 01 CREW: comms-deck's look_crew() as a widget. Each seat is a cluster of
//! braille dots in its colour, sized by its share of the context pool and
//! breathing while it works; the notes of the last 30 minutes are links (in
//! the sender's colour for 5 minutes), and a dot runs along each link sent
//! in the last 10. Y is the middle.
//!
//! The canvas is the area at braille resolution: two dots across and four
//! down per cell, so it fills whatever Rect it is given.

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;
use serde_json::Value;

use super::theme::{seat_colour, OWNER_NAME};
use crate::data::{Note, Snapshot};
use crate::navdata::event_time;

/// The braille bit for the dot at (row % 4, col % 2) of a cell.
const LOOK_DOT: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
/// Seconds per shimmer step (comms-deck's PULSE_EVERY): a breath is 8 steps.
const PULSE_EVERY: f64 = 0.5;
/// Links fade to grey after this long.
const LINK_FRESH: f64 = 300.0;
/// A dot runs along links younger than this.
const LINK_MOVING: f64 = 600.0;
/// Links older than this are not drawn.
const LINK_SHOWN: f64 = 1800.0;
const LINK_GREY: u8 = 240;

pub struct Crew<'a> {
    pub snap: &'a Snapshot,
    pub now: f64,
}

impl Crew<'_> {
    /// True when some link carries a running dot: the screen should keep
    /// redrawing fast (comms-deck's look_crew.moving).
    pub fn moving(&self) -> bool {
        recent(self.snap, self.now).iter().any(|(_, t)| self.now - t <= LINK_MOVING)
    }
}

/// The notes of the last 30 minutes (out of the newest 300), with their times.
fn recent(snap: &Snapshot, now: f64) -> Vec<(&Note, f64)> {
    let start = snap.notes.len().saturating_sub(300);
    snap.notes[start..]
        .iter()
        .map(|n| (n, note_time(n)))
        .filter(|(_, t)| now - t < LINK_SHOWN)
        .collect()
}

fn note_time(n: &Note) -> f64 {
    event_time(Some(&Value::String(n.time.clone())))
}

/// FNV-1a then a splitmix64 finish: a stable hash (no sha256 in the crate).
fn hash64(s: &str, salt: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    splitmix(h)
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A 256-bit seed for a seat (four little-endian limbs), standing in for
/// the int of its sha256 hex digest.
fn seed256(s: &str) -> [u64; 4] {
    [hash64(s, 0), hash64(s, 1), hash64(s, 2), hash64(s, 3)]
}

/// The low 64 bits of `seed >> shift`, for shift < 256.
fn shifted(seed: &[u64; 4], shift: u32) -> u64 {
    let (i, b) = ((shift / 64) as usize, shift % 64);
    let lo = seed[i] >> b;
    let hi = if b > 0 && i + 1 < 4 { seed[i + 1] << (64 - b) } else { 0 };
    lo | hi
}

fn ink(colour: Color, modifier: Option<Modifier>) -> Style {
    let s = Style::default().fg(colour);
    match modifier {
        Some(m) => s.add_modifier(m),
        None => s,
    }
}

/// comms-deck's breathe_style: dim, normal or bold on a cosine over the
/// 0-8 cycle, peak at 4. The hue never changes.
fn breathe_style(colour: Color, phase: f64) -> Style {
    let b = 0.5 + 0.5 * (std::f64::consts::TAU * (phase - 4.0) / 8.0).cos();
    if b <= 0.15 {
        ink(colour, Some(Modifier::DIM))
    } else if b >= 0.85 {
        ink(colour, Some(Modifier::BOLD))
    } else {
        ink(colour, None)
    }
}

/// comms-deck's seat_title(seat, online).lower(): Gravity is Antigravity
/// while it is online.
fn seat_title(seat: &str, online: bool) -> String {
    if seat == "gravity" && online {
        "antigravity".into()
    } else {
        seat.to_lowercase()
    }
}

/// The braille canvas: dots and their styles per cell, plus text over them.
struct Canvas {
    cols: usize,
    rows: usize,
    bits: Vec<u8>,
    ink: Vec<Option<Style>>,
    text: Vec<Option<(char, Style)>>,
}

impl Canvas {
    fn new(cols: usize, rows: usize) -> Self {
        Canvas { cols, rows, bits: vec![0; cols * rows], ink: vec![None; cols * rows], text: vec![None; cols * rows] }
    }

    fn w(&self) -> f64 {
        (self.cols * 2) as f64
    }

    fn h(&self) -> f64 {
        (self.rows * 4) as f64
    }

    fn plot(&mut self, px: f64, py: f64, style: Style) {
        if !px.is_finite() || !py.is_finite() {
            return;
        }
        // `as` truncates toward zero, like Python's int().
        let (px, py) = (px as i64, py as i64);
        if px < 0 || py < 0 || px >= (self.cols * 2) as i64 || py >= (self.rows * 4) as i64 {
            return;
        }
        let (px, py) = (px as usize, py as usize);
        let i = (py / 4) * self.cols + px / 2;
        self.bits[i] |= LOOK_DOT[py % 4][px % 2];
        self.ink[i] = Some(style);
    }

    /// `s` centred on dot (px, py), kept inside the width where it fits.
    fn label(&mut self, px: f64, py: f64, s: &str, style: Style) {
        if !px.is_finite() || !py.is_finite() {
            return;
        }
        let r = (py as i64).div_euclid(4);
        if r < 0 || r >= self.rows as i64 {
            return;
        }
        let len = s.chars().count() as i64;
        let c = ((px as i64).div_euclid(2) - len / 2).min(self.cols as i64 - len).max(0);
        for (i, ch) in s.chars().enumerate() {
            let col = c + i as i64;
            if col >= 0 && col < self.cols as i64 {
                self.text[r as usize * self.cols + col as usize] = Some((ch, style));
            }
        }
    }

    fn line(&mut self, a: (f64, f64), b: (f64, f64), style: Style) {
        let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / 3.0) as usize + 1;
        for k in 0..=n {
            let f = k as f64 / n as f64;
            self.plot(a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f, style);
        }
    }
}

impl Widget for Crew<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.width < 10 || area.height < 4 {
            return;
        }
        let mut cv = Canvas::new(area.width as usize, area.height as usize);
        let (w, h) = (cv.w(), cv.h());
        let now = self.now;
        let snap = self.snap;

        // Seats around the middle, the first at the top, then clockwise.
        let seats: Vec<&str> = snap.seats.iter().filter(|s| !s.hidden).map(|s| s.name.as_str()).collect();
        let mut pos: HashMap<&str, (f64, f64)> = HashMap::new();
        for (i, s) in seats.iter().enumerate() {
            let ang = -std::f64::consts::FRAC_PI_2 + std::f64::consts::TAU * i as f64 / seats.len().max(1) as f64;
            pos.insert(*s, (w / 2.0 + ang.cos() * w * 0.38, h / 2.0 + ang.sin() * h * 0.34));
        }
        pos.insert("owner", (w / 2.0, h / 2.0));

        let tokens: HashMap<String, f64> = snap.context_split().into_iter().collect();
        let biggest = tokens.values().cloned().fold(f64::NAN, f64::max);
        let biggest = if biggest.is_finite() && biggest != 0.0 { biggest } else { 1.0 };
        let phase = now / PULSE_EVERY;
        let notes = recent(snap, now);

        let ends = |n: &Note| -> Vec<((f64, f64), (f64, f64))> {
            let targets: Vec<&str> = if n.to == "all" { seats.clone() } else { n.to.split('+').collect() };
            let Some(&a) = pos.get(n.from.as_str()) else { return Vec::new() };
            targets
                .into_iter()
                .filter_map(|t| pos.get(t).copied())
                .filter(|&b| b != a)
                .map(|b| (a, b))
                .collect()
        };

        // Links: the sender's colour while fresh, then grey.
        for (n, t) in &notes {
            let c = if now - t < LINK_FRESH { seat_colour(&n.from) } else { Color::Indexed(LINK_GREY) };
            for (a, b) in ends(*n) {
                cv.line(a, b, ink(c, None));
            }
        }
        // A 2x2 dot running along each link of the last 10 minutes, once
        // every 3 seconds, each note at its own offset.
        for (n, t) in &notes {
            if now - t > LINK_MOVING {
                continue;
            }
            let id = format!("{}\u{1f}{}\u{1f}{}\u{1f}{}", n.time, n.from, n.to, n.text);
            let off = (hash64(&id, 0) & 0xffff) as f64 / 65536.0 * 3.0;
            let f = (now + off).rem_euclid(3.0) / 3.0;
            let style = ink(seat_colour(&n.from), Some(Modifier::BOLD));
            for (a, b) in ends(*n) {
                for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    cv.plot(a.0 + (b.0 - a.0) * f + dx, a.1 + (b.1 - a.1) * f + dy, style);
                }
            }
        }
        // Clusters, then labels under them.
        for seat in snap.seats.iter().filter(|s| !s.hidden) {
            let s = seat.name.as_str();
            let (cx, cy) = pos[s];
            let colour = seat_colour(s);
            let n = match tokens.get(s) {
                Some(&t) if t != 0.0 => (14.0 + 140.0 * (t / biggest).max(0.0).sqrt()) as usize,
                _ => 8,
            };
            let seed = seed256(s);
            let on = seat.shown == "busy";
            let spread = 3.0 + w.min(h) * 0.11 * (n as f64 / 154.0).sqrt();
            for k in 0..n {
                let hh = shifted(&seed, (k % 200) as u32) ^ (k as u64).wrapping_mul(2_654_435_761);
                let ang = (hh % 6283) as f64 / 1000.0 + if on { now * 0.15 } else { 0.0 };
                let rad = (((hh >> 13) % 1000) as f64 / 1000.0).powf(0.6) * spread;
                let style = if on { breathe_style(colour, k as f64 + phase) } else { ink(colour, None) };
                cv.plot(cx + ang.cos() * rad, cy + ang.sin() * rad, style);
            }
            let dim = matches!(seat.state.as_str(), "away" | "not set up" | "no chat yet");
            let text = format!("[{}{}]", seat_title(s, !dim), if on { " ●" } else { "" });
            let style = if dim {
                ink(Color::Indexed(244), Some(Modifier::DIM))
            } else {
                ink(colour, Some(Modifier::BOLD))
            };
            cv.label(cx, (h - 1.0).min(cy + spread + 5.0), &text, style);
        }
        cv.label(w / 2.0, h / 2.0, &format!("( {OWNER_NAME} )"), ink(Color::Indexed(255), Some(Modifier::BOLD)));

        for r in 0..cv.rows {
            for c in 0..cv.cols {
                let i = r * cv.cols + c;
                let (ch, style) = if let Some((ch, st)) = cv.text[i] {
                    (ch, st)
                } else if cv.bits[i] != 0 {
                    let ch = char::from_u32(0x2800 + cv.bits[i] as u32).unwrap_or(' ');
                    (ch, cv.ink[i].unwrap_or_default())
                } else {
                    continue;
                };
                // r < area.height and c < area.width, and area lies in buf.
                buf[(area.x + c as u16, area.y + r as u16)].set_char(ch).set_style(style);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Seat, Usage};
    use chrono::TimeZone;

    const NOW: f64 = 1_795_000_000.0;

    fn stamp(t: f64) -> String {
        chrono::Local.timestamp_opt(t as i64, 0).single().unwrap().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    fn seat(name: &str, shown: &str) -> Seat {
        Seat { name: name.into(), kind: "agent".into(), state: shown.into(), shown: shown.into(), ..Default::default() }
    }

    fn snap(busy: bool, notes: bool) -> Snapshot {
        let mut s = Snapshot { time: NOW, ..Default::default() };
        s.seats = vec![seat("claude", if busy { "busy" } else { "idle" }), seat("grok", "idle"), seat("codex", "away")];
        s.usage.insert("claude".into(), Usage { context_tokens: Some(120_000.0), ..Default::default() });
        s.usage.insert("grok".into(), Usage { context_tokens: Some(30_000.0), ..Default::default() });
        if notes {
            s.notes = vec![
                Note { from: "claude".into(), to: "grok".into(), time: stamp(NOW - 60.0), text: "hi".into(), quiet: false },
                Note { from: "owner".into(), to: "all".into(), time: stamp(NOW - 900.0), text: "go".into(), quiet: false },
            ];
        }
        s
    }

    fn render(s: &Snapshot, now: f64, area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        Crew { snap: s, now }.render(area, &mut buf);
        buf
    }

    fn text(buf: &Buffer) -> String {
        let a = buf.area;
        let mut out = String::new();
        for y in a.top()..a.bottom() {
            for x in a.left()..a.right() {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_a_60x20_area() {
        let buf = render(&snap(true, true), NOW, Rect::new(0, 0, 60, 20));
        let t = text(&buf);
        assert!(t.chars().any(|c| ('\u{2801}'..='\u{28ff}').contains(&c)), "some braille dots:\n{t}");
        assert!(t.contains("( Y )"), "Y in the middle:\n{t}");
    }

    #[test]
    fn seat_labels_appear() {
        let t = text(&render(&snap(true, false), NOW, Rect::new(0, 0, 60, 20)));
        assert!(t.contains("[claude ●]"), "busy label:\n{t}");
        assert!(t.contains("[grok]"), "idle label:\n{t}");
    }

    #[test]
    fn tiny_area_draws_nothing() {
        for area in [Rect::new(0, 0, 9, 20), Rect::new(0, 0, 60, 3), Rect::new(0, 0, 0, 0)] {
            let buf = render(&snap(true, true), NOW, area);
            assert_eq!(buf, Buffer::empty(area), "{area:?}");
        }
    }

    #[test]
    fn stays_inside_an_offset_area_of_any_size() {
        let outer = Rect::new(0, 0, 120, 50);
        for (w, h) in [(10, 4), (11, 5), (37, 9), (100, 40)] {
            let area = Rect::new(7, 3, w, h);
            let mut buf = Buffer::empty(outer);
            Crew { snap: &snap(true, true), now: NOW }.render(area, &mut buf);
            for y in 0..outer.height {
                for x in 0..outer.width {
                    if !(area.x..area.right()).contains(&x) || !(area.y..area.bottom()).contains(&y) {
                        assert_eq!(buf[(x, y)].symbol(), " ", "drew outside at {x},{y} for {w}x{h}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_busy_seat_breathes_and_an_idle_crew_is_still() {
        let area = Rect::new(0, 0, 60, 20);
        let busy = snap(true, false);
        assert_ne!(render(&busy, NOW, area), render(&busy, NOW + 2.0, area));
        let idle = snap(false, false);
        assert_eq!(render(&idle, NOW, area), render(&idle, NOW + 2.0, area));
    }

    #[test]
    fn same_input_same_picture() {
        let area = Rect::new(0, 0, 60, 20);
        let s = snap(true, true);
        assert_eq!(render(&s, NOW, area), render(&s, NOW, area));
    }

    #[test]
    fn recent_links_carry_a_moving_dot() {
        assert!(Crew { snap: &snap(false, true), now: NOW }.moving());
        assert!(!Crew { snap: &snap(false, false), now: NOW }.moving());
    }
}
