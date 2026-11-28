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

#[derive(Resource, Default)]
pub struct Left {
    pub tab: LeftTab,
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
        let wait = if seat.waiting > 0 { format!("{} waiting, oldest {}", seat.waiting, ago(seat.oldest_waiting_s)) } else { "no notes waiting".into() };
        s.push_str(&format!("         {wait}\n"));
        if seat.ctx.is_some() || seat.five_hour.is_some() || seat.week.is_some() {
            s.push_str(&format!("         ctx {}  5h {}  wk {}\n", pct(seat.ctx), pct(seat.five_hour), pct(seat.week)));
        }
        if !held.is_empty() {
            s.push_str(&format!("         holds {}\n", held.join(", ")));
        }
        s.push('\n');
    }
    s
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
        if keys.just_pressed(k) {
            left.tab = tab;
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
    mut header: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<LeftHeader>, Without<LeftBg>, Without<LeftBody>)>,
    mut body: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<LeftBody>, Without<LeftBg>, Without<LeftHeader>)>,
    mut state: Local<(bool, f32, Option<serde_json::Value>)>,
) {
    let font = mods.cfg.left.font;
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
        state.0 = true;
    }
    let show = layout.left > 0.0;
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    let h = layout.half();
    let x0 = -h.x - layout.left; // the panel's left edge, in world
    for (mut tf, mut vis) in &mut bg {
        *vis = want;
        tf.translation.x = x0 + layout.left * 0.5;
        tf.translation.y = 0.0;
        tf.scale = Vec3::new(layout.left, layout.scene.y, 1.0);
    }
    for (mut t, mut tf, mut vis) in &mut header {
        *vis = want;
        let text = header_text(left.tab);
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
        *vis = want;
        let (text, y) = match left.tab {
            // The stats themselves are the SysPanel text, placed above this.
            LeftTab::System => (net_line(state.2.as_ref(), unix), -h.y + 90.0),
            LeftTab::Tasks => (String::new(), h.y - 50.0),
            LeftTab::Crew => (crew_text(&deck.seats, &deck.tasks), h.y - 50.0),
        };
        if t.0 != text {
            t.0 = text;
        }
        tf.translation.x = x0 + 14.0;
        tf.translation.y = y;
    }
}

/// Where the floating SYSTEM and TASKS texts go, and whether they show:
/// inside the panel on their tab when it's on, where they always were when off.
pub fn place(layout: &Layout, left: &Left, tab: LeftTab, old: Vec2) -> (Vec2, bool) {
    if layout.left <= 0.0 {
        return (old, true);
    }
    let h = layout.half();
    (Vec2::new(-h.x - layout.left + 14.0, h.y - 50.0), left.tab == tab)
}

#[cfg(test)]
mod tests {
    use super::*;

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
