//! The comms view: the conversation on the left with a text box under it,
//! the crew, activity and usage on the right.

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

use super::theme::{bar, dim, display_name, meter_colour, panel, seat_colour, span_of, state_mark};
use super::App;
use crate::data::Note;

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let narrow = area.width < 100;
    let [left, right] = if narrow {
        [area, Rect::default()]
    } else {
        Layout::horizontal([Constraint::Min(60), Constraint::Length(42)]).areas(area)
    };
    let [log_area, input_area] = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(left);
    draw_log(f, log_area, app, "COMMS");
    draw_input(f, input_area, app, true);
    if !narrow {
        let [crew, usage] = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(right);
        draw_crew(f, crew, app);
        draw_usage(f, usage, app);
    }
}

/// Split `text` into rows of at most `width` display cells.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    let mut row = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        row.push(c);
        used += w;
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

fn note_lines(n: &Note, width: usize) -> Vec<Line<'static>> {
    let clock = n.time.get(11..16).unwrap_or("").to_string();
    let who = display_name(&n.from).to_string();
    let to = if n.to == "owner" { "Y".to_string() } else { n.to.clone() };
    let head_w = clock.len() + 1 + who.chars().count() + 3 + to.chars().count() + 2;
    let body_w = width.saturating_sub(head_w).max(10);
    let mut out = Vec::new();
    let text_style = if n.quiet { dim() } else { Style::default() };
    for (i, row) in wrap(&n.text, body_w).into_iter().enumerate() {
        if i == 0 {
            out.push(Line::from(vec![
                Span::styled(format!("{clock} "), dim()),
                Span::styled(who.clone(), Style::default().fg(seat_colour(&n.from)).add_modifier(Modifier::BOLD)),
                Span::styled(" → ", dim()),
                Span::styled(to.clone(), Style::default().fg(seat_colour(&n.to))),
                Span::raw("  "),
                Span::styled(row, text_style),
            ]));
        } else {
            out.push(Line::from(vec![Span::raw(" ".repeat(head_w)), Span::styled(row, text_style)]));
        }
    }
    out
}

pub fn draw_log(f: &mut Frame, area: Rect, app: &App, title: &str) {
    let block = panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let width = inner.width as usize;
    let height = inner.height as usize;
    // Newest last; build from the end until the screen (plus the scroll) is full.
    let mut lines: Vec<Line> = Vec::new();
    for n in app.snap.notes.iter().rev() {
        let mut l = note_lines(n, width);
        l.extend(lines);
        lines = l;
        if lines.len() >= height + app.scroll {
            break;
        }
    }
    let end = lines.len().saturating_sub(app.scroll);
    let start = end.saturating_sub(height);
    let shown: Vec<Line> = lines[start..end].to_vec();
    if shown.is_empty() {
        let why = app.snap.problems.first().cloned().unwrap_or_else(|| "no notes yet".into());
        f.render_widget(Paragraph::new(Span::styled(why, dim())), inner);
        return;
    }
    f.render_widget(Paragraph::new(shown), inner);
}

/// The chat box. `focused`: it has the keyboard, so it shows the cursor.
pub fn draw_input(f: &mut Frame, area: Rect, app: &App, focused: bool) {
    let to = crate::send::route(&app.input, &app.seat_names());
    let title = match &to {
        Ok(t) => format!("to {t}"),
        Err(e) => e.clone(),
    };
    let block = panel(&title).border_style(Style::default().fg(match to {
        _ if !focused => Color::Indexed(240),
        Ok(_) => Color::Indexed(117),
        Err(_) => Color::Indexed(203),
    }));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Keep the cursor in view: scroll the line horizontally.
    let before: String = app.input.chars().take(app.cursor).collect();
    let before_w: usize = before.chars().map(|c| c.width().unwrap_or(0)).sum();
    let room = inner.width.saturating_sub(3) as usize;
    let skip = before_w.saturating_sub(room);
    let mut shown = String::new();
    let mut seen = 0;
    for c in app.input.chars() {
        let w = c.width().unwrap_or(0);
        if seen >= skip {
            shown.push(c);
        }
        seen += w;
    }
    let prompt = "y › ";
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(prompt, Style::default().fg(seat_colour("owner"))), Span::raw(shown)])),
        inner,
    );
    if !focused {
        return;
    }
    let x = inner.x + prompt.chars().count() as u16 + (before_w - skip) as u16;
    f.set_cursor_position(Position::new(x.min(inner.right().saturating_sub(1)), inner.y));
}

pub fn draw_crew(f: &mut Frame, area: Rect, app: &App) {
    draw_crew_titled(f, area, app, "01 CREW · 02 ACTIVITY");
}

pub fn draw_crew_titled(f: &mut Frame, area: Rect, app: &App, title: &str) {
    let block = panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines = Vec::new();
    for s in &app.snap.seats {
        let (mark, colour) = state_mark(&s.shown);
        let mut spans = vec![
            Span::styled(format!("{mark} "), Style::default().fg(colour)),
            Span::styled(format!("{:<8}", display_name(&s.name)), Style::default().fg(seat_colour(&s.name))),
            Span::styled(format!("{:<7} ", s.shown), Style::default().fg(colour)),
        ];
        let when = match s.spent_until {
            Some(t) if s.shown == "spent" => format!("back in {}", span_of(t - app.snap.time)),
            _ => span_of(s.since_s),
        };
        spans.push(Span::styled(format!("{when:<10} "), dim()));
        if s.waiting > 0 {
            spans.push(Span::styled(format!("✉{}", s.waiting), Style::default().fg(Color::Indexed(214))));
        }
        lines.push(Line::from(spans));
        if !s.doing.is_empty() {
            let w = inner.width.saturating_sub(4) as usize;
            let doing = wrap(&s.doing, w).into_iter().next().unwrap_or_default();
            lines.push(Line::from(Span::styled(format!("  ↳ {doing}"), dim())));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

pub fn draw_usage(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("03 USAGE");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let bw = inner.width.saturating_sub(18).max(4) as usize;
    let mut lines = Vec::new();
    for (seat, u) in &app.snap.usage {
        let row = |label: &str, pct: Option<f64>| -> Option<Line<'static>> {
            let p = pct?;
            Some(Line::from(vec![
                Span::styled(format!("  {label:<4}"), dim()),
                Span::styled(bar(p, bw), Style::default().fg(meter_colour(p))),
                Span::styled(format!(" {p:>3.0}%"), dim()),
            ]))
        };
        let rows: Vec<Line> = [row("ctx", u.context_pct), row("5h", u.five_hour_pct), row("week", u.week_pct)]
            .into_iter()
            .flatten()
            .collect();
        if rows.is_empty() {
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{seat} "), Style::default().fg(seat_colour(seat))),
            Span::styled(u.model.clone(), dim()),
        ]));
        lines.extend(rows);
    }
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::wrap;

    #[test]
    fn wrap_counts_wide_characters() {
        assert_eq!(wrap("abcdef", 4), ["abcd", "ef"]);
        assert_eq!(wrap("日本語", 4), ["日本", "語"]);
        assert_eq!(wrap("", 4), [""]);
    }
}
