//! The left panel (t87, Y: "Let's start building the left panel in the
//! warp core. The right panel is the comms, activity, and notify.").
//! It mirrors the chat panel on the right: same background, same header,
//! tabs on the next function keys.
//!
//!   F4 SYSTEM  the machine: gpu frames, ram, swap, power, context (the stats
//!              that floated at the top left), plus the internet (netwatch)
//!   F5 TASKS   the task board (it floated at the bottom left)
//!   F6 CREW    one row per seat: state, waiting notes, oldest wait,
//!              context, the five-hour and week limits, what it's doing
//!
//! Written by: claude

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeftTab {
    #[default]
    System,
    Tasks,
    Crew,
}

impl LeftTab {
    pub const ALL: [LeftTab; 3] = [LeftTab::System, LeftTab::Tasks, LeftTab::Crew];
    fn name(self) -> &'static str {
        match self {
            LeftTab::System => "SYSTEM",
            LeftTab::Tasks => "TASKS",
            LeftTab::Crew => "CREW",
        }
    }
}

#[derive(Resource)]
pub struct Left {
    pub tab: LeftTab,
}

impl Default for Left {
    /// The tab Y was last on: a reinstall restarts the warp core, and it used
    /// to come back on F4 every time (Y couldn't see TASKS on screenshots).
    fn default() -> Self {
        let tab = std::fs::read_to_string(tab_file())
            .ok()
            .and_then(|t| LeftTab::ALL.iter().copied().find(|x| x.name() == t.trim()))
            .unwrap_or_default();
        Left { tab }
    }
}

fn tab_file() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state/warp-core/left-tab")
}

/// The panel's width for a window, or 0 when it's off.
pub fn left_width(window_w: f32, cfg: &mods::LeftCfg) -> f32 {
    if !cfg.show {
        return 0.0;
    }
    (window_w * cfg.width).clamp(260.0, 520.0).min(window_w * 0.4)
}

#[derive(Component)]
pub struct LeftBg;
#[derive(Component)]
pub struct LeftHeader;
#[derive(Component)]
pub struct LeftBody;

/// The header: the three tabs, the current one bracketed.
pub fn header_text(tab: LeftTab) -> String {
    LeftTab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| if *t == tab { format!("[F{} {}]", i + 4, t.name()) } else { format!(" F{} {} ", i + 4, t.name()) })
        .collect::<Vec<_>>()
        .join(" ")
}

fn pct(v: Option<f64>) -> String {
    v.map(|x| format!("{:>3.0}%", x)).unwrap_or_else(|| "   -".into())
}

fn token_count(tokens: f64) -> String {
    if tokens >= 1_000_000.0 {
        format!("{:.1}M", tokens / 1_000_000.0)
    } else if tokens >= 1_000.0 {
        format!("{:.1}k", tokens / 1_000.0)
    } else {
        format!("{tokens:.0}")
    }
}

fn context_line(seat: &SeatInfo) -> String {
    let mut line = "Context".to_owned();
    if let Some(used) = seat.ctx_tokens {
        line.push(' ');
        line.push_str(&token_count(used));
        if let Some(capacity) = seat.ctx_capacity {
            line.push('/');
            line.push_str(&token_count(capacity));
        }
        line.push_str(" tokens");
    } else if let Some(capacity) = seat.ctx_capacity {
        line.push_str(&format!(" capacity {} tokens", token_count(capacity)));
    }
    if let Some(percent) = seat.ctx {
        line.push_str(&format!(" | {percent:.0}%"));
    } else if seat.ctx_tokens.is_none() && seat.ctx_capacity.is_none() {
        line.push_str(" unavailable");
    }
    line
}

/// The crew tab: one block per seat, in the ring's order.
pub fn crew_text(seats: &[SeatInfo], tasks: &[feed::TaskInfo]) -> String {
    let mut s = String::from("CREW\n\n");
    if seats.is_empty() {
        s.push_str("waiting for the deck\n");
        return s;
    }
    for seat in seats {
        let held: Vec<&str> = tasks.iter().filter(|t| t.holder.as_deref() == Some(seat.name.as_str())).map(|t| t.id.as_str()).collect();
        s.push_str(&format!("{:<8} {}\n", label_for(&seat.name), seat.shown));
        s.push_str(&format!("         {}\n", context_line(seat)));
        let wait = if seat.waiting > 0 { format!("{} waiting, oldest {}", seat.waiting, ago(seat.oldest_waiting_s)) } else { "no notes waiting".into() };
        s.push_str(&format!("         {wait}\n"));
        if seat.five_hour.is_some() || seat.week.is_some() {
            s.push_str(&format!("         5h {}  wk {}\n", pct(seat.five_hour), pct(seat.week)));
        }
        if !held.is_empty() {
            s.push_str(&format!("         holds {}\n", held.join(", ")));
        }
        s.push('\n');
    }
    s
}

/// A task's title as a person would write it (t87, Y: "it should be human
/// readable"): the board keeps the first words of Y's chat line, so drop the
/// lead-in ("Right,", "can you", "we need a", "let's",
/// "@claude") and start with a capital.
pub fn clean_title(raw: &str) -> String {
    let mut title = raw.trim();
    let fillers = [
        "right,", "can you please", "could you please",
        "can you", "could you", "can we", "we need an", "we need a",
        "we need to", "let's", "let us", "please",
    ];
    loop {
        let before = title;
        if title.starts_with('@') {
            title = title.split_once(char::is_whitespace).map_or(title, |(_, rest)| rest.trim_start());
        }
        for filler in fillers {
            let Some(prefix) = title.get(..filler.len()) else { continue };
            let rest = &title[filler.len()..];
            if prefix.eq_ignore_ascii_case(filler) && rest.starts_with(char::is_whitespace) {
                title = rest.trim_start();
                break;
            }
        }
        if title == before {
            break;
        }
    }
    let mut chars = title.chars();
    chars.next().map_or_else(String::new, |first| first.to_uppercase().collect::<String>() + chars.as_str())
}

/// Wrap at word boundaries and split any token wider than the available line.
pub fn wrap(text: &str, cols: usize, max_lines: usize) -> Vec<String> {
    if cols == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut width = 0;
    for word in text.split_whitespace() {
        let word_width = word.chars().count();
        if width > 0 && width + 1 + word_width > cols {
            lines.push(std::mem::take(&mut line));
            width = 0;
        }
        if width > 0 {
            line.push(' ');
            width += 1;
        }
        for ch in word.chars() {
            if width == cols {
                lines.push(std::mem::take(&mut line));
                width = 0;
            }
            line.push(ch);
            width += 1;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        let last = &mut lines[max_lines - 1];
        if last.chars().count() == cols {
            last.pop();
        }
        if !last.ends_with('…') {
            last.push('…');
        }
    }
    lines
}

/// How long a task has been held, as a colour: dim while fresh, amber past
/// 30 minutes, red past two hours.
pub fn age_colour(secs: f64) -> [f32; 3] {
    if secs >= 7200.0 {
        [1.0, 0.38, 0.32]
    } else if secs >= 1800.0 {
        [1.0, 0.72, 0.3]
    } else {
        [0.55, 0.62, 0.75]
    }
}

/// The TASKS tab as coloured runs: a summary line, then the open tasks (they
/// need someone), then each holder's tasks under its name in its colour.
/// Each task leads with its title, followed by the ID and assignment age.
pub fn task_spans(tasks: &[feed::TaskInfo], now: f64, cols: usize) -> Vec<(String, Color)> {
    let dim = Color::srgb(0.5, 0.58, 0.72);
    let text = Color::srgb(0.82, 0.86, 0.94);
    let mut out: Vec<(String, Color)> = Vec::new();
    let age = |t: &feed::TaskInfo| t.since.map(|s| (now - s).max(0.0)).unwrap_or(0.0);
    let open: Vec<&feed::TaskInfo> = tasks.iter().filter(|t| t.holder.is_none()).collect();
    let oldest = tasks.iter().map(age).fold(0.0f64, f64::max);
    if tasks.is_empty() {
        out.push(("TASKS\n\nNothing on the board.\n".into(), dim));
        return out;
    }
    out.push((
        format!("TASKS   {} on the board · {} open · oldest {}\n", tasks.len(), open.len(), ago(oldest)),
        dim,
    ));
    let card = |out: &mut Vec<(String, Color)>, t: &feed::TaskInfo| {
        let a = age(t);
        out.push(("\n".into(), dim));
        let title = t.title.as_deref().map(clean_title).unwrap_or_else(|| "Private task".into());
        for line in wrap(&title, cols.saturating_sub(2), 2) {
            out.push((format!("  {line}\n"), text));
        }
        let age = t.since.map_or_else(|| "age unavailable".into(), |_| {
            format!("{} {}", if t.holder.is_none() { "Waiting" } else { "Held" }, ago(a))
        });
        out.push((format!("  {} · {}\n", t.id, age), lin(age_colour(a), 1.0)));
    };
    if !open.is_empty() {
        out.push((format!("\nOPEN · needs someone · {}\n", open.len()), Color::srgb(1.0, 0.72, 0.3)));
        for t in &open {
            card(&mut out, t);
        }
    }
    let mut holders: Vec<&str> = tasks.iter().filter_map(|t| t.holder.as_deref()).collect();
    holders.sort();
    holders.dedup();
    for h in holders {
        let mine: Vec<&feed::TaskInfo> = tasks.iter().filter(|t| t.holder.as_deref() == Some(h)).collect();
        out.push((format!("\n{} · {}\n", label_for(h).to_uppercase(), mine.len()), seat_colour(h, 1.3)));
        for t in mine {
            card(&mut out, t);
        }
    }
    out
}

/// The TASKS tab's text root: its coloured runs are child spans.
#[derive(Component)]
pub struct LeftTasks;

type Row = (String, Color);

#[derive(Default)]
pub struct LeftRows {
    tasks: Vec<feed::TaskInfo>,
    seats: Vec<SeatInfo>,
    source: Option<(LeftTab, usize, f32, f32)>,
    rows: Vec<Row>,
    revision: u64,
    drawn: Option<(u64, usize, usize)>,
    scroll: [usize; 2],
}

fn visible_rows(total: usize, requested: usize, capacity: usize) -> (usize, usize, bool) {
    let overflow = total > capacity && capacity > 1;
    let count = capacity.saturating_sub(usize::from(overflow)).min(total);
    let first = requested.min(total.saturating_sub(count));
    (first, first + count, overflow)
}

fn compact_header(tab: LeftTab, cols: usize) -> String {
    let full = header_text(tab);
    if full.chars().count() <= cols {
        return full;
    }
    let short = match tab {
        LeftTab::System => "[F4 SYSTEM] F5 F6",
        LeftTab::Tasks => "F4 [F5 TASKS] F6",
        LeftTab::Crew => "F4 F5 [F6 CREW]",
    };
    wrap(short, cols, 1).pop().unwrap_or_default()
}

fn panel_rows(tab: LeftTab, deck: &Deck, unix: f64, cols: usize) -> Vec<Row> {
    let spans = match tab {
        LeftTab::Tasks => task_spans(&deck.tasks, unix, cols),
        LeftTab::Crew => vec![(crew_text(&deck.seats, &deck.tasks), Color::srgb(0.72, 0.79, 0.89))],
        LeftTab::System => Vec::new(),
    };
    let mut rows = Vec::new();
    for (text, colour) in spans {
        for line in text.lines() {
            if line.chars().count() <= cols {
                rows.push((line.to_owned(), colour));
            } else {
                rows.extend(wrap(line, cols, usize::MAX).into_iter().map(|line| (line, colour)));
            }
        }
    }
    rows
}

/// netwatch's last word (~/.local/state/netwatch/status.json), as a line.
pub fn net_line(status: Option<&serde_json::Value>, now_unix: f64) -> String {
    let Some(v) = status else { return "net    no netwatch status".into() };
    let age = now_unix - v.get("time").and_then(|t| t.as_f64()).unwrap_or(0.0);
    if age > 120.0 {
        return format!("net    netwatch silent for {}", ago(age));
    }
    match v.get("online").and_then(|o| o.as_bool()) {
        Some(true) => "net    online".into(),
        Some(false) => {
            let since = v.get("down_since").and_then(|d| d.as_f64()).map(|d| ago(now_unix - d)).unwrap_or_else(|| "?".into());
            let step = v.get("last_step").and_then(|s| s.as_str()).unwrap_or("-");
            format!("net    DOWN for {since}  !\n       last try: {step}")
        }
        None => "net    checking".into(),
    }
}

fn read_net() -> Option<serde_json::Value> {
    let home = std::env::var("HOME").ok()?;
    let text = std::fs::read_to_string(format!("{home}/.local/state/netwatch/status.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// F4-F6 pick the tab (F1-F3 stay the chat's).
pub fn left_keys(keys: Res<ButtonInput<KeyCode>>, mut left: ResMut<Left>) {
    for (k, tab) in [(KeyCode::F4, LeftTab::System), (KeyCode::F5, LeftTab::Tasks), (KeyCode::F6, LeftTab::Crew)] {
        if keys.just_pressed(k) && left.tab != tab {
            left.tab = tab;
            let f = tab_file();
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(f, tab.name());
        }
    }
}

/// Lay out and fill the panel each frame.
#[allow(clippy::too_many_arguments)]
pub fn left_panel(
    mut commands: Commands,
    left: Res<Left>,
    layout: Res<Layout>,
    deck: Res<Deck>,
    mods: Res<Mods>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    real: Res<Time<Real>>,
    mut bg: Query<(&mut Transform, &mut Visibility), (With<LeftBg>, Without<LeftHeader>, Without<LeftBody>)>,
    mut header: Query<(&mut Text2d, &mut TextFont, &mut Transform, &mut Visibility), (With<LeftHeader>, Without<LeftBg>, Without<LeftBody>, Without<LeftTasks>)>,
    mut body: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<LeftBody>, Without<LeftBg>, Without<LeftHeader>, Without<LeftTasks>)>,
    mut tasks_text: Query<(Entity, &mut Transform, &mut Visibility), (With<LeftTasks>, Without<LeftBg>, Without<LeftHeader>, Without<LeftBody>)>,
    mut state: Local<(bool, f32, Option<serde_json::Value>)>,
    mut rows: Local<LeftRows>,
    windows: Query<&Window>,
    mut wheel: MessageReader<MouseWheel>,
) {
    let font = mods.cfg.left.font.max(1.0);
    if !state.0 {
        let panel_bg = materials.add(Color::srgb(0.028, 0.032, 0.058));
        commands.spawn((LeftBg, Mesh2d(assets.quad.clone()), MeshMaterial2d(panel_bg), Transform::from_xyz(0.0, 0.0, 7.0), Visibility::Hidden));
        commands.spawn((
            LeftHeader,
            Text2d::new(""),
            TextFont { font_size: FontSize::Px(font), ..default() },
            TextColor(Color::srgb(0.55, 0.65, 0.8)),
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 10.0),
            Visibility::Hidden,
        ));
        commands.spawn((
            LeftBody,
            Text2d::new(""),
            TextFont { font_size: FontSize::Px(font), ..default() },
            TextColor(Color::srgb(0.5, 0.6, 0.75)),
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 10.0),
            Visibility::Hidden,
        ));
        commands.spawn((
            LeftTasks,
            Text2d::new(""),
            TextFont { font_size: FontSize::Px(font), ..default() },
            TextColor(Color::srgb(0.5, 0.6, 0.75)),
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 10.0),
            Visibility::Hidden,
        ));
        state.0 = true;
    }
    let show = layout.left > 0.0;
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    let h = layout.half();
    let x0 = -h.x - layout.left; // the panel's left edge, in world
    for (mut tf, mut vis) in &mut bg {
        *vis = want;
        // Full window height: the chat strip only covers the central column.
        tf.translation.x = x0 + layout.left * 0.5;
        tf.translation.y = -layout.panel * 0.5;
        tf.scale = Vec3::new(layout.left, layout.scene.y + layout.panel, 1.0);
    }
    let cols = ((layout.left - 28.0) / (font * 0.8)).max(1.0) as usize;
    for (mut t, mut text_font, mut tf, mut vis) in &mut header {
        *vis = want;
        if !matches!(text_font.font_size, FontSize::Px(size) if size == font) {
            text_font.font_size = FontSize::Px(font);
        }
        let text = compact_header(left.tab, cols);
        let text = glyphs(&text);
        if t.0 != text {
            t.0 = text;
        }
        tf.translation.x = x0 + 14.0;
        tf.translation.y = h.y - 14.0;
    }
    // netwatch's file, read every 5 s.
    let now = real.elapsed_secs();
    if state.1 == 0.0 || now - state.1 > 5.0 {
        state.2 = read_net();
        state.1 = now.max(1e-3);
    }
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    for (mut t, mut tf, mut vis) in &mut body {
        let on_system = show && left.tab == LeftTab::System;
        *vis = if on_system { Visibility::Inherited } else { Visibility::Hidden };
        if on_system {
            let text = net_line(state.2.as_ref(), unix);
            let text = glyphs(&text);
            if t.0 != text {
                t.0 = text;
            }
        }
        tf.translation.x = x0 + 14.0;
        tf.translation.y = -h.y + 90.0;
    }

    let scroll_slot = match left.tab {
        LeftTab::Tasks => Some(0),
        LeftTab::Crew => Some(1),
        LeftTab::System => None,
    };
    let on_rows = show && scroll_slot.is_some();
    let over_left = windows.iter().next().and_then(Window::cursor_position)
        .is_some_and(|position| position.x >= 0.0 && position.x < layout.left);
    for ev in wheel.read() {
        if let Some(slot) = scroll_slot.filter(|_| on_rows && over_left) {
            if ev.y > 0.0 {
                rows.scroll[slot] = rows.scroll[slot].saturating_sub(3);
            } else if ev.y < 0.0 {
                rows.scroll[slot] = rows.scroll[slot].saturating_add(3);
            }
        }
    }

    for (entity, mut tf, mut vis) in &mut tasks_text {
        *vis = if on_rows { Visibility::Inherited } else { Visibility::Hidden };
        tf.translation.x = x0 + 14.0;
        tf.translation.y = h.y - 50.0;
        let Some(slot) = scroll_slot.filter(|_| on_rows) else { continue };
        let snapshot_changed = rows.tasks != deck.tasks
            || (left.tab == LeftTab::Crew && rows.seats != deck.seats);
        let source_changed = rows.source.is_none_or(|(tab, previous_cols, previous_font, last)| {
            tab != left.tab || previous_cols != cols || previous_font != font || now - last >= 30.0
        });
        if snapshot_changed || source_changed {
            rows.tasks.clone_from(&deck.tasks);
            if left.tab == LeftTab::Crew {
                rows.seats.clone_from(&deck.seats);
            }
            rows.rows = panel_rows(left.tab, &deck, unix, cols);
            rows.source = Some((left.tab, cols, font, now));
            rows.revision = rows.revision.wrapping_add(1);
        }
        let capacity = ((layout.scene.y + layout.panel - 64.0) / (font * 1.2)).max(0.0) as usize;
        let (first, end, overflow) = visible_rows(rows.rows.len(), rows.scroll[slot], capacity);
        rows.scroll[slot] = first;
        let draw_key = (rows.revision, first, capacity);
        if rows.drawn == Some(draw_key) {
            continue;
        }
        rows.drawn = Some(draw_key);
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).with_children(|parent| {
            for (text, colour) in &rows.rows[first..end] {
                parent.spawn((
                    TextSpan::new(glyphs(&format!("{text}\n"))),
                    TextColor(*colour),
                    TextFont { font_size: FontSize::Px(font), ..default() },
                ));
            }
            if overflow {
                let cue = format!("Rows {}–{} of {} · Scroll", first + 1, end, rows.rows.len());
                let cue = wrap(&cue, cols, 1).pop().unwrap_or_default();
                parent.spawn((
                    TextSpan::new(glyphs(&cue)),
                    TextColor(Color::srgb(0.55, 0.65, 0.8)),
                    TextFont { font_size: FontSize::Px(font), ..default() },
                ));
            }
        });
    }
}

/// Where the floating SYSTEM and TASKS texts go, and whether they show:
/// inside the panel on their tab when it's on, where they always were when off.
pub fn place(layout: &Layout, left: &Left, tab: LeftTab, old: Vec2) -> (Vec2, bool) {
    if layout.left <= 0.0 {
        return (old, true);
    }
    let h = layout.half();
    (Vec2::new(-h.x - layout.left + 14.0, h.y - 50.0), left.tab == tab && tab != LeftTab::Tasks)
}

/// The warp core's default font has no middle dot, ellipsis or en dash
/// (they drew as boxes on the 03:09 screenshot): draw them in ASCII.
pub fn glyphs(s: &str) -> String {
    s.replace('\u{00b7}', "|").replace('\u{2026}', "...").replace('\u{2013}', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_window_labels_distinguish_zero_and_unknown() {
        let mut seat = SeatInfo::default();
        assert_eq!(context_line(&seat), "Context unavailable");
        seat.ctx_tokens = Some(0.0);
        seat.ctx_capacity = Some(200_000.0);
        seat.ctx = Some(0.0);
        assert_eq!(context_line(&seat), "Context 0/200.0k tokens | 0%");
    }

    #[test]
    fn glyphs_the_font_lacks_become_ascii() {
        assert_eq!(glyphs("t82 \u{00b7} Held 1h\u{2026} 2\u{2013}3"), "t82 | Held 1h... 2-3");
    }

    #[test]
    fn titles_read_like_a_person_wrote_them() {
        assert_eq!(clean_title("Right, once those are done, can we resolve the task obiters on claude?"), "Once those are done, can we resolve the task obiters on claude?");
        assert_eq!(clean_title("we need a local script that ticks away checking to see if we're conne…"), "Local script that ticks away checking to see if we're conne…");
        assert_eq!(clean_title("@claude let's start building the left panel"), "Start building the left panel");
        assert_eq!(clean_title("Fix the ring"), "Fix the ring");
        assert_eq!(clean_title("okay"), "Okay");
    }

    #[test]
    fn wrap_keeps_words_and_marks_a_cut() {
        assert_eq!(wrap("one two three four", 9, 2), vec!["one two", "three…"]);
        assert_eq!(wrap("short", 20, 2), vec!["short"]);
        assert_eq!(wrap("abcdefghijk", 4, 2), vec!["abcd", "efg…"]);
        assert_eq!(wrap("partial…", 12, 2), vec!["partial…"]);
        assert!(wrap("short", 0, 2).is_empty());
    }

    #[test]
    fn row_window_reserves_a_scroll_cue_and_clamps_after_resize() {
        assert_eq!(visible_rows(12, 1, 5), (1, 5, true));
        assert_eq!(visible_rows(12, 40, 5), (8, 12, true));
        assert_eq!(visible_rows(3, 40, 5), (0, 3, false));
        assert_eq!(visible_rows(0, 40, 0), (0, 0, false));
    }

    #[test]
    fn task_spans_group_open_first_then_by_holder() {
        let t = |id: &str, holder: Option<&str>, since: f64| feed::TaskInfo {
            id: id.into(), title: Some("we need to fix it".into()), since: Some(since), from: "owner".into(), holder: holder.map(Into::into), open: holder.is_none(),
        };
        let spans = task_spans(&[t("t1", Some("vibe"), 0.0), t("t2", None, 900.0), t("t3", Some("claude"), 990.0)], 1000.0, 40);
        let all: String = spans.iter().map(|s| s.0.as_str()).collect();
        let open = all.find("OPEN").unwrap();
        let claude = all.find("CLAUDE").unwrap();
        let vibe = all.find("VIBE").unwrap();
        assert!(open < claude && claude < vibe, "{all}");
        assert!(all.starts_with("TASKS   3 on the board · 1 open · oldest 16m"), "{all}");
        assert!(all.contains("Fix it"));
        assert_eq!(age_colour(60.0), [0.55, 0.62, 0.75]);
        assert_eq!(age_colour(3600.0), [1.0, 0.72, 0.3]);
        assert_eq!(age_colour(9000.0), [1.0, 0.38, 0.32]);
    }

    #[test]
    fn header_brackets_the_current_tab() {
        assert_eq!(header_text(LeftTab::Tasks), " F4 SYSTEM  [F5 TASKS]  F6 CREW ");
    }

    #[test]
    fn net_line_reads_netwatch() {
        let up = serde_json::json!({"time": 1000.0, "online": true});
        assert_eq!(net_line(Some(&up), 1010.0), "net    online");
        let down = serde_json::json!({"time": 1000.0, "online": false, "down_since": 880.0, "last_step": "reconnect wld0"});
        assert!(net_line(Some(&down), 1000.0).contains("DOWN for 2m") && net_line(Some(&down), 1000.0).contains("reconnect wld0"));
        assert!(net_line(Some(&up), 1500.0).contains("silent"));
        assert_eq!(net_line(None, 0.0), "net    no netwatch status");
    }

    #[test]
    fn width_is_bounded_and_off_is_zero() {
        let cfg = mods::LeftCfg::default();
        assert_eq!(left_width(2240.0, &cfg), (2240.0f32 * 0.22).clamp(260.0, 520.0));
        assert_eq!(left_width(2240.0, &mods::LeftCfg { show: false, ..cfg }), 0.0);
    }
}
