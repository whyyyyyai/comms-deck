//! warp-core: the comms-deck drawn as a ship's reactor, on the GPU (Bevy).
//!
//! The seats sit in a ring around a pulsing core, each in its deck colour,
//! bright when busy and dim when away. Every note sent on the deck flies as a
//! comet from sender to receiver through the core; a broadcast to `all`
//! sends one to everyone and throws the starfield into warp. It only reads
//! what the service writes (feed.rs), so closing it changes nothing, and the
//! terminal deck keeps working without it.
//!
//! Keys: Space fires a test broadcast, Esc or q quits.
//!
//! Written by: claude

mod feed;

use bevy::{
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    post_process::bloom::Bloom,
    prelude::*,
    sprite::Anchor,
};
use feed::{Msg, NoteInfo, SeatInfo};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Mutex};

const STARS: usize = 260;
const COMET_SECS: f32 = 1.8;
const TRAIL: usize = 28;

fn main() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || feed::run(tx));
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.01, 0.012, 0.03)))
        .insert_resource(Feed(Mutex::new(rx)))
        .insert_resource(Deck::default())
        .insert_resource(Rng(0x9E37_79B9_7F4A_7C15))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "warp core".into(),
                // The Wayland app id: the scratchpad keeper places windows by it.
                name: Some("warp-core".into()),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                read_feed,
                layout_seats,
                glow_seats,
                pulse_core,
                fly_comets,
                move_stars,
                draw_links,
                hud,
                keys,
            )
                .chain(),
        )
        .run();
}

// ---------------------------------------------------------------- state

#[derive(Resource)]
struct Feed(Mutex<mpsc::Receiver<Msg>>);

#[derive(Resource, Default)]
struct Deck {
    seats: Vec<SeatInfo>,
    /// The seat list the ring was last built from (names only).
    built: Vec<String>,
    /// When notes arrived (seconds of app time), for the core's pace.
    arrivals: VecDeque<f32>,
    /// 0..1, the starfield's warp: kicked by broadcasts, decays on its own.
    warp: f32,
    /// How far round the orbit the ring has turned (radians).
    orbit: f32,
    problem: Option<String>,
    pending: Vec<NoteInfo>,
}

impl Deck {
    fn notes_last(&self, now: f32, secs: f32) -> usize {
        self.arrivals.iter().filter(|t| now - **t <= secs).count()
    }
}

/// A tiny xorshift: the stars need scatter, not cryptography.
#[derive(Resource)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Component)]
struct SeatNode {
    name: String,
    /// The extra glow of a note just landed, fading to 0.
    flash: f32,
    /// 0 at the back of the orbit (behind the core), 1 at the front.
    depth: f32,
}

#[derive(Component)]
struct SeatLabel;

#[derive(Component)]
struct Core;

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct Star {
    dir: Vec2,
    dist: f32,
    speed: f32,
}

#[derive(Component)]
struct Comet {
    from: Vec2,
    via: Vec2,
    to: String,
    to_pos: Vec2,
    t: f32,
    colour: Color,
    trail: VecDeque<Vec2>,
}

#[derive(Resource)]
struct Assets2d {
    dot: Handle<Mesh>,
    star: Handle<Mesh>,
    seat: Handle<Mesh>,
}

// ---------------------------------------------------------------- colours

/// The deck's seat colours (theme.rs in the terminal deck), as xterm-256
/// indexes, so a seat looks the same on both screens.
fn seat_index(seat: &str) -> u8 {
    match seat {
        "claude" => 208,
        "grok" => 45,
        "codex" => 78,
        "vibe" => 203,
        "devin" => 141,
        "owner" => 220,
        "push-courier" => 245,
        other => (140 + other.chars().map(|c| c as u32).sum::<u32>() % 40) as u8,
    }
}

/// xterm-256 index -> linear-ish RGB in 0..1.
fn xterm_rgb(n: u8) -> Vec3 {
    let n = n as u32;
    let (r, g, b) = if n < 16 {
        let base = [
            (0, 0, 0), (128, 0, 0), (0, 128, 0), (128, 128, 0), (0, 0, 128), (128, 0, 128),
            (0, 128, 128), (192, 192, 192), (128, 128, 128), (255, 0, 0), (0, 255, 0),
            (255, 255, 0), (0, 0, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
        ];
        base[n as usize]
    } else if n < 232 {
        let c = n - 16;
        let step = |v: u32| if v == 0 { 0 } else { 55 + v * 40 };
        (step(c / 36), step((c / 6) % 6), step(c % 6))
    } else {
        let v = 8 + (n - 232) * 10;
        (v, v, v)
    };
    Vec3::new(r as f32, g as f32, b as f32) / 255.0
}

/// A seat's colour at a brightness; above 1 it blooms.
fn seat_colour(seat: &str, brightness: f32) -> Color {
    let c = xterm_rgb(seat_index(seat)) * brightness;
    Color::linear_rgb(c.x, c.y, c.z)
}

/// How brightly a seat burns for what the deck shows.
fn state_brightness(shown: &str) -> f32 {
    match shown {
        "busy" => 3.2,
        "stuck" => 2.0,
        "ready" | "idle" => 1.1,
        "spent" => 0.35,
        _ => 0.22, // away, not set up, paused
    }
}

fn label_for(name: &str) -> String {
    if name == "owner" { "Y".into() } else { name.into() }
}

// ---------------------------------------------------------------- setup

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut rng: ResMut<Rng>,
) {
    commands.spawn((
        Camera2d,
        Tonemapping::TonyMcMapface,
        Bloom { intensity: 0.28, ..Bloom::NATURAL },
        DebandDither::Enabled,
    ));

    let assets = Assets2d {
        dot: meshes.add(Circle::new(5.0)),
        star: meshes.add(Rectangle::new(1.0, 1.6)),
        seat: meshes.add(Circle::new(26.0)),
    };

    // The core: a white-hot centre in a blue shell.
    commands
        .spawn((
            Core,
            Mesh2d(meshes.add(Circle::new(54.0))),
            MeshMaterial2d(materials.add(Color::linear_rgb(0.25, 0.9, 3.2))),
            Transform::from_xyz(0.0, 0.0, 1.0),
        ))
        .with_children(|core| {
            core.spawn((
                Mesh2d(meshes.add(Circle::new(22.0))),
                MeshMaterial2d(materials.add(Color::linear_rgb(6.0, 6.0, 7.0))),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
        });

    let star_mat = materials.add(Color::linear_rgb(0.9, 0.95, 1.4));
    for _ in 0..STARS {
        let a = rng.next() * std::f32::consts::TAU;
        let dir = Vec2::from_angle(a);
        commands.spawn((
            Star { dir, dist: 40.0 + rng.next() * 1400.0, speed: 0.4 + rng.next() * 1.2 },
            Mesh2d(assets.star.clone()),
            MeshMaterial2d(star_mat.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
    }

    commands.spawn((
        Hud,
        Text2d::new("warp core: waiting for the deck"),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::srgb(0.55, 0.65, 0.8)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 5.0),
    ));

    commands.insert_resource(assets);
}

// ---------------------------------------------------------------- systems

fn read_feed(feed: Res<Feed>, mut deck: ResMut<Deck>, time: Res<Time>) {
    let now = time.elapsed_secs();
    let Ok(rx) = feed.0.lock() else { return };
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Msg::Seats(seats) => {
                deck.seats = seats;
                deck.problem = None;
            }
            Msg::Problem(p) => deck.problem = Some(p),
            Msg::Note(n) => {
                deck.arrivals.push_back(now);
                if n.all {
                    deck.warp = 1.0;
                }
                deck.pending.push(n);
            }
        }
    }
    while deck.arrivals.front().is_some_and(|t| now - *t > 3600.0) {
        deck.arrivals.pop_front();
    }
    deck.warp = (deck.warp - time.delta_secs() * 0.22).max(0.0);
}

/// Where each seat sits on its orbit: evenly spaced in the service's order,
/// the first at the top when `orbit` is 0, turning clockwise as it grows.
/// The orbit is tilted towards the viewer, so the top of the ellipse is the
/// far side: returns (position, depth), depth 0 at the back, 1 at the front.
fn ring_positions(names: &[String], window: Vec2, orbit: f32) -> HashMap<String, (Vec2, f32)> {
    let radius = Vec2::new(window.x * 0.38, window.y * 0.30);
    let n = names.len().max(1) as f32;
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let a = std::f32::consts::FRAC_PI_2 - i as f32 / n * std::f32::consts::TAU - orbit;
            let depth = 0.5 - 0.5 * a.sin();
            (name.clone(), (Vec2::new(a.cos() * radius.x, a.sin() * radius.y), depth))
        })
        .collect()
}

/// The orbit's speed in radians a second: a slow drift when quiet, faster
/// with traffic, and a hard swing in warp.
fn orbit_speed(notes_10min: usize, warp: f32) -> f32 {
    0.05 + (notes_10min.min(30) as f32) * 0.006 + warp * 1.2
}

fn window_size(windows: &Query<&Window>) -> Vec2 {
    windows.iter().next().map(|w| w.size()).unwrap_or(Vec2::new(1280.0, 800.0))
}

/// Builds the ring when the seat list changes, and keeps it fitted to the
/// window when that is resized.
fn layout_seats(
    mut commands: Commands,
    mut deck: ResMut<Deck>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    windows: Query<&Window>,
    time: Res<Time>,
    mut nodes: Query<(Entity, &mut SeatNode, &mut Transform)>,
) {
    let pace = deck.notes_last(time.elapsed_secs(), 600.0);
    let warp = deck.warp;
    deck.orbit = (deck.orbit + time.delta_secs() * orbit_speed(pace, warp)) % std::f32::consts::TAU;
    let names: Vec<String> = deck.seats.iter().filter(|s| s.online()).map(|s| s.name.clone()).collect();
    let pos = ring_positions(&names, window_size(&windows), deck.orbit);
    if names != deck.built {
        for (e, _, _) in &nodes {
            commands.entity(e).despawn();
        }
        for name in &names {
            let (p, depth) = pos[name];
            commands
                .spawn((
                    SeatNode { name: name.clone(), flash: 0.0, depth },
                    Mesh2d(assets.seat.clone()),
                    MeshMaterial2d(materials.add(seat_colour(name, 1.0))),
                    Transform::from_xyz(p.x, p.y, 2.0),
                ))
                .with_children(|seat| {
                    seat.spawn((
                        SeatLabel,
                        Text2d::new(label_for(name)),
                        TextFont { font_size: FontSize::Px(17.0), ..default() },
                        TextColor(seat_colour(name, 0.9)),
                        Anchor::TOP_CENTER,
                        Transform::from_xyz(0.0, -36.0, 0.1),
                    ));
                });
        }
        deck.built = names;
        return;
    }
    for (_, mut node, mut tf) in &mut nodes {
        if let Some((p, depth)) = pos.get(&node.name) {
            tf.translation.x = p.x;
            tf.translation.y = p.y;
            // Behind the core on the far side, in front of it on the near side.
            tf.translation.z = if *depth < 0.5 { 0.5 } else { 2.0 };
            node.depth = *depth;
        }
    }
}

fn glow_seats(
    deck: Res<Deck>,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut nodes: Query<(&mut SeatNode, &MeshMaterial2d<ColorMaterial>, &mut Transform, &Children)>,
    mut labels: Query<&mut Text2d, With<SeatLabel>>,
) {
    let t = time.elapsed_secs();
    for (mut node, mat, mut tf, children) in &mut nodes {
        let Some(seat) = deck.seats.iter().find(|s| s.name == node.name) else { continue };
        node.flash = (node.flash - time.delta_secs() * 1.6).max(0.0);
        let mut b = state_brightness(&seat.shown);
        if seat.shown == "busy" {
            b *= 0.85 + 0.15 * (t * 3.0 + node.name.len() as f32).sin();
        }
        b *= 0.55 + 0.45 * node.depth; // the far side of the orbit is dimmer
        b += node.flash * 6.0;
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.color = seat_colour(&node.name, b);
        }
        tf.scale = Vec3::splat((0.72 + 0.4 * node.depth) * (1.0 + node.flash * 0.35));
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                let mut line = format!("{}\n{}", label_for(&node.name), seat.shown);
                if seat.waiting > 0 {
                    line.push_str(&format!(" · {} waiting", seat.waiting));
                }
                if text.0 != line {
                    text.0 = line;
                }
            }
        }
    }
}

/// The core breathes faster the more notes fly: idle, it barely moves.
fn pulse_core(
    deck: Res<Deck>,
    time: Res<Time>,
    mut core: Query<&mut Transform, With<Core>>,
    mut gizmos: Gizmos,
) {
    let t = time.elapsed_secs();
    let pace = deck.notes_last(t, 600.0) as f32;
    let rate = 0.8 + pace.min(30.0) * 0.25;
    let s = 1.0 + 0.07 * (t * rate).sin() + deck.warp * 0.25;
    for mut tf in &mut core {
        tf.scale = Vec3::splat(s);
    }
    // Two containment rings turning against each other.
    for (r, dir, k) in [(78.0, 1.0, 0.6), (96.0, -1.0, 0.35)] {
        let spin = t * dir * (0.4 + rate * 0.2);
        for i in 0..3 {
            let start = spin + i as f32 * std::f32::consts::TAU / 3.0;
            gizmos.arc_2d(
                Isometry2d::from_rotation(Rot2::radians(start)),
                1.4,
                r * s,
                Color::linear_rgb(0.2 * k, 0.7 * k, 2.4 * k),
            );
        }
    }
}

/// Faint conduits from each seat to the core; busy ones carry light.
fn draw_links(deck: Res<Deck>, nodes: Query<(&SeatNode, &Transform)>, time: Res<Time>, mut gizmos: Gizmos) {
    let t = time.elapsed_secs();
    for (node, tf) in &nodes {
        let Some(seat) = deck.seats.iter().find(|s| s.name == node.name) else { continue };
        let p = tf.translation.truncate();
        let inner = p.normalize_or_zero() * 70.0;
        let b = (state_brightness(&seat.shown) * 0.18).min(0.6) + node.flash;
        gizmos.line_gradient_2d(inner, p * 0.9, seat_colour(&node.name, b), seat_colour(&node.name, b * 0.4));
        if seat.shown == "busy" {
            // A packet of light running down the conduit.
            let k = (t * 0.7 + node.name.len() as f32 * 0.13).fract();
            gizmos.circle_2d(Isometry2d::from_translation(p * 0.9 - (p * 0.9 - inner) * k), 2.5, seat_colour(&node.name, 3.0));
        }
    }
}

fn bezier(a: Vec2, b: Vec2, c: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    a * u * u + b * 2.0 * u * t + c * t * t
}

fn fly_comets(
    mut commands: Commands,
    mut deck: ResMut<Deck>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
    mut comets: Query<(Entity, &mut Comet, &mut Transform), Without<SeatNode>>,
    mut nodes: Query<(&mut SeatNode, &Transform), Without<Comet>>,
    mut gizmos: Gizmos,
) {
    let positions: HashMap<String, Vec2> =
        nodes.iter().map(|(n, tf)| (n.name.clone(), tf.translation.truncate())).collect();

    // Launch what arrived this frame. Before the first status poll there is
    // no ring yet, so notes wait rather than vanish (Vibe's review of #150).
    if positions.is_empty() && deck.seats.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut deck.pending);
    for note in pending {
        let from = positions.get(&note.from).copied().unwrap_or(Vec2::ZERO);
        let targets: Vec<String> = if note.all {
            positions.keys().filter(|k| **k != note.from).cloned().collect()
        } else {
            note.to.clone()
        };
        for to in targets {
            let Some(to_pos) = positions.get(&to).copied() else { continue };
            // Through the core, bowed a little so two-way traffic doesn't overlap.
            let via = (from + to_pos) * 0.12 + (to_pos - from).perp().normalize_or_zero() * 40.0;
            let colour = seat_colour(&note.from, 5.0);
            commands.spawn((
                Comet { from, via, to: to.clone(), to_pos, t: 0.0, colour, trail: VecDeque::new() },
                Mesh2d(assets.dot.clone()),
                MeshMaterial2d(materials.add(colour)),
                Transform::from_xyz(from.x, from.y, 3.0),
            ));
        }
    }

    let dt = time.delta_secs();
    for (e, mut comet, mut tf) in &mut comets {
        // Follow the receiver if the ring moved (a resize) mid-flight.
        if let Some(p) = positions.get(&comet.to) {
            comet.to_pos = *p;
        }
        comet.t += dt / COMET_SECS;
        let eased = 1.0 - (1.0 - comet.t.min(1.0)).powi(2);
        let p = bezier(comet.from, comet.via, comet.to_pos, eased);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        comet.trail.push_front(p);
        comet.trail.truncate(TRAIL);
        let n = comet.trail.len();
        for (i, w) in comet.trail.iter().zip(comet.trail.iter().skip(1)).enumerate() {
            let fade = 1.0 - i as f32 / n as f32;
            let c = comet.colour.to_linear() * fade * 0.8;
            gizmos.line_2d(*w.0, *w.1, Color::linear_rgb(c.red, c.green, c.blue));
        }
        if comet.t >= 1.0 {
            for (mut node, _) in &mut nodes {
                if node.name == comet.to {
                    node.flash = 1.0;
                }
            }
            commands.entity(e).despawn();
        }
    }
}

/// Stars stream out from the core; in warp they stretch into lines.
fn move_stars(
    deck: Res<Deck>,
    time: Res<Time>,
    windows: Query<&Window>,
    mut rng: ResMut<Rng>,
    mut stars: Query<(&mut Star, &mut Transform)>,
) {
    let half = window_size(&windows) * 0.5;
    let reach = half.length() + 20.0;
    let pace = (deck.notes_last(time.elapsed_secs(), 600.0) as f32).min(30.0);
    let speed = 18.0 + pace * 2.0 + deck.warp * 900.0;
    let dt = time.delta_secs();
    for (mut star, mut tf) in &mut stars {
        star.dist += speed * star.speed * dt * (0.3 + star.dist / reach);
        if star.dist > reach {
            star.dir = Vec2::from_angle(rng.next() * std::f32::consts::TAU);
            star.dist = 30.0 + rng.next() * 120.0;
        }
        let p = star.dir * star.dist;
        let near = (star.dist / reach).clamp(0.0, 1.0);
        let stretch = 1.0 + deck.warp * 40.0 * near + pace * 0.1;
        tf.translation = Vec3::new(p.x, p.y, 0.0);
        tf.rotation = Quat::from_rotation_z(star.dir.to_angle());
        tf.scale = Vec3::new(stretch * (1.0 + near), 0.6 + near, 1.0);
    }
}

fn hud(
    deck: Res<Deck>,
    time: Res<Time>,
    windows: Query<&Window>,
    mut hud: Query<(&mut Text2d, &mut Transform), With<Hud>>,
) {
    let half = window_size(&windows) * 0.5;
    let now = time.elapsed_secs();
    let online: Vec<_> = deck.seats.iter().filter(|s| s.online()).collect();
    let busy = online.iter().filter(|s| s.shown == "busy").count();
    let waiting: u64 = online.iter().map(|s| s.waiting).sum();
    let mut line = format!(
        "WARP CORE    {} online · {} busy · {} waiting · {} notes in the last hour",
        online.len(),
        busy,
        waiting,
        deck.notes_last(now, 3600.0),
    );
    if deck.warp > 0.05 {
        line.push_str("    ▶ WARP");
    }
    if let Some(p) = &deck.problem {
        line.push_str(&format!("\n{p}"));
    }
    for (mut text, mut tf) in &mut hud {
        if text.0 != line {
            text.0 = line.clone();
        }
        tf.translation.x = -half.x + 18.0;
        tf.translation.y = half.y - 14.0;
    }
}

fn keys(keys: Res<ButtonInput<KeyCode>>, mut deck: ResMut<Deck>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyQ) {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::Space) {
        // A test broadcast from Y: nothing is sent, it only flies here.
        deck.warp = 1.0;
        deck.pending.push(NoteInfo { from: "owner".into(), to: Vec::new(), all: true });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_match_the_terminal_deck() {
        assert_eq!(xterm_rgb(16), Vec3::ZERO);
        assert_eq!(xterm_rgb(231), Vec3::ONE);
        // 208, Claude's orange: (255, 135, 0)
        assert_eq!(xterm_rgb(208), Vec3::new(1.0, 135.0 / 255.0, 0.0));
        assert_eq!(seat_index("y"), seat_index("y"), "stable for unknown seats");
    }

    #[test]
    fn the_ring_puts_the_first_seat_at_the_top() {
        let names: Vec<String> = ["owner", "claude", "grok", "vibe"].map(String::from).to_vec();
        let pos = ring_positions(&names, Vec2::new(1000.0, 1000.0), 0.0);
        assert!(pos["owner"].0.x.abs() < 0.01 && pos["owner"].0.y > 0.0);
        assert!(pos["owner"].1 < 0.01, "the top is the far side");
        assert!(pos["claude"].0.x > 0.0, "clockwise from the top");
        assert!(pos["grok"].0.y < 0.0 && pos["grok"].1 > 0.99, "the bottom is the near side");
        // A quarter turn later, the first seat has moved to where the second was.
        let later = ring_positions(&names, Vec2::new(1000.0, 1000.0), std::f32::consts::FRAC_PI_2);
        assert!((later["owner"].0 - pos["claude"].0).length() < 0.01);
    }

    #[test]
    fn the_orbit_drifts_when_quiet_and_swings_in_warp() {
        assert!(orbit_speed(0, 0.0) > 0.0, "never quite still");
        assert!(orbit_speed(20, 0.0) > orbit_speed(0, 0.0));
        assert!(orbit_speed(0, 1.0) > 10.0 * orbit_speed(0, 0.0));
    }

    #[test]
    fn busy_burns_brighter_than_away() {
        assert!(state_brightness("busy") > 1.0, "busy blooms");
        assert!(state_brightness("away") < state_brightness("ready"));
    }
}
