//! The nav view: nav-deck in Rust. Same panels and places as
//! desktop/nav-deck (the owner, 2026-11-26: "location wise it's fine"), sized by
//! the window instead of by a fixed font: the left and right columns are
//! 30% each (40..80 columns) so the reticle keeps the centre line, and a
//! column's panels share its rows by what they need (share_rows), so no
//! panel is ever an empty frame.
//!
//! A viewer. The only actions are the ones nav-deck has: `w <seat>` runs
//! `comms-deck wake` (its rules decide), `g <ws>` focuses a workspace.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::comms::{draw_crew_titled, draw_input, draw_log, draw_usage, wrap};
use super::crew::Crew;
use super::theme::{bar, dim, display_name, meter_colour, panel, seat_colour};
use super::warp::{WarpCore, WarpLink};
use super::App;
use crate::navdata::{clock, event_time, human_dur, SPARK_POINTS};

const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// (left, centre, right): equal sides, 30% each, 40..80 columns.
pub fn nav_widths(cols: u16) -> (u16, u16, u16) {
    let side = ((cols as f64 * 0.30) as u16).clamp(40.min(cols / 3), 80);
    (side, cols.saturating_sub(2 * side), side)
}

/// The half view: the nav deck in two columns, for the tiled half-window
/// (`nav-deck-chat half`). Same ten panels, same keys — only the places
/// change. The eye keeps its order: the warp core top-left, the crew under
/// it, the readings down the right column in the full view's order.
pub fn draw_half(f: &mut Frame, area: Rect, app: &App) {
    let [left, right] = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(area);

    // left: 04 WARP CORE over 05 EVENT TAIL over 01 CREW. Below 18 rows
    // there's no room for the tail, so it is dropped and the core and crew
    // share the column.
    // 05 COMMS is the deck's chat now (the owner, 2026-11-26 23:55), so it
    // gets real room: about two fifths of the column.
    let tail_h = if area.height >= 18 { (area.height * 2 / 5).clamp(8, 24) } else { 0 };
    let core_h = (area.height.saturating_sub(tail_h) * 3 / 5).max(6.min(area.height));
    let [core, tail, crew] = Layout::vertical([
        Constraint::Length(core_h),
        Constraint::Length(tail_h),
        Constraint::Min(3),
    ])
    .areas(left);
    draw_core(f, core, app);
    if tail_h > 0 {
        draw_comms(f, tail, app);
    }
    draw_crew_map(f, crew, app);

    // right: 02 ACTIVITY on top, then the six readings sharing what's left
    let act_h = (area.height * 3 / 10).max(8.min(area.height));
    let [activity, rest] = Layout::vertical([Constraint::Length(act_h), Constraint::Min(0)]).areas(right);
    draw_crew_titled(f, activity, app, "02 ACTIVITY · ready order");

    let inner_w = right.width.saturating_sub(2) as usize;
    let usage_lines = usage_line_count(app);
    let ws_lines = workspace_lines(app, inner_w).len() as u16;
    let vit_lines = vitals_lines(app, inner_w).len() as u16;
    let nt_lines = nav_time_lines(app, inner_w).len() as u16;
    let pulse_n = pulse_lines(app, inner_w).len() as u16;
    let split_n = split_lines(app, inner_w).len() as u16;
    // 10 CONTEXT SPLIT is sized by its box and legend; 03 USAGE, the
    // longest list, takes what's left.
    let _ = usage_lines;
    let needs = [
        None,
        Some(ws_lines + 2),
        Some(vit_lines + 2),
        Some(split_n + 2),
        Some(nt_lines + 2),
        Some(pulse_n + 2),
    ];
    let heights = share_rows(&needs, rest.height, 3);
    let r = stack(rest, &heights);
    if r[0].height >= 3 {
        draw_usage(f, r[0], app);
    }
    lines_panel(f, r[1], "06 WORKSPACES · hyprctl clients", workspace_lines(app, inner_w));
    lines_panel(f, r[2], "07 VITALS", vitals_lines(app, inner_w));
    lines_panel(f, r[3], "10 CONTEXT SPLIT · the shared pool", split_lines(app, inner_w));
    lines_panel(f, r[4], "08 NAV-TIME · are the two decks in step", nav_time_lines(app, inner_w));
    lines_panel(f, r[5], "09 DECK PULSE · how hard the notes flew", pulse_lines(app, inner_w));
}

/// Rows for stacked panels (nav-deck's share_rows): a floor for every panel
/// first, then one row at a time round the panels that still want more
/// (water-filling), so short panels end up whole and long ones trim to one
/// level; `None` panels take what's left. If even the floors don't fit, the
/// last panels get 0.
pub fn share_rows(needs: &[Option<u16>], total: u16, floor: u16) -> Vec<u16> {
    let n = needs.len();
    let want: Vec<u16> = needs.iter().map(|w| w.unwrap_or(floor).max(floor)).collect();
    let mut rows = vec![0u16; n];
    let mut left = total;
    for r in rows.iter_mut() {
        if left >= floor {
            *r = floor;
            left -= floor;
        }
    }
    let mut hungry: Vec<usize> = (0..n).filter(|&i| rows[i] > 0 && needs[i].is_some() && rows[i] < want[i]).collect();
    while left > 0 && !hungry.is_empty() {
        let mut still = Vec::new();
        for &i in &hungry {
            if left == 0 {
                still.push(i);
                continue;
            }
            rows[i] += 1;
            left -= 1;
            if rows[i] < want[i] {
                still.push(i);
            }
        }
        hungry = still;
    }
    let flex: Vec<usize> = (0..n).filter(|&i| needs[i].is_none() && rows[i] > 0).collect();
    let count = flex.len() as u16;
    for (k, &i) in flex.iter().enumerate() {
        let part = left / (count - k as u16);
        rows[i] += part;
        left -= part;
    }
    if left > 0 {
        if let Some(last) = (0..n).rev().find(|&i| rows[i] > 0) {
            rows[last] += left;
        }
    }
    rows
}

fn stack(area: Rect, heights: &[u16]) -> Vec<Rect> {
    let mut y = area.y;
    heights
        .iter()
        .map(|&h| {
            let r = Rect::new(area.x, y, area.width, h.min(area.bottom().saturating_sub(y)));
            y += r.height;
            r
        })
        .collect()
}

/// Zoomed (z): the warp core alone, over the whole body, in any layout.
pub fn draw_zoomed(f: &mut Frame, area: Rect, app: &App) {
    draw_core(f, area, app);
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let (lw, cw, rw) = nav_widths(area.width);
    let [left, centre, right] =
        Layout::horizontal([Constraint::Length(lw), Constraint::Length(cw), Constraint::Length(rw)]).areas(area);

    // left: 01 CREW over 02 ACTIVITY, crew takes 45% like nav-deck
    let crew_h = ((left.height as f64 * 0.45) as u16).max(10.min(left.height));
    let [crew, activity] = Layout::vertical([Constraint::Length(crew_h), Constraint::Min(0)]).areas(left);
    draw_crew_map(f, crew, app);
    draw_crew_titled(f, activity, app, "02 ACTIVITY · ready order");

    // centre: 04 WARP CORE with 05 COMMS under it. The chat replaced the
    // event tail (the owner, 2026-11-26 23:55), so it takes two fifths.
    let tail_h = (centre.height * 2 / 5).clamp(8.min(centre.height), 30);
    let [core, tail] = Layout::vertical([Constraint::Min(3), Constraint::Length(tail_h)]).areas(centre);
    draw_core(f, core, app);
    draw_comms(f, tail, app);

    // right: 03 USAGE, 06 WORKSPACES, 07 VITALS, 10 CONTEXT SPLIT, 08 NAV-TIME, 09 DECK PULSE
    let inner_w = rw.saturating_sub(2) as usize;
    let usage_lines = usage_line_count(app);
    let ws_lines = workspace_lines(app, inner_w).len() as u16;
    let vit_lines = vitals_lines(app, inner_w).len() as u16;
    let nt_lines = nav_time_lines(app, inner_w).len() as u16;
    let pulse_n = pulse_lines(app, inner_w).len() as u16;
    let split_n = split_lines(app, inner_w).len() as u16;
    // 10 CONTEXT SPLIT is sized by its box and legend; 03 USAGE, the
    // longest list, takes what's left.
    let _ = usage_lines;
    let needs = [
        None,
        Some(ws_lines + 2),
        Some(vit_lines + 2),
        Some(split_n + 2),
        Some(nt_lines + 2),
        Some(pulse_n + 2),
    ];
    let heights = share_rows(&needs, right.height, 3);
    let r = stack(right, &heights);
    if r[0].height >= 3 {
        draw_usage(f, r[0], app);
    }
    lines_panel(f, r[1], "06 WORKSPACES · hyprctl clients", workspace_lines(app, inner_w));
    lines_panel(f, r[2], "07 VITALS", vitals_lines(app, inner_w));
    lines_panel(f, r[3], "10 CONTEXT SPLIT · the shared pool", split_lines(app, inner_w));
    lines_panel(f, r[4], "08 NAV-TIME · are the two decks in step", nav_time_lines(app, inner_w));
    lines_panel(f, r[5], "09 DECK PULSE · how hard the notes flew", pulse_lines(app, inner_w));
}

fn usage_line_count(app: &App) -> u16 {
    app.snap
        .usage
        .values()
        .map(|u| {
            let rows = [u.context_pct, u.five_hour_pct, u.week_pct].iter().filter(|p| p.is_some()).count();
            if rows == 0 { 0 } else { rows + 1 }
        })
        .sum::<usize>() as u16
}

fn lines_panel(f: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>) {
    if area.height < 3 {
        return;
    }
    let block = panel(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_crew_map(f: &mut Frame, area: Rect, app: &App) {
    let working = app.snap.seats.iter().filter(|s| s.shown == "busy").count();
    let block = panel("01 CREW · who talks to whom").title_bottom(
        Line::from(Span::styled(format!(" ● {working} WORKING "), Style::default().fg(Color::Indexed(114)))).right_aligned(),
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(Crew { snap: &app.snap, now: app.snap.time }, inner);
}

/// The note streaks the warp core draws: notes of the last 30 minutes (the
/// newest 300), grouped sender -> receiver, "all" fanned out to the seats —
/// the same window the crew map's links use.
fn warp_links(app: &App) -> Vec<WarpLink> {
    use std::collections::HashMap;
    const SHOWN: f64 = 1800.0;
    let now = app.snap.time;
    let seats: Vec<&str> = app.snap.seats.iter().filter(|s| !s.hidden).map(|s| s.name.as_str()).collect();
    let mut known: Vec<&str> = seats.clone();
    known.push("owner");
    let mut agg: HashMap<(String, String), (u32, f64)> = HashMap::new();
    for n in app.snap.notes.iter().rev().take(300) {
        let t = event_time(Some(&serde_json::Value::String(n.time.clone())));
        let age = now - t;
        if t <= 0.0 || !(0.0..SHOWN).contains(&age) {
            continue;
        }
        let targets: Vec<&str> = if n.to == "all" { seats.clone() } else { n.to.split('+').collect() };
        for to in targets {
            if to == n.from || !known.contains(&to) {
                continue;
            }
            let e = agg.entry((n.from.clone(), to.to_string())).or_insert((0, age));
            e.0 += 1;
            e.1 = e.1.min(age);
        }
    }
    let mut out: Vec<WarpLink> = agg.into_iter().map(|((from, to), (count, age))| WarpLink { from, to, age, count }).collect();
    out.sort_by(|a, b| a.age.partial_cmp(&b.age).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(24);
    out
}

/// Seats within reach of a usage limit: the fullest of ctx / 5h / week at or
/// over 90%. A spent seat is already out, so it doesn't flicker.
fn usage_alerts(app: &App) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = app
        .snap
        .usage
        .iter()
        .filter_map(|(seat, u)| {
            let pct = [u.context_pct, u.five_hour_pct, u.week_pct].into_iter().flatten().fold(0.0f64, f64::max);
            let spent = app.snap.seat(seat).map_or(false, |s| s.shown == "spent");
            (pct >= 90.0 && !spent).then(|| (seat.clone(), pct))
        })
        .collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(6);
    out
}

fn draw_core(f: &mut Frame, area: Rect, app: &App) {
    let warp = app.warp();
    let title = if app.zoom { "04 WARP CORE · zoomed (z back)" } else { "04 WARP CORE · the pilot's seat" };
    let block = panel(title).title_bottom(
        Line::from(Span::styled(format!(" WARP {warp:.1} "), Style::default().fg(Color::Indexed(117)))).right_aligned(),
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    app.core_area.set(inner);
    let ring = if app.zoom {
        app.snap
            .seats
            .iter()
            .filter(|s| !s.hidden)
            .map(|s| {
                let mail = if s.waiting > 0 { format!(" ✉{}", s.waiting) } else { String::new() };
                (s.name.clone(), format!("{} {}{}", display_name(&s.name), s.shown, mail))
            })
            .collect()
    } else {
        Vec::new()
    };
    f.render_widget(
        WarpCore {
            field: &app.stars,
            warp,
            shield_pct: app.snap.memory.available_pct(),
            contacts: app.contacts(),
            selected: app.selected.clone(),
            ring,
            links: warp_links(app),
            alerts: usage_alerts(app),
            spent: app.snap.seats.iter().filter(|s| s.shown == "spent").map(|s| s.name.clone()).collect(),
            now: app.now,
        },
        inner,
    );
}

/// 05 EVENT TAIL: who changed state and who said what, newest last.
/// 05 COMMS: the conversation and the chat box, so nav is the whole deck
/// (the owner, 2026-11-26 23:55: "chat on nav 05, replacing event tail").
fn draw_comms(f: &mut Frame, area: Rect, app: &App) {
    app.chat_area.set(area);
    if area.height < 5 {
        draw_log(f, area, app, "05 COMMS · Enter to write");
        return;
    }
    let [log, input] = Layout::vertical([Constraint::Min(2), Constraint::Length(3)]).areas(area);
    let title = if app.typing { "05 COMMS · writing (Esc stops)" } else { "05 COMMS · Enter to write" };
    draw_log(f, log, app, title);
    draw_input(f, input, app, app.typing);
}

/// The 10 CONTEXT SPLIT box: every dot an equal slice of the context pool,
/// seats filling in order (biggest first) in their colour, a busy seat's
/// dots breathing. The comms screen's 06 CONTEXT SPLIT, moved here in
/// place of 10 COCKPIT-CHAT (the owner, 2026-11-26 23:55).
pub fn split_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let split = app.snap.context_split();
    let total: f64 = split.iter().map(|(_, t)| t).sum();
    if split.is_empty() || total <= 0.0 {
        return vec![Line::from(Span::styled(" no seat reports its context yet", dim()))];
    }
    let mut order = split.clone();
    order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let cols = width.saturating_sub(2).clamp(10, 80);
    let rows = 4;
    let counts = super::warp::star_counts(&order, cols * rows);
    let owners: Vec<&str> = counts.iter().flat_map(|(s, n)| std::iter::repeat(s.as_str()).take(*n)).collect();
    let busy: Vec<&str> = app.snap.seats.iter().filter(|s| s.shown == "busy").map(|s| s.name.as_str()).collect();
    let mut out = Vec::new();
    for r in 0..rows {
        let mut spans = vec![Span::raw(" ")];
        for c in 0..cols {
            let Some(&seat) = owners.get(r * cols + c) else { break };
            let mut style = Style::default().fg(seat_colour(seat));
            if busy.contains(&seat) {
                // a wave runs through the box, dot by dot, while the seat works
                let phase = (app.now * 1.5 - (r * cols + c) as f64 * 0.08).rem_euclid(1.0);
                style = style.add_modifier(if phase < 0.5 { Modifier::BOLD } else { Modifier::DIM });
            }
            spans.push(Span::styled("●", style));
        }
        out.push(Line::from(spans));
    }
    for (seat, used) in &order {
        out.push(Line::from(vec![
            Span::styled(" ■ ", Style::default().fg(seat_colour(seat))),
            Span::raw(format!("{:8} {:4.1}%  ", display_name(seat), 100.0 * used / total)),
            Span::styled(short_tokens(*used), dim()),
        ]));
    }
    out.push(Line::from(vec![
        Span::styled(" Total ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(short_tokens(total)),
        Span::styled(format!(" across {} seats", order.len()), dim()),
    ]));
    out
}

fn short_tokens(t: f64) -> String {
    if t >= 1e6 {
        format!("{:.1}M", t / 1e6)
    } else if t >= 1e3 {
        format!("{:.0}k", t / 1e3)
    } else {
        format!("{t:.0}")
    }
}

#[allow(dead_code)] // kept for a later home: the event tail before 05 became the chat
fn draw_tail(f: &mut Frame, area: Rect, app: &App) {
    let block = panel("05 EVENT TAIL · the cockpit's log");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let mut events: Vec<(f64, Line<'static>)> = Vec::new();
    for n in app.snap.notes.iter().rev().take(40) {
        let t = crate::navdata::event_time(Some(&serde_json::Value::String(n.time.clone())));
        let to = if n.to == "owner" { "Y".to_string() } else { n.to.clone() };
        let text = wrap(&n.text, w.saturating_sub(20).max(8)).into_iter().next().unwrap_or_default();
        events.push((
            t,
            Line::from(vec![
                Span::styled(display_name(&n.from).to_string(), Style::default().fg(seat_colour(&n.from))),
                Span::styled(format!(" → {to}: "), dim()),
                Span::raw(text),
            ]),
        ));
    }
    for a in app.snap.activity.iter().rev().take(40) {
        let doing = if a.doing.is_empty() {
            String::new()
        } else {
            format!(" · {}", wrap(&a.doing, w.saturating_sub(24).max(8)).into_iter().next().unwrap_or_default())
        };
        events.push((
            a.time,
            Line::from(vec![
                Span::styled(display_name(&a.seat).to_string(), Style::default().fg(seat_colour(&a.seat))),
                Span::raw(format!(" {}", a.state)),
                Span::styled(doing, dim()),
            ]),
        ));
    }
    events.retain(|(t, _)| t.is_finite() && *t > 0.0);
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let h = inner.height as usize;
    let start = events.len().saturating_sub(h);
    let lines: Vec<Line> = events[start..]
        .iter()
        .filter_map(|(t, l)| {
            let stamp = clock(*t)?;
            let mut spans = vec![Span::styled(format!(" {stamp}  "), dim())];
            spans.extend(l.spans.iter().cloned());
            Some(Line::from(spans))
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(" the decks are quiet", dim())), inner);
    } else {
        f.render_widget(Paragraph::new(lines), inner);
    }
}

fn workspace_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(16).max(8);
    let mut out = Vec::new();
    for (ws, titles) in &app.snap.workspaces {
        let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
        let mut used = 0;
        for t in titles {
            let limit = room.saturating_sub(3).clamp(6, 28);
            let shown: String = if t.chars().count() > limit {
                t.chars().take(limit - 1).chain(std::iter::once('…')).collect()
            } else {
                t.clone()
            };
            let chip = format!("[{shown}]");
            let cw = chip.chars().count();
            if used > 0 && used + 1 + cw > room {
                rows.push(Vec::new());
                used = 0;
            }
            if used > 0 {
                rows.last_mut().unwrap().push(Span::raw(" "));
                used += 1;
            }
            let colour = if t.starts_with("nav-deck") || t.starts_with("deck") { Color::Indexed(81) } else { Color::Indexed(250) };
            rows.last_mut().unwrap().push(Span::styled(chip, Style::default().fg(colour)));
            used += cw;
        }
        for (i, row) in rows.into_iter().enumerate() {
            let head = if i == 0 { format!(" {ws:<15}") } else { " ".repeat(16) };
            let mut spans = vec![Span::styled(head, Style::default().fg(Color::Indexed(245)))];
            spans.extend(row);
            out.push(Line::from(spans));
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(" hyprctl gave no windows", dim())));
    }
    out
}

fn spark(values: &[f64]) -> String {
    let (lo, hi) = values.iter().fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (hi - lo).max(1e-9);
    values.iter().map(|v| BLOCKS[1 + (((v - lo) / span) * 7.0).round() as usize]).collect()
}

fn vitals_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let m = &app.snap.memory;
    let nav = &app.snap.nav;
    if m.total_kb == 0 {
        return vec![Line::from(Span::styled(" /proc/meminfo unreadable", dim()))];
    }
    let avail = m.available_kb * 1024;
    let col = if avail < 2 * crate::navdata::MEM_FLOOR { Color::Indexed(214) } else { Color::Indexed(78) };
    let used = 100.0 - m.available_pct();
    let bw = width.saturating_sub(26).clamp(6, 30);
    let mut out = vec![Line::from(vec![
        Span::styled(" memfree ", dim()),
        Span::styled(bar(used, bw), Style::default().fg(col)),
        Span::styled(format!(" {:.1} GiB free", avail as f64 / 1073741824.0), Style::default().fg(col)),
        Span::styled(format!(" / {:.0}", m.total_kb as f64 / 1048576.0), dim()),
    ])];
    if !nav.memory.is_empty() {
        let vals: Vec<f64> = nav.memory.iter().map(|(_, v)| *v as f64 / 1073741824.0).collect();
        let take = width.saturating_sub(3).min(SPARK_POINTS);
        let vals = &vals[vals.len().saturating_sub(take)..];
        out.push(Line::from(Span::styled(format!(" {}", spark(vals)), Style::default().fg(Color::Indexed(250)))));
    }
    if m.swap_total_kb == 0 {
        out.push(Line::from(Span::styled(" swap  none — the next squeeze has nowhere to go", Style::default().fg(Color::Indexed(214)))));
    } else {
        let su = 100.0 * (m.swap_total_kb - m.swap_free_kb.min(m.swap_total_kb)) as f64 / m.swap_total_kb as f64;
        out.push(Line::from(vec![
            Span::styled(" swap  ", dim()),
            Span::styled(bar(su, 10), Style::default().fg(meter_colour(su))),
            Span::styled(format!(" {:.1} GiB", m.swap_total_kb as f64 / 1048576.0), dim()),
        ]));
    }
    let low = nav.low_samples;
    out.push(Line::from(vec![
        Span::styled(format!(" load {:.2}   below-512-MiB in two days: ", nav.load), dim()),
        Span::styled(low.to_string(), Style::default().fg(if low > 0 { Color::Indexed(214) } else { Color::Indexed(78) })),
    ]));
    out.push(if nav.streams.is_empty() {
        Line::from(Span::styled(" stream idle (obs/ffmpeg on the box)", dim()))
    } else {
        Line::from(vec![
            Span::styled(" stream ", dim()),
            Span::styled(nav.streams.join(" · "), Style::default().fg(Color::Indexed(78))),
            Span::styled(" up", dim()),
        ])
    });
    out
}

#[allow(dead_code)] // 10 is the context split now; twitch chat waits for a new home
fn draw_chat(f: &mut Frame, area: Rect, app: &App) {
    if area.height < 3 {
        return;
    }
    let nav = &app.snap.nav;
    let (tag, colour) = match nav.chat_state.as_str() {
        "live" => ("● LIVE", Color::Indexed(42)),
        "auth-failed" => ("⚠ AUTH", Color::Indexed(196)),
        "connecting" => ("◌ connecting", Color::Indexed(226)),
        "config" => ("○ config", Color::Indexed(214)),
        _ => ("○ idle", Color::Indexed(244)),
    };
    let block = panel("10 COCKPIT-CHAT · what the channel says")
        .title_bottom(Line::from(Span::styled(format!(" {tag} "), Style::default().fg(colour))).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width as usize;
    let h = inner.height as usize;
    let start = nav.chat.len().saturating_sub(h);
    let lines: Vec<Line> = nav.chat[start..]
        .iter()
        .map(|c| {
            let stamp = clock(c.time).unwrap_or_else(|| clock(app.snap.time).unwrap_or_default());
            let (chip, col) = if c.broadcaster {
                ("◆", Color::Indexed(208))
            } else if c.moderator {
                ("＋", Color::Indexed(86))
            } else {
                (" ", Color::Indexed(99))
            };
            let text = wrap(&c.text, w.saturating_sub(c.user.chars().count() + 10).max(8)).into_iter().next().unwrap_or_default();
            Line::from(vec![
                Span::styled(format!(" {stamp} "), dim()),
                Span::styled(format!("{chip} {}", c.user), Style::default().fg(col).add_modifier(Modifier::BOLD)),
                Span::raw(format!(" {text}")),
            ])
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(" no twitch chat — idle or the channel is quiet", dim())), inner);
    } else {
        f.render_widget(Paragraph::new(lines), inner);
    }
}

fn nav_time_lines(app: &App, _width: usize) -> Vec<Line<'static>> {
    let nav = &app.snap.nav;
    let mut out = Vec::new();
    for s in &nav.surfaces {
        let (shown, col) = match s.age {
            None => ("never".to_string(), Color::Indexed(245)),
            Some(a) if a < 60.0 => (human_dur(a), Color::Indexed(78)),
            Some(a) if a < 600.0 => (human_dur(a), Color::Indexed(226)),
            Some(a) => (human_dur(a), Color::Indexed(214)),
        };
        out.push(Line::from(vec![
            Span::styled(format!(" {:<15}", s.name), Style::default().fg(Color::Indexed(245))),
            Span::styled(format!("{shown:>7}"), Style::default().fg(col)),
            Span::styled(format!("  {}", s.detail), dim()),
        ]));
    }
    let mut lat = nav.latency.clone();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if let (Some(&mn), Some(&mx)) = (lat.first(), lat.last()) {
        let med = lat[lat.len() / 2];
        let col = if mx < 5.0 { Color::Indexed(78) } else if mx < 30.0 { Color::Indexed(226) } else { Color::Indexed(214) };
        let mut spans = vec![
            Span::styled(format!(" {:<15}", "note→inbox"), dim()),
            Span::styled(format!("med {}  max {}", human_dur(med), human_dur(mx)), Style::default().fg(col)),
        ];
        if mn < -1.0 {
            spans.push(Span::styled(format!("  skew {}", human_dur(-mn)), Style::default().fg(Color::Indexed(214))));
        }
        spans.push(Span::styled(format!("  last {}", lat.len()), dim()));
        out.push(Line::from(spans));
    }
    let waiting: u64 = app.snap.seats.iter().map(|s| s.waiting).sum();
    let log_age = nav.surfaces.iter().find(|s| s.name == "log.jsonl").and_then(|s| s.age);
    if let Some(age) = log_age {
        if waiting > 0 && age > 600.0 {
            out.push(Line::from(Span::styled(
                format!(" ⚠ {waiting} notes waiting but the log has been silent {}", human_dur(age)),
                Style::default().fg(Color::Indexed(214)),
            )));
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(" no timing data", dim())));
    }
    out
}

fn pulse_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let nav = &app.snap.nav;
    let total: u32 = nav.hours.iter().sum();
    let mut out = Vec::new();
    if total == 0 {
        out.push(Line::from(Span::styled(" no notes yet today", dim())));
    } else {
        let peak = *nav.hours.iter().max().unwrap_or(&1) as f64;
        let cells: String = (0..24)
            .map(|h| {
                let n = nav.hours[h];
                if n > 0 {
                    BLOCKS[((n as f64 / peak) * 8.0).round().clamp(1.0, 8.0) as usize]
                } else if h % 6 == 0 {
                    '┬'
                } else {
                    '·'
                }
            })
            .collect();
        out.push(Line::from(vec![
            Span::styled(format!(" {cells}"), Style::default().fg(Color::Indexed(81))),
            Span::styled(format!(" {total} notes today"), dim()),
        ]));
        out.push(Line::from(Span::styled(" 0a    6a    12p   6p", dim())));
    }
    if !nav.busy.is_empty() {
        let mut top: Vec<(&String, &f64)> = nav.busy.iter().collect();
        top.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut spans = vec![Span::styled(" busy today: ", dim())];
        let mut used = 13;
        for (seat, secs) in top.into_iter().take(4) {
            let item = format!("{} {}  ", display_name(seat), human_dur(*secs));
            if used + item.chars().count() > width {
                break;
            }
            used += item.chars().count();
            spans.push(Span::styled(display_name(seat).to_string(), Style::default().fg(seat_colour(seat))));
            spans.push(Span::styled(format!(" {}  ", human_dur(*secs)), dim()));
        }
        out.push(Line::from(spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Note, Seat, Snapshot};
    use crate::ui::{App, View};

    #[test]
    fn warp_links_group_notes_and_fan_out_all() {
        let mut snap = Snapshot::default();
        snap.seats = vec![
            Seat { name: "claude".into(), shown: "busy".into(), ..Default::default() },
            Seat { name: "grok".into(), shown: "ready".into(), ..Default::default() },
        ];
        let t = event_time(Some(&serde_json::Value::String("2026-11-26 16:16:48".into())));
        snap.time = t + 60.0;
        snap.notes = vec![
            Note { from: "claude".into(), to: "grok".into(), time: "2026-11-26 16:16:48".into(), text: "a".into(), quiet: false },
            Note { from: "claude".into(), to: "grok".into(), time: "2026-11-26 16:16:48".into(), text: "b".into(), quiet: false },
            Note { from: "owner".into(), to: "all".into(), time: "2026-11-26 16:16:48".into(), text: "c".into(), quiet: false },
        ];
        let app = App::new(View::Nav, snap);
        let links = warp_links(&app);
        let pair = |f: &str, t: &str| links.iter().find(|l| l.from == f && l.to == t);
        assert_eq!(pair("claude", "grok").map(|l| l.count), Some(2));
        assert!(pair("owner", "claude").is_some() && pair("owner", "grok").is_some());
        assert!(links.iter().all(|l| l.age < 1800.0));
    }

    #[test]
    fn rows_are_shared_like_nav_deck() {
        let n = [Some(15), Some(11), Some(9), None, Some(10), Some(7)];
        let r = share_rows(&n, 45, 5);
        assert_eq!(r.iter().sum::<u16>(), 45);
        assert!(r.iter().all(|&x| x >= 5), "{r:?}");
        assert_eq!(share_rows(&[Some(4), Some(4), None], 30, 5), vec![5, 5, 20]);
        assert_eq!(share_rows(&[Some(20), Some(20), None], 12, 5)[2], 0);
        assert_eq!(share_rows(&[Some(24), Some(7), Some(9), None, Some(8), Some(6)], 47, 5)[1], 7);
    }

    #[test]
    fn sides_are_equal_so_the_reticle_is_centred() {
        for cols in [80u16, 150, 209, 250, 400] {
            let (l, c, r) = nav_widths(cols);
            assert_eq!(l, r);
            assert_eq!(l + c + r, cols);
        }
    }
}
