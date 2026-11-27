//! 04 WARP CORE: a first-person starfield. Each star belongs to a seat and
//! wears its colour; stars are dealt out in proportion to the context split
//! (the owner, 2026-11-26: "the stars in the warp core ... colored and distributed
//! according to the context split"). Speed is the deck's traffic.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use super::theme::seat_colour;

#[derive(Clone, Debug)]
pub struct Star {
    x: f64,
    y: f64,
    z: f64,
    pub seat: Option<String>,
    /// This seat's metric pace (Snapshot::seat_pace), 0.6..1.8.
    pace: f64,
}

pub struct Starfield {
    pub stars: Vec<Star>,
    rng: u64,
    /// (seat, share 0..1), cumulative order kept stable for dealing.
    split: Vec<(String, f64)>,
    /// (seat, pace) looked up when a star is dealt or respawns.
    pace: Vec<(String, f64)>,
}

const NEAR: f64 = 0.05;

impl Starfield {
    pub fn new(count: usize, seed: u64) -> Self {
        let mut f = Starfield { stars: Vec::with_capacity(count), rng: seed | 1, split: Vec::new(), pace: Vec::new() };
        for _ in 0..count {
            let mut s = Star { x: 0.0, y: 0.0, z: 0.0, seat: None, pace: 1.0 };
            f.respawn(&mut s, true);
            f.stars.push(s);
        }
        f
    }

    fn rand(&mut self) -> f64 {
        // xorshift64*: deterministic, so the self-test sees the same sky
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Set the context split: [(seat, tokens)]. Stars are re-dealt as they
    /// respawn, so a change shows as a drift, not a jump; `reset` deals now.
    pub fn set_split(&mut self, split: &[(String, f64)], reset: bool) {
        let total: f64 = split.iter().map(|(_, t)| t.max(0.0)).sum();
        self.split = if total > 0.0 {
            split.iter().map(|(s, t)| (s.clone(), t.max(0.0) / total)).collect()
        } else {
            Vec::new()
        };
        if reset {
            let n = self.stars.len();
            let seats: Vec<Option<String>> = (0..n).map(|i| self.deal_exact(i, n)).collect();
            for (s, seat) in self.stars.iter_mut().zip(seats) {
                s.seat = seat;
                s.pace = Self::pace_of(&self.pace, s.seat.as_deref());
            }
        }
    }

    /// Set each seat's pace from Snapshot::seat_pace and apply it to the
    /// stars already in the sky, so a meter change shows on the next frame.
    pub fn set_pace(&mut self, pace: &[(String, f64)]) {
        self.pace = pace.iter().map(|(s, p)| (s.clone(), p.clamp(0.5, 2.0))).collect();
        for s in self.stars.iter_mut() {
            s.pace = Self::pace_of(&self.pace, s.seat.as_deref());
        }
    }

    fn pace_of(pace: &[(String, f64)], seat: Option<&str>) -> f64 {
        seat.and_then(|name| pace.iter().find(|(s, _)| s == name).map(|(_, p)| *p))
            .unwrap_or(1.0)
            .clamp(0.5, 2.0)
    }

    /// The seat for star `i` of `n` when dealing all at once.
    fn deal_exact(&self, i: usize, n: usize) -> Option<String> {
        let counts = star_counts(&self.split, n);
        let mut edge = 0;
        for (seat, c) in counts {
            edge += c;
            if i < edge {
                return Some(seat);
            }
        }
        None
    }

    fn deal(&mut self) -> Option<String> {
        let r = self.rand();
        let mut edge = 0.0;
        for (seat, share) in &self.split {
            edge += share;
            if r < edge {
                return Some(seat.clone());
            }
        }
        self.split.last().map(|(s, _)| s.clone())
    }

    fn respawn(&mut self, s: &mut Star, anywhere: bool) {
        s.x = self.rand() * 2.0 - 1.0;
        s.y = self.rand() * 2.0 - 1.0;
        s.z = if anywhere { NEAR + self.rand() * (1.0 - NEAR) } else { 1.0 };
        s.seat = self.deal();
        s.pace = Self::pace_of(&self.pace, s.seat.as_deref());
    }

    /// Move every star toward the viewer. `speed` is depth per second; each
    /// star's own pace (its seat's meters and traffic) scales that speed.
    pub fn step(&mut self, dt: f64, speed: f64) {
        let mut stars = std::mem::take(&mut self.stars);
        for s in stars.iter_mut() {
            s.z -= dt * speed * s.pace;
            if s.z <= NEAR {
                self.respawn(s, false);
            }
        }
        self.stars = stars;
    }

    pub fn count_for(&self, seat: &str) -> usize {
        self.stars.iter().filter(|s| s.seat.as_deref() == Some(seat)).count()
    }
}

/// Stars per seat out of `n`: nav-deck's star_seats() deal (Devin, #141).
/// Largest remainder with a one-star floor, so every reporting seat shows;
/// an overshoot from the floor is taken back from the biggest seat.
pub fn star_counts(split: &[(String, f64)], n: usize) -> Vec<(String, usize)> {
    if split.is_empty() || n == 0 {
        return Vec::new();
    }
    if split.len() >= n {
        let mut out: Vec<(String, usize)> = split.iter().map(|(s, _)| (s.clone(), 0)).collect();
        for i in 0..n {
            out[i % split.len()].1 += 1;
        }
        return out;
    }
    // Shares are normalised here, so a caller can pass raw token counts:
    // 10 CONTEXT SPLIT once did, and every seat was dealt millions of dots
    // that the trim loop below took back one at a time (~0.5 s a frame).
    let total: f64 = split.iter().map(|(_, v)| v.max(0.0)).sum();
    if total <= 0.0 {
        return split.iter().map(|(s, _)| (s.clone(), n / split.len())).collect();
    }
    let raw: Vec<(String, f64)> =
        split.iter().map(|(s, v)| (s.clone(), v.max(0.0) / total * n as f64)).collect();
    let mut counts: Vec<(String, usize)> = raw.iter().map(|(s, r)| (s.clone(), (r.floor() as usize).max(1))).collect();
    let mut spare = n as i64 - counts.iter().map(|(_, c)| *c as i64).sum::<i64>();
    let mut order: Vec<usize> = (0..raw.len()).collect();
    order.sort_by(|&a, &b| {
        let fa = raw[a].1 - raw[a].1.floor();
        let fb = raw[b].1 - raw[b].1.floor();
        fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal).then(raw[a].0.cmp(&raw[b].0))
    });
    for &i in &order {
        if spare <= 0 {
            break;
        }
        counts[i].1 += 1;
        spare -= 1;
    }
    while spare < 0 {
        let big = (0..counts.len()).max_by_key(|&i| counts[i].1).unwrap_or(0);
        if counts[big].1 <= 1 {
            break;
        }
        counts[big].1 -= 1;
        spare += 1;
    }
    counts
}

/// A streak across the core: `count` notes from `from` to `to` in the
/// window, `age` seconds since the newest of them.
#[derive(Clone, Debug)]
pub struct WarpLink {
    pub from: String,
    pub to: String,
    pub age: f64,
    pub count: u32,
}

pub struct WarpCore<'a> {
    pub field: &'a Starfield,
    pub warp: f64,
    /// Memory available as a percentage; the shield readout and its colour.
    pub shield_pct: f64,
    pub contacts: Vec<(String, u64)>,
    /// The seat picked by a click: its stars stand out, the rest dim.
    pub selected: Option<String>,
    /// Zoomed: (seat, label) written round the edge at the seat's bearing.
    pub ring: Vec<(String, String)>,
    /// Recent notes as seat-to-seat streaks.
    pub links: Vec<WarpLink>,
    /// Seats near a usage limit: (seat, fullest pct); they flicker red.
    pub alerts: Vec<(String, f64)>,
    /// Spent seats: their stars drift dim.
    pub spent: Vec<String>,
    /// Wall time for the flicker, in seconds.
    pub now: f64,
}

/// Where a star at (x, y, z) lands in `area`, if it does.
pub fn project(area: Rect, x: f64, y: f64, z: f64) -> Option<(u16, u16)> {
    let (cx, cy) = centre(area);
    // Cells are about twice as tall as wide: halve y to keep it round.
    let px = cx as f64 + x / z * area.width as f64 / 2.0 * 0.5;
    let py = cy as f64 + y / z * area.height as f64 / 2.0 * 0.5;
    if px < area.x as f64 || py < area.y as f64 {
        return None;
    }
    let (px, py) = (px as u16, py as u16);
    if px >= area.right() || py >= area.bottom() {
        return None;
    }
    Some((px, py))
}

/// A contact blip's cell (not yet clipped): its seat's bearing, pushed out
/// by the notes waiting.
fn contact_cell(area: Rect, seat: &str, waiting: u64) -> (f64, f64) {
    let (cx, cy) = centre(area);
    let angle = bearing(seat);
    let reach = 0.45 + 0.25 * (waiting.min(30) as f64 / 30.0);
    (cx as f64 + angle.cos() * area.width as f64 / 2.0 * reach, cy as f64 + angle.sin() * area.height as f64 / 2.0 * reach)
}

fn contact_label(seat: &str, waiting: u64) -> String {
    format!("{}{}", seat.chars().next().unwrap_or('?').to_ascii_uppercase(), waiting)
}

/// The seat under a click at (col, row) in the core's `area`: a contact
/// blip first (it's text, so the whole label counts), then the nearest
/// seat star within two cells (rows count double, as they're tall).
pub fn hit(field: &Starfield, contacts: &[(String, u64)], area: Rect, col: u16, row: u16) -> Option<String> {
    for (seat, waiting) in contacts {
        let (px, py) = contact_cell(area, seat, *waiting);
        let len = contact_label(seat, *waiting).len() as f64;
        if py as u16 == row && (col as f64) >= px.floor() && (col as f64) < px.floor() + len {
            return Some(seat.clone());
        }
    }
    let mut best: Option<(f64, &str)> = None;
    for s in &field.stars {
        let Some(seat) = s.seat.as_deref() else { continue };
        let Some((px, py)) = project(area, s.x, s.y, s.z) else { continue };
        let d = (px as f64 - col as f64).powi(2) + (2.0 * (py as f64 - row as f64)).powi(2);
        if d <= 4.0 && best.map_or(true, |(b, _)| d < b) {
            best = Some((d, seat));
        }
    }
    best.map(|(_, s)| s.to_string())
}

/// The reticle's cell: the area's exact centre (nav-deck's HUD rule).
pub fn centre(area: Rect) -> (u16, u16) {
    (area.x + area.width / 2, area.y + area.height / 2)
}

/// A contact's bearing: stable per seat, so adding or dropping another
/// contact never swings this one around the reticle.
fn bearing(seat: &str) -> f64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in seat.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h % 6283) as f64 / 1000.0
}

impl Widget for WarpCore<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 3 {
            return;
        }
        let (cx, cy) = centre(area);
        let half_w = area.width as f64 / 2.0;
        let half_h = area.height as f64 / 2.0;
        let project = |x: f64, y: f64, z: f64| project(area, x, y, z);
        let sel = self.selected.as_deref();
        // With a seat picked, its stars are bold and every other star dims.
        let picked = |seat: Option<&str>, style: Style| match sel {
            Some(p) if seat == Some(p) => style.add_modifier(Modifier::BOLD),
            Some(_) => style.add_modifier(Modifier::DIM),
            None => style,
        };
        // Note streaks: a line across the core between the sender's and the
        // receiver's bearings, in the sender's colour, dimming with age.
        for l in &self.links {
            if l.from == l.to {
                continue;
            }
            let (a1, a2) = (bearing(&l.from), bearing(&l.to));
            let r = 0.55;
            let (x1, y1) = (cx as f64 + a1.cos() * half_w * r, cy as f64 + a1.sin() * half_h * r);
            let (x2, y2) = (cx as f64 + a2.cos() * half_w * r, cy as f64 + a2.sin() * half_h * r);
            let steps = ((x2 - x1).abs().max((y2 - y1).abs()) * 2.0).ceil() as usize + 1;
            let mut style = Style::default().fg(seat_colour(&l.from));
            if l.age > 300.0 {
                style = style.add_modifier(Modifier::DIM);
            } else if l.count >= 3 {
                style = style.add_modifier(Modifier::BOLD);
            }
            let glyph = if l.count >= 3 { "•" } else { "·" };
            for k in 0..steps {
                let f = k as f64 / (steps - 1).max(1) as f64;
                let (px, py) = (x1 + (x2 - x1) * f, y1 + (y2 - y1) * f);
                if px >= area.x as f64 && py >= area.y as f64 && px < area.right() as f64 && py < area.bottom() as f64 {
                    buf[(px as u16, py as u16)].set_symbol(glyph).set_style(style);
                }
            }
        }
        // Trails are a warp readout: they only exist while the deck is hot.
        if self.warp > 5.0 {
            let n = (((self.warp - 5.0) * 0.6).round() as usize + 1).clamp(1, 3);
            for s in &self.field.stars {
                let colour = s.seat.as_deref().map(seat_colour).unwrap_or(Color::Indexed(244));
                let trail = Style::default().fg(colour).add_modifier(Modifier::DIM);
                for k in 1..=n {
                    let tz = s.z * (1.0 + 0.30 * k as f64);
                    if let Some((px, py)) = project(s.x, s.y, tz) {
                        buf[(px, py)].set_symbol("·").set_style(trail);
                    }
                }
            }
        }
        for s in &self.field.stars {
            let Some((px, py)) = project(s.x, s.y, s.z) else { continue };
            let glyph = if s.z < 0.25 { "✦" } else if s.z < 0.55 { "•" } else { "·" };
            let colour = s.seat.as_deref().map(seat_colour).unwrap_or(Color::Indexed(244));
            let mut style = Style::default().fg(colour);
            if s.pace >= 1.5 {
                style = style.add_modifier(Modifier::BOLD);
            }
            let glyph = if sel.is_some() && s.seat.as_deref() == sel && glyph == "·" { "•" } else { glyph };
            let mut style = picked(s.seat.as_deref(), style);
            if s.seat.as_deref().is_some_and(|n| self.spent.iter().any(|x| x == n)) {
                style = style.add_modifier(Modifier::DIM);
            }
            buf[(px, py)].set_symbol(glyph).set_style(style);
        }
        // Contacts keep a bearing of their own (stable per seat); more notes
        // waiting pushes the blip out on its reach.
        for (seat, waiting) in &self.contacts {
            let (px, py) = contact_cell(area, seat, *waiting);
            let label = contact_label(seat, *waiting);
            if px < area.x as f64 || py < area.y as f64 {
                continue;
            }
            let (px, py) = (px as u16, py as u16);
            if py >= area.bottom() || (px as usize + label.len()) >= area.right() as usize {
                continue;
            }
            let mut style = Style::default().fg(seat_colour(seat));
            if *waiting >= 10 {
                style = style.add_modifier(Modifier::BOLD);
            }
            if sel == Some(seat.as_str()) {
                style = style.add_modifier(Modifier::REVERSED);
            }
            buf.set_string(px, py, label, style);
        }
        // A seat near a usage limit flickers red on its bearing.
        for (seat, _pct) in &self.alerts {
            let angle = bearing(seat);
            let px = cx as f64 + angle.cos() * half_w * 0.33;
            let py = cy as f64 + angle.sin() * half_h * 0.33;
            if px < area.x as f64 || py < area.y as f64 || px >= area.right() as f64 || py >= area.bottom() as f64 {
                continue;
            }
            let on = (self.now * 2.0 + angle).rem_euclid(1.0) < 0.55;
            let style = Style::default()
                .fg(Color::Indexed(203))
                .add_modifier(if on { Modifier::BOLD } else { Modifier::DIM });
            buf[(px as u16, py as u16)].set_symbol(if on { "!" } else { "·" }).set_style(style);
        }
        // Zoomed: every seat's name round the edge, on its own bearing, so
        // the blips and stars can be read without the side panels.
        for (seat, label) in &self.ring {
            let angle = bearing(seat);
            let w = label.chars().count() as f64;
            let px = (cx as f64 + angle.cos() * (half_w - 1.0) * 0.92 - w / 2.0)
                .clamp(area.x as f64 + 1.0, (area.right() as f64 - w - 1.0).max(area.x as f64));
            let py = (cy as f64 + angle.sin() * (half_h - 1.0) * 0.92).clamp(area.y as f64, area.bottom() as f64 - 2.0);
            let mut style = Style::default().fg(seat_colour(seat));
            if sel == Some(seat.as_str()) {
                style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            buf.set_string(px as u16, py as u16, label, style);
        }
        // Reticle on the exact centre.
        let ret = Style::default().fg(Color::Indexed(252));
        buf[(cx, cy)].set_symbol("╋").set_style(ret);
        if cx >= area.x + 2 && cx + 2 < area.right() {
            buf[(cx - 2, cy)].set_symbol("─").set_style(ret);
            buf[(cx + 2, cy)].set_symbol("─").set_style(ret);
        }
        let hud = format!(" WARP {:.1} ", self.warp);
        buf.set_string(area.x + 1, area.bottom() - 1, hud, Style::default().fg(Color::Indexed(117)));
        let pct = self.shield_pct.clamp(0.0, 100.0);
        let low = pct < 15.0;
        let shield = format!(" SHLD {} {:.0}% ", if low { "LOW" } else { "OK" }, pct);
        let colour = if low { Color::Indexed(203) } else { Color::Indexed(114) };
        let x = area.right().saturating_sub(shield.len() as u16 + 1);
        buf.set_string(x, area.bottom() - 1, shield, Style::default().fg(colour));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_follow_the_context_split() {
        let mut f = Starfield::new(200, 7);
        f.set_split(&[("claude".into(), 300.0), ("grok".into(), 100.0)], true);
        assert_eq!(f.count_for("claude"), 150);
        assert_eq!(f.count_for("grok"), 50);
    }

    #[test]
    fn star_counts_takes_raw_tokens_as_shares() {
        let tokens = vec![("claude".to_string(), 150_000.0), ("vibe".to_string(), 50_000.0)];
        let started = std::time::Instant::now();
        let c = star_counts(&tokens, 312);
        assert!(started.elapsed().as_millis() < 50, "dealt in {:?}", started.elapsed());
        assert_eq!(c.iter().map(|(_, n)| n).sum::<usize>(), 312);
        assert_eq!(c[0].1, 234, "75% of the dots");
    }

    #[test]
    fn every_reporting_seat_gets_a_star() {
        let split: Vec<(String, f64)> = vec![("big".into(), 0.999), ("tiny".into(), 0.001)];
        let c = star_counts(&split, 90);
        assert_eq!(c.iter().map(|x| x.1).sum::<usize>(), 90);
        assert_eq!(c.iter().find(|x| x.0 == "tiny").unwrap().1, 1);
    }

    #[test]
    fn respawned_stars_keep_the_proportions_roughly() {
        let mut f = Starfield::new(2000, 3);
        f.set_split(&[("claude".into(), 1.0), ("devin".into(), 3.0)], false);
        for _ in 0..400 {
            f.step(0.05, 1.0); // enough to cycle every star many times
        }
        let d = f.count_for("devin") as f64 / 2000.0;
        assert!((d - 0.75).abs() < 0.05, "devin share {d}");
    }

    #[test]
    fn no_split_means_grey_stars_not_a_crash() {
        let mut f = Starfield::new(10, 1);
        f.set_split(&[], true);
        assert!(f.stars.iter().all(|s| s.seat.is_none()));
    }

    #[test]
    fn pace_follows_the_seat_metric() {
        let mut f = Starfield::new(100, 5);
        f.set_split(&[("claude".into(), 1.0), ("grok".into(), 1.0)], true);
        f.set_pace(&[("claude".into(), 1.8), ("grok".into(), 0.6)]);
        assert!(f.stars.iter().filter(|s| s.seat.as_deref() == Some("claude")).all(|s| s.pace == 1.8));
        assert!(f.stars.iter().filter(|s| s.seat.as_deref() == Some("grok")).all(|s| s.pace == 0.6));
    }

    #[test]
    fn reticle_is_on_the_exact_centre() {
        let area = Rect::new(0, 0, 41, 21);
        let mut buf = Buffer::empty(area);
        let f = Starfield::new(0, 1);
        core(&f).render(area, &mut buf);
        assert_eq!(buf[(20, 10)].symbol(), "╋");
    }

    fn core<'a>(field: &'a Starfield) -> WarpCore<'a> {
        WarpCore {
            field,
            warp: 1.0,
            shield_pct: 100.0,
            contacts: vec![],
            selected: None,
            ring: vec![],
            links: vec![],
            alerts: vec![],
            spent: vec![],
            now: 0.0,
        }
    }

    fn text(buf: &Buffer) -> String {
        let a = buf.area;
        let mut out = String::new();
        for y in a.top()..a.bottom() {
            for x in a.left()..a.right() {
                out.push_str(buf[(x, y)].symbol());
            }
        }
        out
    }

    #[test]
    fn a_note_streak_draws_between_the_two_seats() {
        let f = Starfield::new(0, 1);
        let area = Rect::new(0, 0, 80, 24);
        let mut plain = Buffer::empty(area);
        core(&f).render(area, &mut plain);
        let mut linked = Buffer::empty(area);
        WarpCore {
            links: vec![WarpLink { from: "claude".into(), to: "grok".into(), age: 30.0, count: 1 }],
            ..core(&f)
        }
        .render(area, &mut linked);
        assert_ne!(text(&plain), text(&linked));
    }

    #[test]
    fn a_usage_alert_flickers_red() {
        let f = Starfield::new(0, 1);
        let area = Rect::new(0, 0, 80, 24);
        let alerts = vec![("claude".to_string(), 95.0)];
        let mut seen = std::collections::HashSet::new();
        for i in 0..20 {
            let mut buf = Buffer::empty(area);
            WarpCore { alerts: alerts.clone(), now: i as f64 * 0.13, ..core(&f) }.render(area, &mut buf);
            let t = text(&buf);
            assert!(t.contains('!') || t.contains('·'), "alert glyph missing at {i}");
            seen.insert(t);
        }
        assert!(seen.len() > 1, "the alert never changed");
    }

    #[test]
    fn spent_seat_stars_go_dim() {
        let mut f = Starfield::new(60, 9);
        f.set_split(&[("claude".into(), 1.0), ("codex".into(), 1.0)], true);
        let area = Rect::new(0, 0, 80, 24);
        let mut lit = Buffer::empty(area);
        core(&f).render(area, &mut lit);
        let mut dim = Buffer::empty(area);
        WarpCore { spent: vec!["codex".to_string()], ..core(&f) }.render(area, &mut dim);
        assert_ne!(lit, dim);
    }
}
