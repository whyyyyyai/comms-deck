//! The look: seat colours (the same xterm-256 numbers as comms-deck's
//! SEAT_COLOURS, so a seat is one colour on every screen), state marks,
//! rounded panels.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

pub const OWNER_NAME: &str = "Y";

pub fn seat_colour(seat: &str) -> Color {
    let n: u8 = match seat {
        "claude" => 208,
        "grok" => 45,
        "codex" => 78,
        "vibe" => 203,
        "devin" => 141,
        "owner" | "y" => 220,
        "push-courier" => 245,
        // comms-deck: 140 + sum(map(ord, seat)) % 40
        other => (140 + other.chars().map(|c| c as u32).sum::<u32>() % 40) as u8,
    };
    Color::Indexed(n)
}

/// A seat's name as people see it: the owner is Y.
pub fn display_name(seat: &str) -> &str {
    if seat == "owner" {
        OWNER_NAME
    } else {
        seat
    }
}

/// One symbol per state, so states differ by shape, not colour alone.
pub fn state_mark(shown: &str) -> (&'static str, Color) {
    match shown {
        "busy" => ("●", Color::Indexed(114)),
        "ready" | "idle" => ("○", Color::Indexed(250)),
        "stuck" => ("◐", Color::Indexed(214)),
        "spent" => ("○", Color::Indexed(240)),
        "away" => ("·", Color::Indexed(240)),
        _ => ("·", Color::Indexed(238)),
    }
}

pub fn panel(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Indexed(238)))
        .title(format!(" {title} "))
        .title_style(Style::default().fg(Color::Indexed(250)).add_modifier(Modifier::BOLD))
}

pub fn dim() -> Style {
    Style::default().fg(Color::Indexed(244))
}

/// "4m", "2h 05m", "3d 4h": an age or a countdown.
pub fn span_of(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (d, h, m) = (s / 86400, (s % 86400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{s}s")
    }
}

/// A bar of `width` cells for a percentage, in eighths.
pub fn bar(pct: f64, width: usize) -> String {
    const PARTS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    let cells = (pct.clamp(0.0, 100.0) / 100.0) * width as f64;
    let full = cells.floor() as usize;
    let mut s: String = "█".repeat(full.min(width));
    let rest = ((cells - full as f64) * 8.0).floor() as usize;
    if full < width && rest > 0 {
        s.push(PARTS[rest - 1]);
    }
    while s.chars().count() < width {
        s.push(' ');
    }
    s
}

/// Green below 60%, amber below 85%, red above: how full a meter is.
pub fn meter_colour(pct: f64) -> Color {
    if pct >= 85.0 {
        Color::Indexed(203)
    } else if pct >= 60.0 {
        Color::Indexed(214)
    } else {
        Color::Indexed(114)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_match_comms_deck() {
        assert_eq!(seat_colour("claude"), Color::Indexed(208));
        // comms-deck's fallback for an unlisted seat
        let n = 140 + "mystra".chars().map(|c| c as u32).sum::<u32>() % 40;
        assert_eq!(seat_colour("mystra"), Color::Indexed(n as u8));
    }

    #[test]
    fn bars_are_exactly_their_width() {
        for p in [0.0, 12.5, 50.0, 99.9, 100.0, 140.0] {
            assert_eq!(bar(p, 10).chars().count(), 10, "{p}");
        }
        assert_eq!(bar(100.0, 4), "████");
    }
}
