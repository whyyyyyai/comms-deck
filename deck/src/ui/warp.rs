//! 04 WARP CORE: a first-person starfield. Each star belongs to a seat and
//! wears its colour; stars are dealt out in proportion to the context split
//! (Y, 2026-11-26: "the stars in the warp core ... colored and distributed
//! according to the context split"). Speed is the deck's traffic.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

use super::theme::seat_colour;

#[derive(Clone, Debug)]
pub struct Star {
    x: f64,
    y: f64,
    z: f64,
    pub seat: Option<String>,
}

pub struct Starfield {
    pub stars: Vec<Star>,
    rng: u64,
    /// (seat, share 0..1), cumulative order kept stable for dealing.
    split: Vec<(String, f64)>,
}

const NEAR: f64 = 0.05;

impl Starfield {
    pub fn new(count: usize, seed: u64) -> Self {
        let mut f = Starfield { stars: Vec::with_capacity(count), rng: seed | 1, split: Vec::new() };
        for _ in 0..count {
            let mut s = Star { x: 0.0, y: 0.0, z: 0.0, seat: None };
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
            }
        }
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
    }

    /// Move every star toward the viewer. `speed` is depth per second.
    pub fn step(&mut self, dt: f64, speed: f64) {
        let mut stars = std::mem::take(&mut self.stars);
        for s in stars.iter_mut() {
            s.z -= dt * speed;
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
    let raw: Vec<(String, f64)> = split.iter().map(|(s, share)| (s.clone(), share * n as f64)).collect();
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

pub struct WarpCore<'a> {
    pub field: &'a Starfield,
    pub warp: f64,
    pub shield_low: bool,
    pub contacts: Vec<(String, u64)>,
}

/// The reticle's cell: the area's exact centre (nav-deck's HUD rule).
pub fn centre(area: Rect) -> (u16, u16) {
    (area.x + area.width / 2, area.y + area.height / 2)
}

impl Widget for WarpCore<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height < 3 {
            return;
        }
        let (cx, cy) = centre(area);
        let half_w = area.width as f64 / 2.0;
        let half_h = area.height as f64 / 2.0;
        for s in &self.field.stars {
            // Cells are about twice as tall as wide: halve y to keep it round.
            let px = cx as f64 + s.x / s.z * half_w * 0.5;
            let py = cy as f64 + s.y / s.z * half_h * 0.5;
            if px < area.x as f64 || py < area.y as f64 {
                continue;
            }
            let (px, py) = (px as u16, py as u16);
            if px >= area.right() || py >= area.bottom() {
                continue;
            }
            let glyph = if s.z < 0.25 { "✦" } else if s.z < 0.55 { "•" } else { "·" };
            let colour = s.seat.as_deref().map(seat_colour).unwrap_or(Color::Indexed(244));
            buf[(px, py)].set_symbol(glyph).set_style(Style::default().fg(colour));
        }
        // Contacts: seats with notes waiting, at a bearing of their own.
        for (i, (seat, waiting)) in self.contacts.iter().enumerate() {
            let angle = (i as f64 + 0.5) * std::f64::consts::TAU / self.contacts.len().max(1) as f64;
            let px = cx as f64 + angle.cos() * half_w * 0.6;
            let py = cy as f64 + angle.sin() * half_h * 0.6;
            let label = format!("{}{}", seat.chars().next().unwrap_or('?').to_ascii_uppercase(), waiting);
            let (px, py) = (px as u16, py as u16);
            if py >= area.y && py < area.bottom() && px >= area.x && (px as usize + label.len()) < area.right() as usize {
                buf.set_string(px, py, label, Style::default().fg(seat_colour(seat)));
            }
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
        let shield = if self.shield_low { " SHLD LOW " } else { " SHLD OK " };
        let colour = if self.shield_low { Color::Indexed(203) } else { Color::Indexed(114) };
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
    fn reticle_is_on_the_exact_centre() {
        let area = Rect::new(0, 0, 41, 21);
        let mut buf = Buffer::empty(area);
        let f = Starfield::new(0, 1);
        WarpCore { field: &f, warp: 1.0, shield_low: false, contacts: vec![] }.render(area, &mut buf);
        assert_eq!(buf[(20, 10)].symbol(), "╋");
    }
}
