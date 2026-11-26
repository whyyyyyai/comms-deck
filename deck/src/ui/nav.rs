//! The nav view: the warp core in the centre, the instruments around it.
//! A viewer: nothing on this view sends or wakes anything.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::comms::wrap;
use super::theme::{bar, dim, display_name, meter_colour, panel, seat_colour, span_of, state_mark};
use super::warp::WarpCore;
use super::App;

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let [left, centre, right] = Layout::horizontal([
        Constraint::Percentage(24),
        Constraint::Percentage(50),
        Constraint::Percentage(26),
    ])
    .areas(area);
    let [core, tail] = Layout::vertical([Constraint::Percentage(68), Constraint::Percentage(32)]).areas(centre);
    let [split, ws] = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(left);
    let [vitals, pulse] = Layout::vertical([Constraint::Length(7), Constraint::Min(4)]).areas(right);
    draw_core(f, core, app);
    draw_tail(f, tail, app);
    draw_split(f, split, app);
    draw_workspaces(f, ws, app);
    draw_vitals(f, vitals, app);
    draw_pulse(f, pulse, app);
}

fn draw_core(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("04 WARP CORE");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let contacts = app
        .snap
        .seats
        .iter()
        .filter(|s| s.waiting > 0 && s.shown != "spent")
        .map(|s| (s.name.clone(), s.waiting))
        .collect();
    f.render_widget(
        WarpCore {
            field: &app.stars,
            warp: app.warp(),
            shield_low: app.snap.memory.available_pct() < 15.0,
            contacts,
        },
        inner,
    );
}

fn draw_split(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("06 CONTEXT SPLIT");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let split = app.snap.context_split();
    let total: f64 = split.iter().map(|(_, t)| t).sum();
    let bw = inner.width.saturating_sub(16).max(4) as usize;
    let mut lines = Vec::new();
    for (seat, tokens) in &split {
        let share = if total > 0.0 { 100.0 * tokens / total } else { 0.0 };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<8}", display_name(seat)), Style::default().fg(seat_colour(seat))),
            Span::styled(bar(share, bw), Style::default().fg(seat_colour(seat))),
            Span::styled(format!(" {share:>3.0}%"), dim()),
        ]));
        lines.push(Line::from(Span::styled(
            format!("        {}k tokens · {} stars", (tokens / 1000.0).round(), app.stars.count_for(seat)),
            dim(),
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("no seat reports its context", dim())));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_workspaces(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("07 WORKSPACES");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width.saturating_sub(4) as usize;
    let mut lines = Vec::new();
    for (ws, titles) in &app.snap.workspaces {
        let name = ws.strip_prefix("special:").map(|s| format!("✦ {s}")).unwrap_or_else(|| ws.clone());
        lines.push(Line::from(Span::styled(name, Style::default().fg(Color::Indexed(117)))));
        for t in titles {
            let t = wrap(t, w).into_iter().next().unwrap_or_default();
            lines.push(Line::from(Span::styled(format!("  {t}"), dim())));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_vitals(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("08 VITALS");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let m = &app.snap.memory;
    let used = 100.0 - m.available_pct();
    let bw = inner.width.saturating_sub(12).max(4) as usize;
    let swap_used = if m.swap_total_kb > 0 {
        100.0 * (m.swap_total_kb - m.swap_free_kb.min(m.swap_total_kb)) as f64 / m.swap_total_kb as f64
    } else {
        0.0
    };
    let mut lines = vec![
        Line::from(vec![
            Span::styled("mem  ", dim()),
            Span::styled(bar(used, bw), Style::default().fg(meter_colour(used))),
            Span::styled(format!(" {used:>3.0}%"), dim()),
        ]),
        Line::from(Span::styled(
            format!("     {:.1} of {:.1} GiB free", m.available_kb as f64 / 1048576.0, m.total_kb as f64 / 1048576.0),
            dim(),
        )),
    ];
    if m.swap_total_kb == 0 {
        lines.push(Line::from(Span::styled("swap none: an OOM kills the desktop", Style::default().fg(Color::Indexed(203)))));
    } else {
        lines.push(Line::from(vec![
            Span::styled("swap ", dim()),
            Span::styled(bar(swap_used, bw), Style::default().fg(meter_colour(swap_used))),
            Span::styled(format!(" {swap_used:>3.0}%"), dim()),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_pulse(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("09 CREW");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines = Vec::new();
    for s in &app.snap.seats {
        let (mark, colour) = state_mark(&s.shown);
        let waiting = if s.waiting > 0 { format!(" ✉{}", s.waiting) } else { String::new() };
        lines.push(Line::from(vec![
            Span::styled(format!("{mark} "), Style::default().fg(colour)),
            Span::styled(format!("{:<8}", display_name(&s.name)), Style::default().fg(seat_colour(&s.name))),
            Span::styled(format!("{:<6} {}", s.shown, span_of(s.since_s)), dim()),
            Span::styled(waiting, Style::default().fg(Color::Indexed(214))),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_tail(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("05 EVENT TAIL");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let h = inner.height as usize;
    let w = inner.width.saturating_sub(16) as usize;
    let start = app.snap.activity.len().saturating_sub(h);
    let lines: Vec<Line> = app.snap.activity[start..]
        .iter()
        .map(|a| {
            let ago = span_of(app.snap.time - a.time);
            let doing = wrap(&a.doing, w.max(4)).into_iter().next().unwrap_or_default();
            Line::from(vec![
                Span::styled(format!("{ago:>6} "), dim()),
                Span::styled(format!("{:<7}", display_name(&a.seat)), Style::default().fg(seat_colour(&a.seat))),
                Span::styled(format!("{:<5} ", a.state), Style::default().fg(state_mark(&a.state).1)),
                Span::styled(doing, dim()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}
