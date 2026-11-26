//! The screen: one app, two views (comms and nav), switched with a key.

pub mod comms;
pub mod crew;
pub mod nav;
pub mod theme;
pub mod warp;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::data::Snapshot;
use theme::{dim, display_name};
use warp::Starfield;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Comms,
    Nav,
}

pub struct App {
    pub view: View,
    pub snap: Snapshot,
    pub input: String,
    /// Cursor position in `input`, in chars.
    pub cursor: usize,
    /// Lines scrolled up from the newest note.
    pub scroll: usize,
    /// The last send's answer: (text, ok).
    pub flash: Option<(String, bool)>,
    pub history: Vec<String>,
    pub history_at: Option<usize>,
    pub stars: Starfield,
    pub quit: bool,
    /// Nav's one-line prompt: 'w' (wake a seat) or 'g' (go to a workspace).
    pub prompt: Option<char>,
    /// Nav in two columns (--half): the tiled half-window's layout.
    pub half: bool,
    /// Seconds left of the warp jump's burst (a note to all, or a merge).
    pub jump_left: f64,
    /// The jump trigger this App has already fired, so it jumps once.
    pub seen_trigger: f64,
}

/// How long a warp jump's burst lasts, and the speed it adds to warp().
pub const JUMP_SECS: f64 = 2.5;
pub const JUMP_BOOST: f64 = 4.0;

impl App {
    pub fn new(view: View, snap: Snapshot) -> Self {
        let mut stars = Starfield::new(260, 0x5eed);
        stars.set_split(&snap.context_split(), true);
        App {
            view,
            snap,
            input: String::new(),
            cursor: 0,
            scroll: 0,
            flash: None,
            history: Vec::new(),
            history_at: None,
            stars,
            quit: false,
            prompt: None,
            half: false,
            jump_left: 0.0,
            seen_trigger: 0.0,
        }
    }

    pub fn refresh(&mut self, snap: Snapshot) {
        self.stars.set_split(&snap.context_split(), false);
        // A fresh broadcast (or merge note) fires the warp jump: once, then
        // the burst runs out on its own.
        if let Some(t) = snap.jump_trigger {
            if t > self.seen_trigger {
                self.seen_trigger = t;
                self.jump_left = JUMP_SECS;
            }
        }
        self.snap = snap;
    }

    /// One frame older: the jump burst counts down here.
    pub fn tick(&mut self, dt: f64) {
        if self.jump_left > 0.0 {
            self.jump_left = (self.jump_left - dt).max(0.0);
        }
    }

    /// Warp factor from the deck's traffic: nav-deck's warp_factor, plus the
    /// warp jump's burst while it lasts (a decay, not a step, so the stars
    /// surge and fade instead of snapping).
    pub fn warp(&self) -> f64 {
        let busy = self.snap.seats.iter().filter(|s| s.state == "busy").count();
        let waiting = self.snap.seats.iter().map(|s| s.waiting).sum();
        let base = crate::navdata::warp_factor(busy, waiting, &self.snap.nav.hours, self.snap.nav.load);
        if self.jump_left > 0.0 {
            (base + JUMP_BOOST * (self.jump_left / JUMP_SECS)).min(9.9)
        } else {
            base
        }
    }

    pub fn seat_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.snap.seats.iter().map(|s| s.name.clone()).collect();
        v.push("owner".into());
        v
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let [top, body, bottom] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(f.area());
    draw_strip(f, top, app);
    match app.view {
        View::Comms => comms::draw(f, body, app),
        View::Nav => {
            if app.half {
                nav::draw_half(f, body, app)
            } else {
                nav::draw(f, body, app)
            }
        }
    }
    draw_keys(f, bottom, app);
}

fn draw_strip(f: &mut Frame, area: Rect, app: &App) {
    let tab = |name: &str, on: bool| {
        if on {
            Span::styled(format!(" {name} "), Style::default().fg(Color::Black).bg(Color::Indexed(117)).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!(" {name} "), dim())
        }
    };
    let mut spans = vec![
        Span::styled(" ◆ DECK ", Style::default().fg(Color::Indexed(117)).add_modifier(Modifier::BOLD)),
        tab("COMMS", app.view == View::Comms),
        tab("NAV", app.view == View::Nav),
        Span::raw("   "),
    ];
    for s in &app.snap.seats {
        let (mark, colour) = theme::state_mark(&s.shown);
        spans.push(Span::styled(mark, Style::default().fg(colour)));
        spans.push(Span::styled(format!("{} ", display_name(&s.name)), Style::default().fg(theme::seat_colour(&s.name))));
    }
    let mem = app.snap.memory.available_pct();
    spans.push(Span::styled(format!("  mem {mem:.0}% free"), Style::default().fg(theme::meter_colour(100.0 - mem))));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_keys(f: &mut Frame, area: Rect, app: &App) {
    if app.view == View::Nav {
        if let Some(p) = app.prompt {
            let label = if p == 'w' { "wake" } else { "goto" };
            let line = Line::from(vec![
                Span::styled(format!(" {label} › "), Style::default().fg(Color::Indexed(117))),
                Span::raw(app.input.clone()),
            ]);
            f.render_widget(Paragraph::new(line), area);
            return;
        }
    }
    let text = match &app.flash {
        Some((msg, ok)) => Line::from(Span::styled(
            format!(" {msg}"),
            Style::default().fg(if *ok { Color::Indexed(114) } else { Color::Indexed(203) }),
        )),
        None => Line::from(Span::styled(
            match app.view {
                View::Comms => " Enter send · @seat to address · PgUp/PgDn scroll · Tab nav · Ctrl+C quit",
                View::Nav => match app.prompt {
                    Some('w') => " wake which seat? (the deck's rules apply) · Enter · Esc",
                    Some(_) => " go to which workspace? (comms, nav, mail, 3..0, or a name) · Enter · Esc",
                    None => " w <seat> wake · g <ws> goto · c / ` / Tab comms · m mail deck · Ctrl+C quit",
                },
            },
            dim(),
        )),
    };
    f.render_widget(Paragraph::new(text), area);
}
