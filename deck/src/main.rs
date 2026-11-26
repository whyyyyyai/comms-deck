//! deck: the command deck in one program. Comms and nav are two views of
//! one screen. A client of the comms-deck service: it reads the service's
//! files and sends only through `comms-deck send`. No AI.
//!
//!   deck                  live, full screen (Tab switches views)
//!   deck --view nav       start on the nav view
//!   deck --view nav --half   the nav view in two columns (the half window)
//!   deck --once [--view nav] [--size WxH]   one frame as text, then exit
//!   deck --self-test      render fake data, check the contracts, touch nothing real
//!
//! Written by: Claude (2026-11-26), at the owner's ask.

mod data;
mod navdata;
mod scrub;
mod send;
mod ui;

use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;

use ui::{App, View, JUMP_SECS};

enum Msg {
    Snap(data::Snapshot),
    Sent(Result<String, String>),
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let view = match value("--view").as_deref() {
        Some("nav") => View::Nav,
        _ => View::Comms,
    };
    // --half: the nav view in two columns, for the tiled half-window
    let half = flag("--half");
    if flag("--help") || flag("-h") {
        println!("deck [--view comms|nav] [--half] [--once [--size WxH]] [--self-test]");
        return;
    }
    if flag("--self-test") {
        std::process::exit(if self_test() { 0 } else { 1 });
    }
    if flag("--once") {
        let (w, h) = value("--size")
            .and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().ok(), b.parse().ok())))
            .and_then(|(a, b)| Some((a?, b?)))
            .unwrap_or((160, 48));
        let mut app = App::new(view, data::load());
        app.half = half;
        print!("{}", render_text(&app, w, h));
        return;
    }
    if let Err(e) = run(view, half) {
        eprintln!("deck: {e}");
        std::process::exit(1);
    }
}

/// One frame as plain text: for --once and the self-test.
fn render_text(app: &App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("test backend");
    term.draw(|f| ui::draw(f, app)).expect("draw");
    let buf = term.backend().buffer();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn run(view: View, half: bool) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    // Data comes in on its own thread, so typing never waits on a file or
    // on `comms-deck status`.
    let loader = tx.clone();
    thread::spawn(move || loop {
        if loader.send(Msg::Snap(data::load())).is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(1500));
    });

    enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let mut term = Terminal::new(CrosstermBackend::new(out))?;
    // Restore the terminal even if a panel panics.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        hook(info);
    }));

    let mut app = App::new(view, data::Snapshot { time: data::now(), ..Default::default() });
    app.half = half;
    let mut last = Instant::now();
    let mut memory_at = 0.0;
    while !app.quit {
        let t = data::now();
        app.now = t;
        if t - memory_at >= 5.0 && app.snap.memory.available_kb > 0 {
            memory_at = t;
            navdata::record_memory(app.snap.memory.available_kb * 1024, t);
        }
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::Snap(s) => app.refresh(s),
                Msg::Sent(r) => {
                    app.flash = Some(match r {
                        Ok(say) => (if say.is_empty() { "sent".into() } else { say }, true),
                        Err(e) => (e, false),
                    })
                }
            }
        }
        let dt = last.elapsed().as_secs_f64();
        last = Instant::now();
        app.tick(dt);
        if app.view == View::Nav {
            let w = app.warp();
            app.stars.step(dt, 0.12 * w);
        }
        term.draw(|f| ui::draw(f, &app))?;
        // The nav view animates at ~30 fps; comms redraws on input or data.
        let wait = if app.view == View::Nav { 33 } else { 250 };
        if event::poll(Duration::from_millis(wait))? {
            match event::read()? {
                Event::Key(k) => on_key(&mut app, k, &tx),
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::Down(MouseButton::Left) if app.view == View::Nav => on_core_click(&mut app, m.column, m.row),
                    MouseEventKind::ScrollUp => app.scroll += 3,
                    MouseEventKind::ScrollDown => app.scroll = app.scroll.saturating_sub(3),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    term.show_cursor()?;
    Ok(())
}

fn on_key(app: &mut App, k: KeyEvent, tx: &mpsc::Sender<Msg>) {
    if k.kind != event::KeyEventKind::Press {
        return;
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Char('c') | KeyCode::Char('q') if ctrl => app.quit = true,
        KeyCode::Tab => app.view = if app.view == View::Comms { View::Nav } else { View::Comms },
        KeyCode::F(1) => app.view = View::Comms,
        KeyCode::F(2) => app.view = View::Nav,
        // Nav: the chat box in 05 has the keyboard only while writing, so
        // nav's one-letter keys still work the rest of the time.
        KeyCode::Esc if app.view == View::Nav && app.typing => app.typing = false,
        _ if app.view == View::Nav && !app.typing => on_nav_key(app, k),
        KeyCode::Enter => submit(app, tx),
        KeyCode::Char(c) => {
            let at = byte_at(&app.input, app.cursor);
            app.input.insert(at, c);
            app.cursor += 1;
            app.flash = None;
        }
        KeyCode::Backspace if app.cursor > 0 => {
            app.cursor -= 1;
            let at = byte_at(&app.input, app.cursor);
            app.input.remove(at);
        }
        KeyCode::Delete if app.cursor < app.input.chars().count() => {
            let at = byte_at(&app.input, app.cursor);
            app.input.remove(at);
        }
        KeyCode::Left => app.cursor = app.cursor.saturating_sub(1),
        KeyCode::Right => app.cursor = (app.cursor + 1).min(app.input.chars().count()),
        KeyCode::Home => app.cursor = 0,
        KeyCode::End => app.cursor = app.input.chars().count(),
        KeyCode::Up => recall(app, true),
        KeyCode::Down => recall(app, false),
        KeyCode::PageUp => app.scroll += 10,
        KeyCode::PageDown => app.scroll = app.scroll.saturating_sub(10),
        KeyCode::Esc => {
            app.input.clear();
            app.cursor = 0;
            app.scroll = 0;
        }
        _ => {}
    }
}

/// A click in the warp core picks the seat under it; a second click on the
/// same seat shows its window. A click on empty space lets go.
fn on_core_click(app: &mut App, col: u16, row: u16) {
    let pos = ratatui::layout::Position { x: col, y: row };
    // A click on 05 COMMS starts writing; anywhere else stops.
    if !app.zoom && app.chat_area.get().contains(pos) {
        app.typing = true;
        return;
    }
    app.typing = false;
    let area = app.core_area.get();
    if !area.contains(ratatui::layout::Position { x: col, y: row }) {
        return;
    }
    let hit = ui::warp::hit(&app.stars, &app.contacts(), area, col, row);
    match hit {
        Some(seat) if app.selected.as_deref() == Some(seat.as_str()) => {
            app.flash = Some(send::focus_seat(&seat).map_or_else(|e| (e, false), |m| (m, true)));
        }
        Some(seat) => {
            app.selected = Some(seat);
            app.flash = None;
        }
        None => app.selected = None,
    }
}

/// Nav's keys: the same ones nav-deck has. Nothing here sends a note.
fn on_nav_key(app: &mut App, k: KeyEvent) {
    if let Some(p) = app.prompt {
        match k.code {
            KeyCode::Esc => {
                app.prompt = None;
                app.input.clear();
            }
            KeyCode::Enter => {
                let arg = app.input.trim().to_lowercase();
                app.prompt = None;
                app.input.clear();
                if arg.is_empty() {
                    return;
                }
                app.flash = Some(if p == 'w' { send::wake(&arg) } else { send::goto(&arg) }.map_or_else(|e| (e, false), |m| (m, true)));
            }
            KeyCode::Backspace => {
                app.input.pop();
            }
            KeyCode::Char(c) => app.input.push(c),
            _ => {}
        }
        return;
    }
    match k.code {
        KeyCode::Char('`') | KeyCode::Char('c') => app.view = View::Comms,
        KeyCode::Char('z') => app.zoom = !app.zoom,
        // Enter writes in 05 COMMS; with a seat picked, the note starts @seat.
        KeyCode::Enter => {
            app.typing = true;
            app.zoom = false;
            app.flash = None;
            if let (Some(seat), true) = (app.selected.clone(), app.input.is_empty()) {
                app.input = format!("@{seat} ");
                app.cursor = app.input.chars().count();
            }
        }
        KeyCode::Esc => {
            app.selected = None;
            app.flash = None;
        }
        KeyCode::Char('w') if app.selected.is_some() => {
            let seat = app.selected.clone().unwrap_or_default();
            app.flash = Some(send::wake(&seat).map_or_else(|e| (e, false), |m| (m, true)));
        }
        KeyCode::Char('w') | KeyCode::Char('g') => {
            if let KeyCode::Char(c) = k.code {
                app.prompt = Some(c);
                app.input.clear();
                app.flash = None;
            }
        }
        KeyCode::Char('m') | KeyCode::Char('2') => {
            app.flash = send::goto("mail").err().map(|e| (e, false));
        }
        _ => {}
    }
}

fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(s.len())
}

fn recall(app: &mut App, older: bool) {
    if app.history.is_empty() {
        return;
    }
    let n = app.history.len();
    let at = match (app.history_at, older) {
        (None, true) => n - 1,
        (None, false) => return,
        (Some(i), true) => i.saturating_sub(1),
        (Some(i), false) if i + 1 < n => i + 1,
        (Some(_), false) => {
            app.history_at = None;
            app.input.clear();
            app.cursor = 0;
            return;
        }
    };
    app.history_at = Some(at);
    app.input = app.history[at].clone();
    app.cursor = app.input.chars().count();
}

fn submit(app: &mut App, tx: &mpsc::Sender<Msg>) {
    let text = app.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    let to = match send::route(&text, &app.seat_names()) {
        Ok(t) => t,
        Err(e) => {
            app.flash = Some((e, false));
            return;
        }
    };
    app.history.push(text.clone());
    app.history_at = None;
    app.input.clear();
    app.cursor = 0;
    app.scroll = 0;
    app.flash = Some((format!("sending to {to}…"), true));
    let tx = tx.clone();
    thread::spawn(move || {
        let _ = tx.send(Msg::Sent(send::send(&to, &text)));
    });
}

/// Draw both views from a fake tree and check what the screen promises.
/// Touches nothing real: no service, no files outside a temp folder.
fn self_test() -> bool {
    use serde_json::json;
    let mut ok = true;
    let mut check = |name: &str, pass: bool, detail: &str| {
        println!("{} {name}", if pass { "ok  " } else { "FAIL" });
        if !pass {
            println!("     {detail}");
            ok = false;
        }
    };
    let status = json!({"seats": {
        "claude": {"kind":"agent","state":"busy","shown":"busy","rank":[1,0],"waiting":0,"since_s":40},
        "grok": {"kind":"agent","state":"idle","shown":"ready","rank":[0,0],"waiting":3,"since_s":200},
        "codex": {"kind":"agent","state":"away","shown":"spent","rank":[7,90],"waiting":90,"spent_until": 1.0e12}
    }});
    let mut snap = data::Snapshot { time: data::now(), seats: data::parse_status(&status), ..Default::default() };
    snap.notes.push(data::Note {
        from: "owner".into(),
        to: "claude".into(),
        time: "2026-11-26 16:16:48".into(),
        text: "make it one program \u{1b}]52;c;aGk=\u{7} in rust".into(),
        quiet: false,
    });
    for n in snap.notes.iter_mut() {
        n.text = scrub::scrub(&n.text);
    }
    snap.usage.insert(
        "claude".into(),
        data::parse_usage(&json!({"model":{"display_name":"Opus 5.5"},
            "context_window":{"used_percentage":30,"context_window_size":1000000}})),
    );
    snap.usage.insert(
        "grok".into(),
        data::parse_usage(&json!({"context_window":{"context_tokens":100000}})),
    );
    snap.memory = data::Memory { total_kb: 8_000_000, available_kb: 4_000_000, swap_total_kb: 4_000_000, swap_free_kb: 4_000_000 };

    let app = App::new(View::Comms, snap.clone());
    let comms = render_text(&app, 160, 48);
    check("comms view shows the note, with the owner as the sender", comms.contains("Y → claude") && comms.contains("make it one program"), &comms);
    check("no escape character reaches the screen", !comms.contains('\u{1b}') && !comms.contains('\u{7}'), "escape found");
    check("the crew is in the service's ready order (grok before claude)",
        comms.find("grok").zip(comms.find("claude ")).is_some_and(|(g, c)| g < c), &comms);
    check("a spent seat shows when it's back", comms.contains("back in"), &comms);
    check("usage shows a context bar", comms.contains("ctx") && comms.contains("30%"), &comms);

    let mut app = App::new(View::Nav, snap);
    let nav = render_text(&app, 160, 48);
    for panel in ["01 CREW", "02 ACTIVITY", "03 USAGE", "04 WARP CORE", "05 COMMS", "06 WORKSPACES",
                  "07 VITALS", "08 NAV-TIME", "09 DECK PULSE", "10 CONTEXT SPLIT"] {
        check(&format!("nav view at 160x48 shows {panel}"), nav.contains(panel), &nav);
    }
    let big = render_text(&app, 209, 50);
    check("nav at the owner's 209x50 shows every panel title",
        ["03 USAGE", "05 COMMS", "06 WORKSPACES", "07 VITALS", "10 CONTEXT SPLIT", "08 NAV-TIME", "09 DECK PULSE"].iter().all(|p| big.contains(p)), &big);
    let total = app.stars.stars.len();
    let claude = app.stars.count_for("claude");
    check("stars are dealt by the context split (claude 300k : grok 100k = 3:1)",
        claude * 4 == total * 3 && app.stars.count_for("grok") * 4 == total, &format!("{claude} of {total}"));
    check("a seat with notes waiting is a contact (G3)", nav.contains("G3"), &nav);
    check("a spent seat is not a contact", !nav.contains("C90"), &nav);
    app.stars.step(0.5, 1.0);
    let small = render_text(&app, 80, 24);
    check("a small window still draws both strips", small.contains("DECK") && small.lines().count() == 24, &small);
    // Clicks: the G3 contact picks grok, the info line says so, Esc lets go.
    let _ = render_text(&app, 160, 48);
    let core = app.core_area.get();
    let cell = (core.y..core.bottom())
        .flat_map(|y| (core.x..core.right()).map(move |x| (x, y)))
        .find(|&(x, y)| ui::warp::hit(&app.stars, &app.contacts(), core, x, y).as_deref() == Some("grok"));
    check("a grok star or contact can be clicked", cell.is_some(), "no grok cell in the core");
    if let Some((x, y)) = cell {
        on_core_click(&mut app, x, y);
    }
    let picked = render_text(&app, 160, 48);
    check("a click picks the seat and the key line describes it",
        app.selected.as_deref() == Some("grok") && picked.contains("◉ grok · ready · ✉3"), &picked);
    app.zoom = true;
    let zoomed = render_text(&app, 160, 48);
    check("z zooms the warp core over the whole screen, seats round its edge",
        zoomed.contains("zoomed") && !zoomed.contains("03 USAGE") && zoomed.contains("claude busy") && zoomed.contains("grok ready ✉3"), &zoomed);
    let none = (core.y..core.bottom()).flat_map(|y| (core.x..core.right()).map(move |x| (x, y)))
        .find(|&(x, y)| ui::warp::hit(&app.stars, &app.contacts(), app.core_area.get(), x, y).is_none());
    if let Some((x, y)) = none {
        on_core_click(&mut app, x, y);
    }
    check("a click on empty space lets the seat go", app.selected.is_none(), "still selected");
    app.zoom = false;
    // The half view: same ten panels in two columns, at a half-window's size.
    app.half = true;
    let half = render_text(&app, 110, 30);
    for panel in ["01 CREW", "02 ACTIVITY", "03 USAGE", "04 WARP CORE", "05 COMMS", "06 WORKSPACES",
                  "07 VITALS", "08 NAV-TIME", "09 DECK PULSE", "10 CONTEXT SPLIT"] {
        check(&format!("half nav at 110x30 shows {panel}"), half.contains(panel), &half);
    }
    check("half nav is two columns (the core shares the left with the crew)",
        half.contains("04 WARP CORE") && half.find("04 WARP CORE").zip(half.find("01 CREW")).is_some_and(|(c, k)| {
            let (line_c, line_k) = (half[..c].lines().count(), half[..k].lines().count());
            line_k > line_c
        }), &half);
    // One deck: nav carries the chat (05) and the context split (10).
    app.half = false;
    app.zoom = false;
    app.selected = None;
    let one = render_text(&app, 209, 50);
    check("nav's 05 COMMS shows the conversation", one.contains("05 COMMS") && one.contains("make it one program"), &one);
    check("nav's 10 CONTEXT SPLIT shows the dots and the shares (claude 75%)",
        one.contains("10 CONTEXT SPLIT") && one.contains('●') && one.contains("75.0%"), &one);
    on_nav_key(&mut app, event::KeyEvent::from(KeyCode::Enter));
    check("Enter in nav starts writing in 05", app.typing, "not typing");
    for c in "hi".chars() {
        let (tx, _rx) = mpsc::channel::<Msg>();
        on_key(&mut app, event::KeyEvent::from(KeyCode::Char(c)), &tx);
    }
    check("while writing, letters go to the chat box, not nav's keys", app.input == "hi" && !app.zoom && app.prompt.is_none(), &app.input);
    let (tx, _rx) = mpsc::channel::<Msg>();
    on_key(&mut app, event::KeyEvent::from(KeyCode::Esc), &tx);
    check("Esc stops writing and keeps the draft", !app.typing && app.input == "hi", &app.input);
    app.input.clear();
    app.cursor = 0;
    let routed = send::route("@grok -claude hi", &["grok".into(), "claude".into()]);
    check("chat box routing matches comms-deck route()", routed.as_deref() == Ok("grok -claude"), &format!("{routed:?}"));

    // The warp jump: a fresh note to all (or a merge note) bursts the speed,
    // once, then fades. An old broadcast fires nothing.
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let all_note = |time: &str, to: &str, text: &str| data::Note {
        from: "owner".into(),
        to: to.into(),
        time: time.into(),
        text: text.into(),
        quiet: false,
    };
    let mut fresh = data::Snapshot { time: data::now(), ..Default::default() };
    fresh.notes = vec![all_note(&stamp, "all", "bring everyone back online")];
    check("a fresh note to all is a jump trigger", fresh.newest_jump_note(120.0).is_some(), "no trigger");
    let mut old = data::Snapshot { time: data::now() + 3600.0, ..Default::default() };
    old.notes = vec![all_note(&stamp, "all", "bring everyone back online")];
    check("an hour-old broadcast is not a jump trigger", old.newest_jump_note(120.0).is_none(), "fired anyway");
    let mut merge = data::Snapshot { time: data::now(), ..Default::default() };
    merge.notes = vec![all_note(&stamp, "claude", "merged #144, thanks for the review")];
    check("a merge note is a jump trigger too", merge.newest_jump_note(120.0).is_some(), "no trigger");
    let mut no_merge = data::Snapshot { time: data::now(), ..Default::default() };
    no_merge.notes = vec![all_note(&stamp, "claude", "not merged #144 yet, waiting on Y")];
    check("\"not merged #n\" is not a jump trigger", no_merge.newest_jump_note(120.0).is_none(), "fired anyway");
    let mut japp = App::new(View::Nav, data::Snapshot { time: data::now(), ..Default::default() });
    let base = japp.warp();
    japp.refresh(data::Snapshot { jump_trigger: Some(data::now()), ..Default::default() });
    check("the warp jump bursts above the traffic's warp", japp.jump_left > 0.0 && japp.warp() > base, &format!("{} vs {base}", japp.warp()));
    japp.tick(JUMP_SECS);
    check("the burst fades when its timer runs out", japp.jump_left == 0.0 && japp.warp() == base, &format!("{}", japp.warp()));
    japp.refresh(data::Snapshot { jump_trigger: Some(data::now()), ..Default::default() });
    let fired = japp.jump_left;
    japp.refresh(data::Snapshot { jump_trigger: Some(japp.seen_trigger), ..Default::default() });
    check("the same broadcast doesn't fire twice", japp.jump_left == fired, &format!("{} then {}", fired, japp.jump_left));
    println!("{}", if ok { "all passed" } else { "some failed" });
    ok
}
