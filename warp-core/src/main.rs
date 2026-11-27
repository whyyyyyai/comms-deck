//! warp-core: the comms-deck drawn as a ship's reactor, on the GPU (Bevy).
//!
//! The seats sit in a ring around a pulsing core, each in its deck colour,
//! bright when busy and dim when away. Every note sent on the deck flies as a
//! comet from sender to receiver through the core; a broadcast to `all`
//! sends one to everyone and throws the starfield into warp. Around each
//! seat, fuel rings show its context / 5-hour / weekly meters and moons show
//! its waiting notes, whitening to red as the oldest one ages. The core runs
//! hotter as the crew's meters fill.
//!
//! It is moddable while it runs: mods/warp-core.toml holds every setting and
//! the effect rules, mods/shaders/*.wgsl how the core and the rings are
//! drawn. Save one and it changes on screen; a broken edit is refused and
//! the HUD says why (mods.rs).
//!
//! The chat panel on the right shows the notes and has a chat box that
//! sends as Y (chat.rs), the one thing the warp core does rather than reads.
//! Everything else only reads what the service writes (feed.rs), so closing
//! it changes nothing, and the terminal deck, chat included, keeps working
//! without it. The stars take the crew's colours in proportion to how their
//! context is split.
//!
//! Mouse: hover a seat for its details, click it to focus its traffic and
//! address the chat box to it, click empty space to clear; the wheel over the
//! panel scrolls the notes. Keys while typing: Enter sends, Esc clears (again
//! to leave the box), Ctrl+U/W/V clear, delete a word, paste; Tab or Esc on
//! an empty line hands the keys to the ring: 1-9 focus a seat, 0 clears, h
//! HUD, c fuel rings, m moons, f freeze, r reload mods, Space a test
//! broadcast, PgUp/PgDn scroll, Enter or Tab back to the box, q quits.
//! Ctrl+Q quits from either.
//!
//! Written by: claude

mod chat;
mod feed;
mod mods;

use bevy::{
    asset::uuid_handle,
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseWheel,
    },
    text::TextBounds,
    post_process::bloom::Bloom,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{Shader, ShaderRef},
    sprite::Anchor,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
    winit::{UpdateMode, WinitSettings},
};
use feed::{ChatNote, Msg, NoteInfo, SeatInfo};
use mods::{Config, Rule, Val};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Mutex};

const STARS: usize = 260;
const CORE_SHADER: Handle<Shader> = uuid_handle!("6f1d2c3a-6b0e-4c55-9a1e-2f4c8d7b9e01");
const RING_SHADER: Handle<Shader> = uuid_handle!("0b9e3f4d-1a2c-4e6f-8b7d-5c3a2e1f0d02");
/// Fuel ring diameters around a seat: context, 5-hour, week.
const FUEL_D: [f32; 3] = [70.0, 82.0, 94.0];
const HALO_D: f32 = 116.0;

fn main() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || feed::run(tx));
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.01, 0.012, 0.03)))
        .insert_resource(Feed(Mutex::new(rx)))
        .insert_resource(Deck::default())
        .insert_resource(Mods::default())
        .insert_resource(View::default())
        .insert_resource(Looks::default())
        .insert_resource(Chat::default())
        .insert_resource(Layout::default())
        .insert_resource(Rng(0x9E37_79B9_7F4A_7C15))
        // 30 fps until the mods say otherwise ([look] fps): it is a display,
        // not a game, and half the CPU of drawing at the refresh rate.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(std::time::Duration::from_millis(33)),
            unfocused_mode: UpdateMode::reactive(std::time::Duration::from_millis(33)),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "warp core".into(),
                // The Wayland app id: the scratchpad keeper places windows by it.
                name: Some("warp-core".into()),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((Material2dPlugin::<RingMaterial>::default(), Material2dPlugin::<CoreMaterial>::default()))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (watch_mods, read_feed, layout_view, apply_rules, layout_seats, pointer, chat_input, keys).chain(),
                (
                    glow_seats,
                    draw_rings,
                    pulse_core,
                    fly_comets,
                    move_stars,
                    colour_stars,
                    draw_links,
                    draw_moons_and_marks,
                    hud,
                    tooltip,
                    chat_results,
                    draw_chat,
                )
                    .chain(),
            )
                .chain(),
        )
        .run();
}

// ---------------------------------------------------------------- materials

#[derive(ShaderType, Clone, Copy, Debug, Default)]
struct RingUniform {
    colour: Vec4,
    back: Vec4,
    /// fill, thickness, time, pulse
    params: Vec4,
}

/// mods/shaders/ring.wgsl: the fuel rings and the rule halos.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
struct RingMaterial {
    #[uniform(0)]
    u: RingUniform,
}

impl Material2d for RingMaterial {
    fn fragment_shader() -> ShaderRef {
        RING_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
struct CoreUniform {
    cool: Vec4,
    hot: Vec4,
    /// time, heat, warp, traffic
    params: Vec4,
}

/// mods/shaders/core.wgsl: the reactor itself.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
struct CoreMaterial {
    #[uniform(0)]
    u: CoreUniform,
}

impl Material2d for CoreMaterial {
    fn fragment_shader() -> ShaderRef {
        CORE_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

fn v4(c: [f32; 3], a: f32) -> Vec4 {
    Vec4::new(c[0], c[1], c[2], a)
}

// ---------------------------------------------------------------- state

#[derive(Resource)]
struct Feed(Mutex<mpsc::Receiver<Msg>>);

/// A note the screen has seen: who and when, never what it said.
#[derive(Clone, Debug)]
struct Seen {
    at: f32,
    from: String,
    to: Vec<String>,
    all: bool,
}

struct Shock {
    seat: String,
    at: f32,
    colour: [f32; 3],
    size: f32,
}

#[derive(Resource, Default)]
struct Deck {
    seats: Vec<SeatInfo>,
    /// The seat list the ring was last built from (names only).
    built: Vec<String>,
    /// When notes arrived (seconds of app time), for the core's pace.
    arrivals: VecDeque<f32>,
    /// The last notes seen, newest first, for the hover card.
    history: VecDeque<Seen>,
    /// 0..1, the starfield's warp: kicked by broadcasts, decays on its own.
    warp: f32,
    /// How far round the orbit the ring has turned (radians).
    orbit: f32,
    /// 0..1, built by typing in the chat box, draining on its own.
    thrust: f32,
    problem: Option<String>,
    pending: Vec<NoteInfo>,
    shocks: Vec<Shock>,
    /// The seat under the pointer, and the one clicked to focus.
    hover: Option<String>,
    focus: Option<String>,
    cursor: Option<Vec2>,
}

impl Deck {
    fn notes_last(&self, now: f32, secs: f32) -> usize {
        self.arrivals.iter().filter(|t| now - **t <= secs).count()
    }
    fn seat(&self, name: &str) -> Option<&SeatInfo> {
        self.seats.iter().find(|s| s.name == name)
    }
}

/// The live mods and what became of the last attempt to load each file.
#[derive(Resource, Default)]
struct Mods {
    cfg: Config,
    rules: Vec<Rule>,
    watch: mods::Watch,
    /// file -> why it was refused; cleared when it loads.
    errors: Vec<(String, String)>,
    notice: Option<(String, f32)>,
    /// Shaders that have loaded at least once (else the default stands in).
    shaders_ok: Vec<String>,
    since_check: f32,
}

/// The keyboard's switches, on top of the mods' own.
#[derive(Resource)]
struct View {
    hud: bool,
    fuel: bool,
    moons: bool,
}
impl Default for View {
    fn default() -> Self {
        View { hud: true, fuel: true, moons: true }
    }
}

#[derive(Clone, Debug, Default)]
struct Halo {
    colour: [f32; 3],
    pulse: f32,
    size: f32,
    bright: f32,
}

/// What the seat rules say a seat looks like this frame.
#[derive(Clone, Debug)]
struct SeatLook {
    tint: Option<[f32; 3]>,
    glow: f32,
    halo: Option<Halo>,
    beacon: Option<([f32; 3], f32)>,
    labels: Vec<String>,
}
impl Default for SeatLook {
    fn default() -> Self {
        SeatLook { tint: None, glow: 1.0, halo: None, beacon: None, labels: Vec::new() }
    }
}

#[derive(Resource, Default)]
struct Looks(HashMap<String, SeatLook>);

/// Where things go this frame: the scene (ring, core, HUD) on the left, the
/// chat panel on the right. World x runs -scene.x/2 .. scene.x/2 + panel.
#[derive(Resource, Default)]
struct Layout {
    scene: Vec2,
    panel: f32,
}

impl Layout {
    fn half(&self) -> Vec2 {
        self.scene * 0.5
    }
    /// Is this world point over the chat panel?
    fn in_panel(&self, p: Vec2) -> bool {
        self.panel > 0.0 && p.x > self.scene.x * 0.5
    }
}

/// The panel's width for a window: a share of it, kept readable.
fn panel_width(window_w: f32, cfg: &mods::ChatCfg) -> f32 {
    if !cfg.show {
        return 0.0;
    }
    (window_w * cfg.width).clamp(300.0, 600.0).min(window_w * 0.6)
}

type SendResult = (String, Result<String, String>);

/// The chat panel: the notes, the line being typed, and sends in flight.
#[derive(Resource)]
struct Chat {
    notes: VecDeque<ChatNote>,
    input: String,
    /// True: keys go to the chat box. False: they drive the ring.
    focus: bool,
    /// How many notes back from the newest the panel is scrolled.
    scroll: usize,
    /// The last send's outcome (text, is_error, real time).
    status: Option<(String, bool, f32)>,
    sending: usize,
    tx: mpsc::Sender<SendResult>,
    rx: Mutex<mpsc::Receiver<SendResult>>,
    /// The notes need laying out again.
    dirty: bool,
    drawn_width: f32,
}

impl Default for Chat {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Chat {
            notes: VecDeque::new(),
            input: String::new(),
            focus: true,
            scroll: 0,
            status: None,
            sending: 0,
            tx,
            rx: Mutex::new(rx),
            dirty: true,
            drawn_width: 0.0,
        }
    }
}

#[derive(Component)]
struct ChatPanel;
#[derive(Component)]
struct ChatHeader;
#[derive(Component)]
struct ChatTitle;
#[derive(Component)]
struct ChatLog;
#[derive(Component)]
struct ChatLine;
#[derive(Component)]
struct ChatStatus;

/// The default star material and one per seat, for colouring by context.
#[derive(Resource)]
struct StarMats {
    default: Handle<ColorMaterial>,
    seats: HashMap<String, Handle<ColorMaterial>>,
    /// What the stars were last coloured for, so they're only redone on change.
    key: String,
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

/// One of a seat's fuel rings: 0 context, 1 five-hour, 2 week.
#[derive(Component)]
struct FuelRing(usize);

#[derive(Component)]
struct HaloRing;

#[derive(Component)]
struct Core;

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct Dock;

#[derive(Component)]
struct Tooltip;

#[derive(Component)]
struct Star {
    dir: Vec2,
    dist: f32,
    speed: f32,
    /// 0..1, fixed: which seat's share of the sky this star falls in.
    slot: f32,
}

#[derive(Component)]
struct Comet {
    from: Vec2,
    via: Vec2,
    to: String,
    to_pos: Vec2,
    t: f32,
    seconds: f32,
    colour: Color,
    trail: VecDeque<Vec2>,
    shock: Option<([f32; 3], f32)>,
}

#[derive(Resource)]
struct Assets2d {
    dot: Handle<Mesh>,
    star: Handle<Mesh>,
    seat: Handle<Mesh>,
    quad: Handle<Mesh>,
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

fn seat_rgb(seat: &str) -> [f32; 3] {
    xterm_rgb(seat_index(seat)).to_array()
}

fn lin(c: [f32; 3], brightness: f32) -> Color {
    Color::linear_rgb(c[0] * brightness, c[1] * brightness, c[2] * brightness)
}

/// A seat's colour at a brightness; above 1 it blooms.
fn seat_colour(seat: &str, brightness: f32) -> Color {
    lin(seat_rgb(seat), brightness)
}

fn mix3(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    let k = k.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}

fn label_for(name: &str) -> String {
    if name == "owner" { "Y".into() } else { name.into() }
}

/// "3m", "1h 20m", "2d 4h": how long ago, for the HUD and the hover card.
fn ago(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h {}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d {}h", s / 86400, s % 86400 / 3600),
    }
}

// ---------------------------------------------------------------- rules

/// A seat's fields for the rules (mods::SEAT_FIELDS).
fn seat_val(s: &SeatInfo, focus: Option<&str>, field: &str) -> Val {
    let pct = |p: Option<f64>| Val::Num(p.unwrap_or(-1.0));
    match field {
        "name" => Val::Str(s.name.clone()),
        "state" => Val::Str(s.state.clone()),
        "shown" => Val::Str(s.shown.clone()),
        "online" => Val::Bool(s.online()),
        "busy" | "idle" | "ready" | "stuck" | "spent" => Val::Bool(s.shown == field),
        "waiting" => Val::Num(s.waiting as f64),
        "waiting_normal" => Val::Num(s.waiting_normal as f64),
        "oldest_waiting_s" => Val::Num(s.oldest_waiting_s),
        "oldest_waiting_h" => Val::Num(s.oldest_waiting_s / 3600.0),
        "since_s" => Val::Num(s.since_s),
        "held_by_draft" => Val::Bool(s.held_by_draft),
        "paused" => Val::Bool(s.paused),
        "ctx" => pct(s.ctx),
        "five_hour" => pct(s.five_hour),
        "week" => pct(s.week),
        "fullest" => pct(s.fullest()),
        "has_usage" => Val::Bool(s.fullest().is_some()),
        "focused" => Val::Bool(focus == Some(s.name.as_str())),
        _ => Val::Num(0.0),
    }
}

/// A note's fields for the rules (mods::NOTE_FIELDS).
fn note_val(n: &NoteInfo, field: &str) -> Val {
    match field {
        "from" => Val::Str(n.from.clone()),
        "to" => Val::Str(if n.all { "all".into() } else { n.to.join(",") }),
        "all" => Val::Bool(n.all),
        "to_owner" => Val::Bool(n.to.iter().any(|t| t == "owner")),
        "to_count" => Val::Num(n.to.len() as f64),
        "quiet" => Val::Bool(n.quiet),
        "len" => Val::Num(n.len as f64),
        _ => Val::Num(0.0),
    }
}

fn rule_colour(rule: &Rule, seat: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    rule.spec.colour.as_ref().and_then(|c| c.rgb(seat).ok()).unwrap_or(fallback)
}

fn seat_look(rules: &[Rule], s: &SeatInfo, focus: Option<&str>) -> SeatLook {
    let mut look = SeatLook::default();
    let own = seat_rgb(&s.name);
    let get = |f: &str| seat_val(s, focus, f);
    for r in rules.iter().filter(|r| r.spec.on == "seat") {
        if !r.when.matches(&get) {
            continue;
        }
        let sp = &r.spec;
        match sp.effect.as_str() {
            "halo" if look.halo.is_none() => {
                look.halo = Some(Halo { colour: rule_colour(r, own, own), pulse: sp.pulse, size: sp.size, bright: sp.bright });
            }
            "tint" => {
                look.tint = Some(rule_colour(r, own, own));
                look.glow *= sp.bright;
            }
            "glow" => look.glow *= sp.bright,
            "beacon" if look.beacon.is_none() => look.beacon = Some((rule_colour(r, own, [1.0, 0.12, 0.1]), sp.pulse)),
            "label" if !sp.text.is_empty() => look.labels.push(sp.text.clone()),
            _ => {}
        }
    }
    look
}

struct NoteLook {
    colour: [f32; 3],
    bright: f32,
    size: f32,
    seconds: f32,
    warp: f32,
    shock: Option<([f32; 3], f32)>,
}

fn note_look(rules: &[Rule], cfg: &Config, n: &NoteInfo) -> NoteLook {
    let own = seat_rgb(&n.from);
    let mut look = NoteLook {
        colour: own,
        bright: cfg.comets.brightness,
        size: cfg.comets.size,
        seconds: cfg.comets.seconds,
        warp: if n.all { cfg.warp.on_broadcast } else { 0.0 },
        shock: None,
    };
    let get = |f: &str| note_val(n, f);
    for r in rules.iter().filter(|r| r.spec.on == "note") {
        if !r.when.matches(&get) {
            continue;
        }
        let sp = &r.spec;
        match sp.effect.as_str() {
            "comet" => {
                look.colour = rule_colour(r, own, look.colour);
                look.bright *= sp.bright;
                look.size *= sp.size;
                if let Some(s) = sp.seconds {
                    look.seconds = s;
                }
            }
            "warp" => look.warp = look.warp.max(sp.strength),
            "shockwave" => look.shock = Some((rule_colour(r, own, own), sp.size)),
            _ => {}
        }
    }
    look.seconds = look.seconds.max(0.1);
    look
}

// ---------------------------------------------------------------- setup

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut core_materials: ResMut<Assets<CoreMaterial>>,
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
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
    };

    // The core: drawn by mods/shaders/core.wgsl on a quad.
    commands.spawn((
        Core,
        Mesh2d(assets.quad.clone()),
        MeshMaterial2d(core_materials.add(CoreMaterial::default())),
        Transform::from_xyz(0.0, 0.0, 1.0).with_scale(Vec3::new(170.0, 170.0, 1.0)),
    ));

    let star_mat = materials.add(Color::linear_rgb(0.9, 0.95, 1.4));
    for _ in 0..STARS {
        let a = rng.next() * std::f32::consts::TAU;
        let dir = Vec2::from_angle(a);
        commands.spawn((
            Star { dir, dist: 40.0 + rng.next() * 1400.0, speed: 0.4 + rng.next() * 1.2, slot: rng.next() },
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
    commands.spawn((
        Dock,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(13.0), ..default() },
        TextColor(Color::srgb(0.42, 0.48, 0.6)),
        Anchor::BOTTOM_LEFT,
        Transform::from_xyz(0.0, 0.0, 5.0),
    ));
    commands.spawn((
        Tooltip,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.85, 0.9, 1.0)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 6.0),
        Visibility::Hidden,
    ));

    // The chat panel: a dark column, a header that hides notes scrolled
    // past the top, the notes, the status line and the chat box.
    let panel_bg = materials.add(Color::srgb(0.028, 0.032, 0.058));
    commands.spawn((ChatPanel, Mesh2d(assets.quad.clone()), MeshMaterial2d(panel_bg.clone()), Transform::from_xyz(0.0, 0.0, 7.0)));
    commands.spawn((ChatHeader, Mesh2d(assets.quad.clone()), MeshMaterial2d(panel_bg), Transform::from_xyz(0.0, 0.0, 9.0)));
    commands.spawn((
        ChatTitle,
        Text2d::new("COMMS"),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.55, 0.65, 0.8)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 10.0),
    ));
    commands.spawn((
        ChatLog,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.85, 0.88, 0.95)),
        TextBounds::new_horizontal(300.0),
        Anchor::BOTTOM_LEFT,
        Transform::from_xyz(0.0, 0.0, 8.0),
    ));
    commands.spawn((
        ChatStatus,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(13.0), ..default() },
        TextColor(Color::srgb(0.5, 0.56, 0.68)),
        Anchor::BOTTOM_LEFT,
        Transform::from_xyz(0.0, 0.0, 10.0),
    ));
    commands.spawn((
        ChatLine,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextColor(Color::srgb(0.95, 0.95, 1.0)),
        TextBounds::new_horizontal(300.0),
        Anchor::BOTTOM_LEFT,
        Transform::from_xyz(0.0, 0.0, 10.0),
    ));

    commands.insert_resource(StarMats { default: star_mat, seats: HashMap::new(), key: String::new() });
    commands.insert_resource(assets);
}

// ---------------------------------------------------------------- mods

/// Twice a second, takes in any mod file that changed. Runs on real time,
/// so a frozen screen still picks up edits.
#[allow(clippy::too_many_arguments)]
fn watch_mods(
    mut m: ResMut<Mods>,
    time: Res<Time<Real>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut clear: ResMut<ClearColor>,
    mut winit: ResMut<WinitSettings>,
    mut bloom: Query<&mut Bloom>,
    star_mats: Option<Res<StarMats>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut chat: ResMut<Chat>,
) {
    let now = time.elapsed_secs();
    m.since_check += time.delta_secs();
    if m.since_check < 0.5 && !m.shaders_ok.is_empty() {
        return;
    }
    m.since_check = 0.0;
    let dir = mods::mods_dir();

    let toml_path = dir.join("warp-core.toml");
    if m.watch.changed(&toml_path) {
        let text = mods::read_or(&toml_path, mods::DEFAULT_CONFIG);
        match mods::load_config(&text) {
            Ok((cfg, rules)) => {
                let n = rules.len();
                m.cfg = cfg;
                m.rules = rules;
                m.errors.retain(|(f, _)| f != "warp-core.toml");
                m.notice = Some((format!("warp-core.toml loaded ({n} rules)"), now));
                let c = &m.cfg;
                clear.0 = Color::linear_rgb(c.look.background[0], c.look.background[1], c.look.background[2]);
                let frame = std::time::Duration::from_secs_f32(1.0 / c.look.fps.clamp(5.0, 120.0));
                winit.focused_mode = UpdateMode::reactive(frame);
                winit.unfocused_mode = UpdateMode::reactive(frame);
                for mut b in &mut bloom {
                    b.intensity = c.look.bloom;
                }
                let sc = c.stars.colour;
                if let Some(sm) = &star_mats {
                    if let Some(mut mm) = materials.get_mut(&sm.default) {
                        mm.color = Color::linear_rgb(sc[0], sc[1], sc[2]);
                    }
                }
                chat.dirty = true;
            }
            Err(e) => {
                eprintln!("warp-core: warp-core.toml refused:\n{e}");
                m.errors.retain(|(f, _)| f != "warp-core.toml");
                m.errors.push(("warp-core.toml".into(), mods::short(&e)));
            }
        }
    }

    for (name, handle, default) in [
        ("core.wgsl", CORE_SHADER, mods::DEFAULT_CORE_SHADER),
        ("ring.wgsl", RING_SHADER, mods::DEFAULT_RING_SHADER),
    ] {
        let path = dir.join("shaders").join(name);
        let first = !m.shaders_ok.iter().any(|s| s == name);
        if !m.watch.changed(&path) && !first {
            continue;
        }
        let text = mods::read_or(&path, default);
        let (source, err) = match mods::check_wgsl(&text) {
            Ok(()) => (Some(text), None),
            // Nothing good loaded yet: the built-in shader stands in.
            Err(e) if first => (Some(default.to_string()), Some(e)),
            Err(e) => (None, Some(e)),
        };
        if let Some(source) = source {
            if shaders.insert(handle.id(), Shader::from_wgsl(source, format!("warp-core/{name}"))).is_ok() {
                if first {
                    m.shaders_ok.push(name.to_string());
                }
                if err.is_none() && !first {
                    m.notice = Some((format!("{name} loaded"), now));
                }
            }
        }
        m.errors.retain(|(f, _)| f != name);
        if let Some(e) = err {
            eprintln!("warp-core: {name} refused:\n{e}");
            m.errors.push((name.to_string(), mods::short(&e)));
        }
    }
}

// ---------------------------------------------------------------- systems

fn read_feed(feed: Res<Feed>, mut deck: ResMut<Deck>, mut chat: ResMut<Chat>, mods: Res<Mods>, time: Res<Time>) {
    let now = time.elapsed_secs();
    let Ok(rx) = feed.0.lock() else { return };
    while let Ok(msg) = rx.try_recv() {
        match msg {
            Msg::Seats(seats) => {
                deck.seats = seats;
                deck.problem = None;
            }
            Msg::Problem(p) => deck.problem = Some(p),
            Msg::Chat(n) => {
                chat.notes.push_back(n);
                let over = chat.notes.len().saturating_sub(mods.cfg.chat.keep.max(10));
                chat.notes.drain(..over);
                // Scrolled back: stay on the same notes as new ones arrive.
                if chat.scroll > 0 {
                    chat.scroll += 1;
                }
                chat.dirty = true;
            }
            Msg::Note(n) => {
                deck.arrivals.push_back(now);
                deck.history.push_front(Seen { at: now, from: n.from.clone(), to: n.to.clone(), all: n.all });
                deck.history.truncate(300);
                deck.pending.push(n);
            }
        }
    }
    // With no ring to fly between, notes wait, but not without end.
    let over = deck.pending.len().saturating_sub(200);
    deck.pending.drain(..over);
    while deck.arrivals.front().is_some_and(|t| now - *t > 3600.0) {
        deck.arrivals.pop_front();
    }
    deck.warp = (deck.warp - time.delta_secs() * mods.cfg.warp.decay).max(0.0);
    deck.thrust = (deck.thrust - time.delta_secs() * mods.cfg.typing.decay).max(0.0);
}

fn apply_rules(deck: Res<Deck>, mods: Res<Mods>, mut looks: ResMut<Looks>) {
    let focus = deck.focus.as_deref();
    looks.0 = deck
        .seats
        .iter()
        .filter(|s| s.online())
        .map(|s| (s.name.clone(), seat_look(&mods.rules, s, focus)))
        .collect();
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
fn orbit_speed(cfg: &mods::OrbitCfg, notes_10min: usize, warp: f32) -> f32 {
    cfg.drift + (notes_10min.min(30) as f32) * cfg.traffic + warp * cfg.warp_swing
}

/// A seat's z: behind the core (1.0) at the back of the orbit, in front at
/// the near side, sliding smoothly rather than popping at the sides.
fn seat_z(depth: f32) -> f32 {
    0.5 + 1.5 * depth
}

fn window_size(windows: &Query<&Window>) -> Vec2 {
    windows.iter().next().map(|w| w.size()).unwrap_or(Vec2::new(1280.0, 800.0))
}

/// Splits the window into the scene and the chat panel, and moves the
/// camera so the core sits in the middle of the scene.
fn layout_view(
    windows: Query<&Window>,
    mods: Res<Mods>,
    mut layout: ResMut<Layout>,
    mut chat: ResMut<Chat>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
) {
    let w = window_size(&windows);
    let panel = panel_width(w.x, &mods.cfg.chat);
    layout.scene = Vec2::new(w.x - panel, w.y);
    layout.panel = panel;
    if (chat.drawn_width - panel).abs() > 0.5 {
        chat.drawn_width = panel;
        chat.dirty = true;
    }
    for mut tf in &mut camera {
        tf.translation.x = panel * 0.5;
    }
}

/// Builds the ring when the seat list changes, and keeps it fitted to the
/// window when that is resized.
#[allow(clippy::too_many_arguments)]
fn layout_seats(
    mut commands: Commands,
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut rings: ResMut<Assets<RingMaterial>>,
    layout: Res<Layout>,
    time: Res<Time>,
    mut nodes: Query<(Entity, &mut SeatNode, &mut Transform)>,
) {
    let pace = deck.notes_last(time.elapsed_secs(), 600.0);
    let warp = deck.warp;
    deck.orbit = (deck.orbit + time.delta_secs() * orbit_speed(&mods.cfg.orbit, pace, warp)) % std::f32::consts::TAU;
    let names: Vec<String> = deck.seats.iter().filter(|s| s.online()).map(|s| s.name.clone()).collect();
    let pos = ring_positions(&names, layout.scene, deck.orbit);
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
                    Transform::from_xyz(p.x, p.y, seat_z(depth)),
                ))
                .with_children(|seat| {
                    seat.spawn((
                        SeatLabel,
                        Text2d::new(label_for(name)),
                        TextFont { font_size: FontSize::Px(17.0), ..default() },
                        TextColor(seat_colour(name, 0.9)),
                        Anchor::TOP_CENTER,
                        Transform::from_xyz(0.0, -52.0, 0.1),
                    ));
                    for (k, d) in FUEL_D.iter().enumerate() {
                        seat.spawn((
                            FuelRing(k),
                            Mesh2d(assets.quad.clone()),
                            MeshMaterial2d(rings.add(RingMaterial::default())),
                            Transform::from_xyz(0.0, 0.0, -0.02).with_scale(Vec3::new(*d, *d, 1.0)),
                            Visibility::Hidden,
                        ));
                    }
                    seat.spawn((
                        HaloRing,
                        Mesh2d(assets.quad.clone()),
                        MeshMaterial2d(rings.add(RingMaterial::default())),
                        Transform::from_xyz(0.0, 0.0, -0.03).with_scale(Vec3::new(HALO_D, HALO_D, 1.0)),
                        Visibility::Hidden,
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
            tf.translation.z = seat_z(*depth);
            node.depth = *depth;
        }
    }
}

/// The seat under the pointer, and clicks to focus one.
#[allow(clippy::too_many_arguments)]
fn pointer(
    mut deck: ResMut<Deck>,
    mut chat: ResMut<Chat>,
    layout: Res<Layout>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    nodes: Query<(&SeatNode, &GlobalTransform)>,
) {
    let world = windows.iter().next().and_then(|w| w.cursor_position()).and_then(|c| {
        let (cam, tf) = cameras.iter().next()?;
        cam.viewport_to_world_2d(tf, c).ok()
    });
    let over_panel = world.is_some_and(|w| layout.in_panel(w));
    // The wheel over the panel scrolls the notes, a note a notch.
    for ev in wheel.read() {
        if over_panel {
            let n = chat.notes.len().saturating_sub(1);
            chat.scroll = if ev.y > 0.0 { (chat.scroll + 1).min(n) } else { chat.scroll.saturating_sub(1) };
            chat.dirty = true;
        }
    }
    if over_panel {
        deck.cursor = None;
        deck.hover = None;
        if mouse.just_pressed(MouseButton::Left) {
            chat.focus = true;
        }
        return;
    }
    deck.cursor = world;
    deck.hover = world.and_then(|w| {
        nodes
            .iter()
            .map(|(n, tf)| (n, tf.translation().truncate().distance(w), tf.scale().x))
            .filter(|(_, d, s)| *d <= 48.0 * s.max(0.5))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _, _)| n.name.clone())
    });
    if mouse.just_pressed(MouseButton::Left) {
        deck.focus = match (&deck.hover, &deck.focus) {
            (Some(h), Some(f)) if h == f => None,
            (Some(h), _) => Some(h.clone()),
            (None, _) => None,
        };
        // Clicking a seat also addresses the chat box to it.
        if let Some(h) = deck.hover.clone().filter(|h| h != "owner") {
            chat.input = chat::address(&chat.input, &h);
            chat.focus = true;
        }
    }
}

#[allow(clippy::type_complexity)]
fn glow_seats(
    deck: Res<Deck>,
    mods: Res<Mods>,
    looks: Res<Looks>,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut nodes: Query<(&mut SeatNode, &MeshMaterial2d<ColorMaterial>, &mut Transform, &Children)>,
    mut labels: Query<&mut Text2d, With<SeatLabel>>,
) {
    let t = time.elapsed_secs();
    let cfg = &mods.cfg;
    for (mut node, mat, mut tf, children) in &mut nodes {
        let Some(seat) = deck.seat(&node.name) else { continue };
        let look = looks.0.get(&node.name).cloned().unwrap_or_default();
        node.flash = (node.flash - time.delta_secs() * 1.6).max(0.0);
        let mut b = cfg.brightness(&seat.shown) * look.glow;
        if seat.shown == "busy" {
            b *= 0.85 + 0.15 * (t * 3.0 + node.name.len() as f32).sin();
        }
        b *= 0.55 + 0.45 * node.depth; // the far side of the orbit is dimmer
        if deck.focus.as_ref().is_some_and(|f| *f != node.name) {
            b *= 0.3;
        }
        b += node.flash * 6.0;
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.color = lin(look.tint.unwrap_or_else(|| seat_rgb(&node.name)), b);
        }
        let hovered = deck.hover.as_ref() == Some(&node.name);
        tf.scale = Vec3::splat(
            cfg.seats.size * (0.72 + 0.4 * node.depth) * (1.0 + node.flash * 0.35) * if hovered { 1.12 } else { 1.0 },
        );
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                let mut line = format!("{}\n{}", label_for(&node.name), seat.shown);
                if seat.waiting > 0 {
                    line.push_str(&format!(" | {} waiting", seat.waiting));
                }
                for l in &look.labels {
                    line.push('\n');
                    line.push_str(l);
                }
                if text.0 != line {
                    text.0 = line;
                }
            }
        }
    }
}

/// The fuel rings (meters) and the rule halos, through ring.wgsl.
#[allow(clippy::type_complexity)]
fn draw_rings(
    deck: Res<Deck>,
    mods: Res<Mods>,
    view: Res<View>,
    looks: Res<Looks>,
    time: Res<Time>,
    nodes: Query<(&SeatNode, &Children)>,
    mut fuel: Query<(&FuelRing, &MeshMaterial2d<RingMaterial>, &mut Visibility), Without<HaloRing>>,
    mut halos: Query<(&MeshMaterial2d<RingMaterial>, &mut Visibility, &mut Transform), With<HaloRing>>,
    mut rings: ResMut<Assets<RingMaterial>>,
) {
    let t = time.elapsed_secs();
    let f = &mods.cfg.fuel;
    for (node, children) in &nodes {
        let Some(seat) = deck.seat(&node.name) else { continue };
        let dim = if deck.focus.as_ref().is_some_and(|x| *x != node.name) { 0.3 } else { 1.0 };
        let meters = [seat.ctx, seat.five_hour, seat.week];
        let look = looks.0.get(&node.name);
        for child in children.iter() {
            if let Ok((ring, mat, mut vis)) = fuel.get_mut(child) {
                let Some(pct) = meters[ring.0].filter(|_| f.show && view.fuel) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                *vis = Visibility::Inherited;
                let pct = pct as f32;
                let colour = if pct >= f.danger {
                    f.danger_colour
                } else if pct >= f.warn {
                    f.warn_colour
                } else {
                    f.ok_colour
                };
                let half = FUEL_D[ring.0] * 0.5;
                if let Some(mut m) = rings.get_mut(&mat.0) {
                    m.u = RingUniform {
                        colour: v4(colour, dim) * Vec4::new(dim, dim, dim, 1.0),
                        back: v4(colour, f.empty * dim),
                        params: Vec4::new(pct / 100.0, 3.0 / half, t, if pct >= f.danger { 1.0 } else { 0.0 }),
                    };
                }
            } else if let Ok((mat, mut vis, mut tf)) = halos.get_mut(child) {
                let Some(h) = look.and_then(|l| l.halo.as_ref()) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                *vis = Visibility::Inherited;
                let d = HALO_D * h.size.max(0.1);
                tf.scale = Vec3::new(d, d, 1.0);
                if let Some(mut m) = rings.get_mut(&mat.0) {
                    let c = h.colour.map(|v| v * 1.6 * h.bright * dim);
                    m.u = RingUniform {
                        colour: v4(c, 0.9 * dim),
                        back: Vec4::ZERO,
                        params: Vec4::new(1.0, 5.0 / (d * 0.5), t, h.pulse),
                    };
                }
            }
        }
    }
}

/// The core breathes faster the more notes fly, and runs hotter as the
/// crew's meters fill.
fn pulse_core(
    deck: Res<Deck>,
    mods: Res<Mods>,
    time: Res<Time>,
    mut core: Query<(&mut Transform, &MeshMaterial2d<CoreMaterial>), With<Core>>,
    mut core_materials: ResMut<Assets<CoreMaterial>>,
    mut gizmos: Gizmos,
) {
    let c = &mods.cfg.core;
    let t = time.elapsed_secs();
    let pace = deck.notes_last(t, 600.0) as f32;
    let rate = 0.8 + pace.min(30.0) * 0.25;
    let s = c.size * (1.0 + c.breath * (t * rate).sin() + deck.warp * 0.25);
    let heat = crew_heat(&deck.seats);
    for (mut tf, mat) in &mut core {
        tf.scale = Vec3::new(170.0 * s, 170.0 * s, 1.0);
        if let Some(mut m) = core_materials.get_mut(&mat.0) {
            m.u = CoreUniform {
                cool: v4(c.cool, 1.0),
                hot: v4(c.hot, 1.0),
                params: Vec4::new(t, heat, deck.warp, pace.min(30.0) / 30.0),
            };
        }
    }
    // Two containment rings turning against each other, coloured by heat.
    let shell = mix3(c.cool, c.hot, heat);
    for (r, dir, k) in [(78.0, 1.0, 0.6), (96.0, -1.0, 0.35)] {
        let spin = t * dir * (0.4 + rate * 0.2) * c.ring_spin;
        for i in 0..3 {
            let start = spin + i as f32 * std::f32::consts::TAU / 3.0;
            gizmos.arc_2d(Isometry2d::from_rotation(Rot2::radians(start)), 1.4, r * s, lin(shell, k * 0.75));
        }
    }
}

/// 0..1: the mean of the online seats' fullest meters (seats without
/// usage files don't count either way).
fn crew_heat(seats: &[SeatInfo]) -> f32 {
    let full: Vec<f64> = seats.iter().filter(|s| s.online()).filter_map(|s| s.fullest()).collect();
    if full.is_empty() {
        return 0.0;
    }
    (full.iter().sum::<f64>() / full.len() as f64 / 100.0).clamp(0.0, 1.0) as f32
}

/// Faint conduits from each seat to the core; busy ones carry light.
fn draw_links(deck: Res<Deck>, mods: Res<Mods>, nodes: Query<(&SeatNode, &Transform)>, time: Res<Time>, mut gizmos: Gizmos) {
    let t = time.elapsed_secs();
    for (node, tf) in &nodes {
        let Some(seat) = deck.seat(&node.name) else { continue };
        let p = tf.translation.truncate();
        let inner = p.normalize_or_zero() * 70.0;
        let dim = if deck.focus.as_ref().is_some_and(|f| *f != node.name) { 0.25 } else { 1.0 };
        let b = ((mods.cfg.brightness(&seat.shown) * 0.18).min(0.6) + node.flash) * dim;
        gizmos.line_gradient_2d(inner, p * 0.9, seat_colour(&node.name, b), seat_colour(&node.name, b * 0.4));
        if seat.shown == "busy" {
            // A packet of light running down the conduit.
            let k = (t * 0.7 + node.name.len() as f32 * 0.13).fract();
            gizmos.circle_2d(Isometry2d::from_translation(p * 0.9 - (p * 0.9 - inner) * k), 2.5, seat_colour(&node.name, 3.0 * dim));
        }
    }
}

/// Backlog moons, rule beacons and shockwaves.
fn draw_moons_and_marks(
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    view: Res<View>,
    looks: Res<Looks>,
    time: Res<Time>,
    nodes: Query<(&SeatNode, &Transform)>,
    mut gizmos: Gizmos,
) {
    let t = time.elapsed_secs();
    let mc = &mods.cfg.moons;
    let positions: HashMap<&str, (Vec2, f32)> =
        nodes.iter().map(|(n, tf)| (n.name.as_str(), (tf.translation.truncate(), tf.scale.x))).collect();
    for (node, tf) in &nodes {
        let Some(seat) = deck.seat(&node.name) else { continue };
        let p = tf.translation.truncate();
        let s = tf.scale.x;
        let dim = if deck.focus.as_ref().is_some_and(|f| *f != node.name) { 0.25 } else { 1.0 };
        if mc.show && view.moons && seat.waiting > 0 {
            let n = (seat.waiting as usize).min(mc.max.max(1));
            let age = (seat.oldest_waiting_s as f32 / 3600.0 / mc.old_hours.max(0.01)).clamp(0.0, 1.0);
            let c = mix3(mc.fresh, mc.old, age);
            // Up to 12 to an orbit, further orbits outside it.
            for i in 0..n {
                let band = i / 12;
                let in_band = (n - band * 12).min(12);
                let r = (56.0 + band as f32 * 9.0) * s;
                let a = t * mc.speed * if band % 2 == 0 { 1.0 } else { -0.8 } + (i % 12) as f32 / in_band as f32 * std::f32::consts::TAU;
                let q = p + Vec2::new(a.cos() * r, a.sin() * r * 0.8);
                gizmos.circle_2d(Isometry2d::from_translation(q), 1.6 * s, lin(c, dim));
            }
        }
        if let Some((c, pulse)) = looks.0.get(&node.name).and_then(|l| l.beacon) {
            let k = if pulse > 0.0 { 0.5 + 0.5 * (t * 4.0 * pulse).sin() } else { 1.0 };
            let q = p + Vec2::new(0.0, 40.0 * s);
            gizmos.rect_2d(
                Isometry2d::new(q, Rot2::radians(std::f32::consts::FRAC_PI_4)),
                Vec2::splat(9.0 * s),
                lin(c, (1.0 + 3.0 * k) * dim),
            );
        }
    }
    deck.shocks.retain(|sh| t - sh.at < 1.2);
    for sh in &deck.shocks {
        let Some((p, s)) = positions.get(sh.seat.as_str()) else { continue };
        let k = (t - sh.at) / 1.2;
        let r = (30.0 + k * 120.0 * sh.size) * s;
        gizmos.circle_2d(Isometry2d::from_translation(*p), r, lin(sh.colour, 3.0 * (1.0 - k)));
    }
}

fn bezier(a: Vec2, b: Vec2, c: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    a * u * u + b * 2.0 * u * t + c * t * t
}

#[allow(clippy::too_many_arguments)]
fn fly_comets(
    mut commands: Commands,
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
    mut comets: Query<(Entity, &mut Comet, &mut Transform), Without<SeatNode>>,
    mut nodes: Query<(&mut SeatNode, &Transform), Without<Comet>>,
    mut gizmos: Gizmos,
) {
    let positions: HashMap<String, Vec2> =
        nodes.iter().map(|(n, tf)| (n.name.clone(), tf.translation.truncate())).collect();

    // Launch what arrived. With no ring (before the first status poll, or
    // when every seat is offline) notes wait rather than vanish or burst
    // from the core (Vibe's reviews of #150).
    if !positions.is_empty() {
        let pending = std::mem::take(&mut deck.pending);
        for note in pending {
            let look = note_look(&mods.rules, &mods.cfg, &note);
            deck.warp = deck.warp.max(look.warp);
            let from = positions.get(&note.from).copied().unwrap_or(Vec2::ZERO);
            let targets: Vec<String> = if note.all {
                positions.keys().filter(|k| **k != note.from).cloned().collect()
            } else {
                note.to.clone()
            };
            let involved = |seat: &str| deck.focus.as_deref().is_none_or(|f| f == seat || f == note.from);
            for to in targets {
                let Some(to_pos) = positions.get(&to).copied() else { continue };
                // Through the core, bowed a little so two-way traffic doesn't overlap.
                let via = (from + to_pos) * 0.12 + (to_pos - from).perp().normalize_or_zero() * 40.0;
                let bright = look.bright * if involved(&to) { 1.0 } else { 0.12 };
                let colour = lin(look.colour, bright);
                commands.spawn((
                    Comet {
                        from,
                        via,
                        to: to.clone(),
                        to_pos,
                        t: 0.0,
                        seconds: look.seconds,
                        colour,
                        trail: VecDeque::new(),
                        shock: look.shock,
                    },
                    Mesh2d(assets.dot.clone()),
                    MeshMaterial2d(materials.add(colour)),
                    Transform::from_xyz(from.x, from.y, 3.0).with_scale(Vec3::splat(look.size)),
                ));
            }
        }
    }

    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let trail = mods.cfg.comets.trail.clamp(2, 200);
    for (e, mut comet, mut tf) in &mut comets {
        // Follow the receiver round the orbit mid-flight.
        if let Some(p) = positions.get(&comet.to) {
            comet.to_pos = *p;
        }
        comet.t += dt / comet.seconds;
        let eased = 1.0 - (1.0 - comet.t.min(1.0)).powi(2);
        let p = bezier(comet.from, comet.via, comet.to_pos, eased);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        comet.trail.push_front(p);
        comet.trail.truncate(trail);
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
            if let Some((colour, size)) = comet.shock {
                deck.shocks.push(Shock { seat: comet.to.clone(), at: now, colour, size });
            }
            commands.entity(e).despawn();
        }
    }
}

/// Stars stream out from the core; in warp they stretch into lines.
fn move_stars(
    deck: Res<Deck>,
    mods: Res<Mods>,
    time: Res<Time>,
    layout: Res<Layout>,
    windows: Query<&Window>,
    mut rng: ResMut<Rng>,
    mut stars: Query<(&mut Star, &mut Transform)>,
) {
    let sc = &mods.cfg.stars;
    // The sky fills the whole window (the panel covers its right side).
    let half = Vec2::new(layout.scene.x * 0.5 + layout.panel, window_size(&windows).y * 0.5);
    let reach = half.length() + 20.0;
    let pace = (deck.notes_last(time.elapsed_secs(), 600.0) as f32).min(30.0);
    let tc = &mods.cfg.typing;
    let speed = sc.speed + pace * 2.0 + deck.warp * sc.warp_speed + deck.thrust * tc.star_speed;
    let dt = time.delta_secs();
    for (mut star, mut tf) in &mut stars {
        star.dist += speed * star.speed * dt * (0.3 + star.dist / reach);
        if star.dist > reach {
            star.dir = Vec2::from_angle(rng.next() * std::f32::consts::TAU);
            star.dist = 30.0 + rng.next() * 120.0;
        }
        let p = star.dir * star.dist;
        let near = (star.dist / reach).clamp(0.0, 1.0);
        let stretch = 1.0 + (deck.warp * sc.stretch + deck.thrust * tc.stretch) * near + pace * 0.1;
        tf.translation = Vec3::new(p.x, p.y, 0.0);
        tf.rotation = Quat::from_rotation_z(star.dir.to_angle());
        tf.scale = Vec3::new(stretch * (1.0 + near), 0.6 + near, 1.0);
    }
}

#[allow(clippy::type_complexity)]
fn hud(
    deck: Res<Deck>,
    mods: Res<Mods>,
    view: Res<View>,
    time: Res<Time>,
    real: Res<Time<Real>>,
    vtime: Res<Time<Virtual>>,
    layout: Res<Layout>,
    mut hud: Query<(&mut Text2d, &mut Transform), (With<Hud>, Without<Dock>)>,
    mut dock: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<Dock>, Without<Hud>)>,
) {
    let half = layout.half();
    let now = time.elapsed_secs();
    let online: Vec<_> = deck.seats.iter().filter(|s| s.online()).collect();
    let busy = online.iter().filter(|s| s.shown == "busy").count();
    // Every seat's queue, on the ring or not (Vibe's review of #150).
    let waiting: u64 = deck.seats.iter().map(|s| s.waiting).sum();
    let mut line = String::new();
    if mods.cfg.hud.show && view.hud {
        line = format!(
            "WARP CORE    {} online | {} busy | {} waiting | {} notes in the last hour",
            online.len(),
            busy,
            waiting,
            deck.notes_last(now, 3600.0),
        );
        if deck.warp > 0.05 {
            line.push_str("    >> WARP");
        } else if deck.thrust > 0.05 {
            line.push_str(&format!("    thrust {:.0}%", deck.thrust * 100.0));
        }
        if vtime.is_paused() {
            line.push_str("    || FROZEN (f)");
        }
        if let Some(f) = &deck.focus {
            line.push_str(&format!("    * {} (click empty space to clear)", label_for(f)));
        }
        let shares = context_shares(&deck.seats);
        if mods.cfg.stars.by_context && !shares.is_empty() {
            let split: Vec<String> =
                shares.iter().map(|(n, s)| format!("{} {:.0}%", label_for(n), s * 100.0)).collect();
            line.push_str(&format!("\nstars = context: {}", split.join(" | ")));
        }
        if let Some(p) = &deck.problem {
            line.push_str(&format!("\n{p}"));
        }
        if let Some((n, at)) = &mods.notice {
            if real.elapsed_secs() - at < 6.0 {
                line.push_str(&format!("\nmods: {n}"));
            }
        }
    }
    // Refused mods always show: they are why the screen isn't changing.
    for (file, why) in &mods.errors {
        line.push_str(&format!("\nmods: {file} refused, keeping the last good one: {why}"));
    }
    for (mut text, mut tf) in &mut hud {
        if text.0 != line {
            text.0 = line.clone();
        }
        tf.translation.x = -half.x + 18.0;
        tf.translation.y = half.y - 14.0;
    }

    // The dock: seats off the ring, and the queues they're holding.
    let off: Vec<String> = deck
        .seats
        .iter()
        .filter(|s| !s.online())
        .map(|s| {
            let mut t = format!("{} {}", label_for(&s.name), s.shown);
            if s.waiting > 0 {
                t.push_str(&format!(" {} (oldest {})", s.waiting, ago(s.oldest_waiting_s)));
            }
            t
        })
        .collect();
    let show = mods.cfg.hud.dock && view.hud && !off.is_empty();
    let dock_line = if show {
        // Three to a line, so it fits a half-width window.
        let rows: Vec<String> = off.chunks(3).map(|c| c.join("   |   ")).collect();
        format!("off the ring: {}\nhover a seat | click to focus | 1-9 | h c m f r | space: test", rows.join("\n              "))
    } else {
        String::new()
    };
    for (mut text, mut tf, mut vis) in &mut dock {
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
        if text.0 != dock_line {
            text.0 = dock_line.clone();
        }
        tf.translation.x = -half.x + 18.0;
        tf.translation.y = -half.y + 12.0;
    }
}

/// The hover card: a seat's state, queue, meters, and its last few notes
/// (who and when only; what they said stays in the deck).
fn hover_card(deck: &Deck, name: &str, now: f32) -> Option<String> {
    let s = deck.seat(name)?;
    let mut lines = vec![format!("{} | {} for {}", label_for(name), s.shown, ago(s.since_s))];
    if s.waiting > 0 {
        lines.push(format!("{} waiting, oldest {}", s.waiting, ago(s.oldest_waiting_s)));
    }
    let pct = |p: Option<f64>| p.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "-".into());
    if s.fullest().is_some() {
        lines.push(format!("ctx {} | 5h {} | week {}", pct(s.ctx), pct(s.five_hour), pct(s.week)));
    }
    if s.held_by_draft {
        lines.push("drafting a note".into());
    }
    let recent: Vec<String> = deck
        .history
        .iter()
        .filter(|n| n.from == name || n.all || n.to.iter().any(|t| t == name))
        .take(4)
        .map(|n| {
            let when = ago((now - n.at) as f64);
            if n.from == name {
                let to = if n.all { "all".into() } else { n.to.iter().map(|t| label_for(t)).collect::<Vec<_>>().join(", ") };
                format!("  -> {to}  {when} ago")
            } else {
                format!("  <- {}  {when} ago", label_for(&n.from))
            }
        })
        .collect();
    if !recent.is_empty() {
        lines.push("recent notes:".into());
        lines.extend(recent);
    }
    Some(lines.join("\n"))
}

fn tooltip(
    deck: Res<Deck>,
    time: Res<Time>,
    layout: Res<Layout>,
    mut tip: Query<(&mut Text2d, &mut Transform, &mut Visibility, &mut Anchor), With<Tooltip>>,
) {
    let half = layout.half();
    let card = deck.hover.as_deref().and_then(|h| hover_card(&deck, h, time.elapsed_secs()));
    for (mut text, mut tf, mut vis, mut anchor) in &mut tip {
        let (Some(card), Some(at)) = (card.as_ref(), deck.cursor) else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        if text.0 != *card {
            text.0 = card.clone();
        }
        // Flip to the cursor's other side near the right or bottom edge.
        let right = at.x > half.x - 300.0;
        let low = at.y < -half.y + 160.0;
        *anchor = match (right, low) {
            (false, false) => Anchor::TOP_LEFT,
            (true, false) => Anchor::TOP_RIGHT,
            (false, true) => Anchor::BOTTOM_LEFT,
            (true, true) => Anchor::BOTTOM_RIGHT,
        };
        let off = Vec2::new(if right { -18.0 } else { 18.0 }, if low { 18.0 } else { -18.0 });
        tf.translation = (at + off).extend(6.0);
    }
}

fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<Chat>,
    mut deck: ResMut<Deck>,
    mut mods: ResMut<Mods>,
    mut view: ResMut<View>,
    mut vtime: ResMut<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if ctrl && keys.just_pressed(KeyCode::KeyQ) {
        exit.write(AppExit::Success);
    }
    // While the chat box has the keys, letters are typing, not commands.
    // (Esc no longer quits: it clears the chat box.)
    if chat.focus {
        return;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        exit.write(AppExit::Success);
    }
    if keys.just_pressed(KeyCode::Space) {
        // A test broadcast from Y: nothing is sent, it only flies here.
        deck.pending.push(NoteInfo { from: "owner".into(), all: true, ..Default::default() });
    }
    if keys.just_pressed(KeyCode::KeyH) {
        view.hud = !view.hud;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        view.fuel = !view.fuel;
    }
    if keys.just_pressed(KeyCode::KeyM) {
        view.moons = !view.moons;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        if vtime.is_paused() {
            vtime.unpause();
        } else {
            vtime.pause();
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        // Forget what was seen, so every mod file loads again.
        mods.watch = mods::Watch::default();
        mods.since_check = 1.0;
    }
    let digits = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    for (i, k) in digits.iter().enumerate() {
        if keys.just_pressed(*k) {
            deck.focus = deck.built.get(i).cloned();
        }
    }
    if keys.just_pressed(KeyCode::Digit0) {
        deck.focus = None;
    }
}

// ---------------------------------------------------------------- stars by context

/// Each seat's share of the crew's context tokens, in the service's order;
/// seats without a figure are left out (as in the deck's split).
fn context_shares(seats: &[SeatInfo]) -> Vec<(String, f32)> {
    let have: Vec<(&str, f64)> =
        seats.iter().filter_map(|s| s.ctx_tokens.filter(|t| *t > 0.0).map(|t| (s.name.as_str(), t))).collect();
    let total: f64 = have.iter().map(|(_, t)| t).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    have.into_iter().map(|(n, t)| (n.to_string(), (t / total) as f32)).collect()
}

/// Which seat a star's slot (0..1) falls to, walking the shares in order.
fn seat_for_slot(shares: &[(String, f32)], slot: f32) -> Option<&str> {
    let mut edge = 0.0;
    for (name, share) in shares {
        edge += share;
        if slot < edge {
            return Some(name);
        }
    }
    shares.last().map(|(n, _)| n.as_str())
}

/// Gives each star the colour of the seat whose share of the context its
/// slot falls in, so the sky is split the way the crew's context is.
fn colour_stars(
    deck: Res<Deck>,
    mods: Res<Mods>,
    mut mats: ResMut<StarMats>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut stars: Query<(&Star, &mut MeshMaterial2d<ColorMaterial>)>,
) {
    let sc = &mods.cfg.stars;
    let shares = if sc.by_context { context_shares(&deck.seats) } else { Vec::new() };
    let key = format!(
        "{}|{:?}",
        sc.context_bright,
        shares.iter().map(|(n, s)| format!("{n}:{:.3}", s)).collect::<Vec<_>>()
    );
    if key == mats.key {
        return;
    }
    mats.key = key;
    for (name, _) in &shares {
        let colour = seat_colour(name, sc.context_bright);
        match mats.seats.get(name) {
            Some(h) => {
                if let Some(mut m) = materials.get_mut(h) {
                    m.color = colour;
                }
            }
            None => {
                let h = materials.add(colour);
                mats.seats.insert(name.clone(), h);
            }
        }
    }
    for (star, mut mat) in &mut stars {
        let want = seat_for_slot(&shares, star.slot)
            .and_then(|n| mats.seats.get(n))
            .unwrap_or(&mats.default)
            .clone();
        if mat.0 != want {
            mat.0 = want;
        }
    }
}

// ---------------------------------------------------------------- chat

/// Typing in the chat box, and the keys that move between it and the ring.
fn chat_input(
    mut chat: ResMut<Chat>,
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let n = chat.notes.len().saturating_sub(1);
    for ev in events.read() {
        if !ev.state.is_pressed() {
            continue;
        }
        // Scrolling works from either side.
        match ev.key_code {
            KeyCode::PageUp => {
                chat.scroll = (chat.scroll + 5).min(n);
                chat.dirty = true;
                continue;
            }
            KeyCode::PageDown => {
                chat.scroll = chat.scroll.saturating_sub(5);
                chat.dirty = true;
                continue;
            }
            _ => {}
        }
        if !chat.focus {
            // On the ring: Enter, Tab or @ come back to the box.
            let at = matches!(&ev.logical_key, Key::Character(c) if c.as_str() == "@");
            if matches!(ev.logical_key, Key::Enter | Key::Tab) || at {
                chat.focus = true;
                if at {
                    chat.input.push('@');
                }
            }
            continue;
        }
        match &ev.logical_key {
            Key::Enter if !ev.repeat => {
                let before = chat.sending;
                send_line(&mut chat, &deck, real.elapsed_secs());
                // A note went out: the thrust built while typing it jumps to warp.
                if chat.sending > before && mods.cfg.typing.jump_on_send {
                    deck.warp = deck.warp.max(deck.thrust);
                    deck.thrust = 0.0;
                }
            }
            Key::Escape => {
                if chat.input.is_empty() {
                    chat.focus = false;
                } else {
                    chat.input.clear();
                }
            }
            Key::Tab => chat.focus = false,
            Key::Backspace if ctrl => chat.input = chat::drop_word(&chat.input),
            Key::Backspace => {
                chat.input.pop();
            }
            Key::Insert if shift => {
                let p = chat::paste();
                chat.input.push_str(&p);
            }
            Key::Character(c) if ctrl => match c.to_lowercase().as_str() {
                "u" => chat.input.clear(),
                "w" => chat.input = chat::drop_word(&chat.input),
                "v" => {
                    let p = chat::paste();
                    chat.input.push_str(&p);
                }
                _ => {}
            },
            _ => {
                if let Some(text) = &ev.text {
                    if !ctrl {
                        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                        deck.thrust = add_thrust(deck.thrust, clean.chars().count(), mods.cfg.typing.kick);
                        chat.input.push_str(&clean);
                    }
                }
            }
        }
    }
}

/// Thrust after typing `keys` characters: each adds `kick`, up to full.
fn add_thrust(thrust: f32, keys: usize, kick: f32) -> f32 {
    (thrust + keys as f32 * kick).clamp(0.0, 1.0)
}

/// Routes the line and sends it on its own thread; the outcome comes back
/// through `chat.rx`. A line that doesn't route stays in the box.
fn send_line(chat: &mut Chat, deck: &Deck, now: f32) {
    let text = chat.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    let seats: Vec<String> = deck.seats.iter().map(|s| s.name.clone()).collect();
    match chat::route(&text, &seats) {
        Err(e) => chat.status = Some((e, true, now)),
        Ok(to) => {
            chat.input.clear();
            chat.scroll = 0;
            chat.sending += 1;
            chat.status = Some((format!("sending to {to}..."), false, now));
            let tx = chat.tx.clone();
            std::thread::spawn(move || {
                let r = chat::send(&to, &text);
                let _ = tx.send((text, r));
            });
        }
    }
}

fn chat_results(mut chat: ResMut<Chat>, real: Res<Time<Real>>) {
    let now = real.elapsed_secs();
    let got: Vec<SendResult> = match chat.rx.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    for (text, r) in got {
        chat.sending = chat.sending.saturating_sub(1);
        match r {
            Ok(say) => {
                let say = if say.is_empty() { "sent".to_string() } else { feed::scrub(&say) };
                chat.status = Some((say, false, now));
            }
            Err(e) => {
                chat.status = Some((format!("not sent: {}", feed::scrub(&e)), true, now));
                // Nothing typed is lost: a failed line comes back to the box.
                if chat.input.is_empty() {
                    chat.input = text;
                }
            }
        }
    }
}

fn clock(time: &str) -> &str {
    time.get(11..16).unwrap_or("")
}

fn to_label(to: &str) -> String {
    to.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| if p.starts_with('-') { p.to_string() } else { label_for(&feed::seat_id(p)) })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The panel's spans for the notes shown: newest last, `scroll` notes back.
fn chat_spans(notes: &VecDeque<ChatNote>, scroll: usize, cap: usize) -> Vec<(String, Color)> {
    let end = notes.len().saturating_sub(scroll);
    let start = end.saturating_sub(cap);
    let dim = Color::srgb(0.42, 0.47, 0.58);
    let mut out = Vec::new();
    for (i, n) in notes.range(start..end).enumerate() {
        if i > 0 {
            out.push(("\n".to_string(), dim));
        }
        out.push((format!("{} ", clock(&n.time)), dim));
        out.push((label_for(&n.from), seat_colour(&n.from, 1.1)));
        out.push((" -> ".into(), dim));
        let first = n.to.split([',', ' ']).find(|p| !p.is_empty()).map(feed::seat_id).unwrap_or_default();
        let to_colour = if first == "all" { Color::srgb(0.8, 0.82, 0.9) } else { seat_colour(&first, 1.0) };
        out.push((to_label(&n.to), to_colour));
        let mut text: String = n.text.chars().take(1500).collect();
        if n.text.chars().count() > 1500 {
            text.push_str("...");
        }
        if n.quiet {
            out.push(("  (quiet) ".into(), dim));
            out.push((text, Color::srgb(0.6, 0.64, 0.72)));
        } else {
            out.push(("  ".into(), dim));
            out.push((text, Color::srgb(0.88, 0.9, 0.96)));
        }
    }
    out
}

/// Lays out the panel each frame, and rebuilds the note spans when they
/// changed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn draw_chat(
    mut commands: Commands,
    mut chat: ResMut<Chat>,
    deck: Res<Deck>,
    mods: Res<Mods>,
    layout: Res<Layout>,
    real: Res<Time<Real>>,
    mut panels: Query<(&mut Transform, &mut Visibility, Option<&ChatHeader>), Or<(With<ChatPanel>, With<ChatHeader>)>>,
    mut log: Query<(Entity, &mut Transform, &mut TextBounds, &mut Visibility, &mut TextFont), (With<ChatLog>, Without<ChatPanel>, Without<ChatHeader>, Without<ChatLine>)>,
    mut line: Query<(&mut Text2d, &mut Transform, &mut TextBounds, &mut Visibility), (With<ChatLine>, Without<ChatPanel>, Without<ChatHeader>, Without<ChatLog>, Without<ChatStatus>, Without<ChatTitle>)>,
    mut status: Query<(&mut Text2d, &mut TextColor, &mut Transform, &mut Visibility), (With<ChatStatus>, Without<ChatPanel>, Without<ChatHeader>, Without<ChatLog>, Without<ChatLine>, Without<ChatTitle>)>,
    mut title: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<ChatTitle>, Without<ChatPanel>, Without<ChatHeader>, Without<ChatLog>, Without<ChatLine>, Without<ChatStatus>)>,
) {
    let cfg = &mods.cfg.chat;
    let show = layout.panel > 0.0;
    let vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    let h = layout.scene.y;
    let left = layout.scene.x * 0.5;
    let pad = 14.0;
    let inner = (layout.panel - pad * 2.0).max(50.0);
    let header_h = 38.0;
    let input_h = 64.0;
    let bottom = -h * 0.5;
    let top = h * 0.5;

    for (mut tf, mut v, header) in &mut panels {
        *v = vis;
        if header.is_some() {
            tf.translation = Vec3::new(left + layout.panel * 0.5, top - header_h * 0.5, 9.0);
            tf.scale = Vec3::new(layout.panel, header_h, 1.0);
        } else {
            tf.translation = Vec3::new(left + layout.panel * 0.5, 0.0, 7.0);
            tf.scale = Vec3::new(layout.panel, h, 1.0);
        }
    }
    for (mut text, mut tf, mut v) in &mut title {
        *v = vis;
        let mut t = format!("COMMS   {} notes", chat.notes.len());
        if chat.scroll > 0 {
            t.push_str(&format!("   (scrolled back {}, PgDn)", chat.scroll));
        }
        if text.0 != t {
            text.0 = t;
        }
        tf.translation = Vec3::new(left + pad, top - 12.0, 10.0);
    }
    for (e, mut tf, mut bounds, mut v, mut font) in &mut log {
        *v = vis;
        tf.translation = Vec3::new(left + pad, bottom + input_h + 10.0, 8.0);
        if bounds.width != Some(inner) {
            bounds.width = Some(inner);
        }
        if font.font_size != FontSize::Px(cfg.font) {
            font.font_size = FontSize::Px(cfg.font);
            chat.dirty = true;
        }
        if chat.dirty {
            chat.dirty = false;
            commands.entity(e).despawn_related::<Children>();
            let spans = chat_spans(&chat.notes, chat.scroll, 60);
            commands.entity(e).with_children(|p| {
                for (text, colour) in spans {
                    p.spawn((TextSpan::new(text), TextColor(colour), TextFont { font_size: FontSize::Px(cfg.font), ..default() }));
                }
            });
        }
    }

    let now = real.elapsed_secs();
    let blink = (now * 2.0) as i64 % 2 == 0;
    for (mut text, mut tf, mut bounds, mut v) in &mut line {
        *v = vis;
        let t = if chat.input.is_empty() && !chat.focus {
            "> Enter or Tab to type a note".to_string()
        } else if chat.input.is_empty() {
            format!("> {}type a note: @seat to address it, none for all", if blink { "_" } else { " " })
        } else {
            format!("> {}{}", chat.input, if chat.focus && blink { "_" } else { " " })
        };
        if text.0 != t {
            text.0 = t;
        }
        if bounds.width != Some(inner) {
            bounds.width = Some(inner);
        }
        tf.translation = Vec3::new(left + pad, bottom + 12.0, 10.0);
    }
    for (mut text, mut colour, mut tf, mut v) in &mut status {
        *v = vis;
        let seats: Vec<String> = deck.seats.iter().map(|s| s.name.clone()).collect();
        let fresh = chat.status.as_ref().filter(|(_, _, at)| now - at < 8.0);
        let (t, err) = match fresh {
            Some((msg, err, _)) => (msg.clone(), *err),
            None if !chat.input.trim().is_empty() => match chat::route(&chat.input, &seats) {
                Ok(to) => (format!("to: {}", to_label(&to)), false),
                Err(e) => (e, true),
            },
            None if !chat.focus => ("keys drive the ring (Tab: back to typing)".into(), false),
            None => ("Enter sends as Y | Esc clears | Tab: ring keys".into(), false),
        };
        let t: String = t.chars().take(120).collect();
        if text.0 != t {
            text.0 = t;
        }
        colour.0 = if err { Color::srgb(1.0, 0.45, 0.4) } else { Color::srgb(0.5, 0.56, 0.68) };
        tf.translation = Vec3::new(left + pad, bottom + 42.0, 10.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(toml: &str) -> Vec<Rule> {
        mods::load_config(toml).unwrap().1
    }

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
        let cfg = mods::OrbitCfg::default();
        assert!(orbit_speed(&cfg, 0, 0.0) > 0.0, "never quite still");
        assert!(orbit_speed(&cfg, 20, 0.0) > orbit_speed(&cfg, 0, 0.0));
        assert!(orbit_speed(&cfg, 0, 1.0) > 10.0 * orbit_speed(&cfg, 0, 0.0));
    }

    #[test]
    fn seats_slide_behind_the_core_without_popping() {
        assert!(seat_z(0.0) < 1.0 && seat_z(1.0) > 1.0, "far side behind the core, near side in front");
        assert!((seat_z(0.51) - seat_z(0.49)).abs() < 0.05, "no jump at the sides");
    }

    #[test]
    fn busy_burns_brighter_than_away() {
        let cfg = Config::default();
        assert!(cfg.brightness("busy") > 1.0, "busy blooms");
        assert!(cfg.brightness("away") < cfg.brightness("ready"));
    }

    #[test]
    fn seat_rules_dress_the_seat() {
        let r = rules(
            "[[rule]]\nwhen = 'oldest_waiting_h >= 1'\neffect = 'halo'\ncolour = 'amber'\n\
             [[rule]]\nwhen = 'fullest >= 90'\neffect = 'beacon'\n\
             [[rule]]\nwhen = 'held_by_draft'\neffect = 'label'\ntext = 'drafting'\n\
             [[rule]]\nwhen = 'busy'\neffect = 'glow'\nbright = 2.0",
        );
        let s = SeatInfo {
            name: "vibe".into(),
            shown: "busy".into(),
            state: "busy".into(),
            oldest_waiting_s: 7200.0,
            week: Some(95.0),
            held_by_draft: true,
            ..Default::default()
        };
        let look = seat_look(&r, &s, None);
        assert_eq!(look.halo.unwrap().colour, [1.0, 0.55, 0.05]);
        assert!(look.beacon.is_some());
        assert_eq!(look.labels, ["drafting"]);
        assert_eq!(look.glow, 2.0);
        let calm = SeatInfo { name: "vibe".into(), shown: "idle".into(), ..Default::default() };
        let look = seat_look(&r, &calm, None);
        assert!(look.halo.is_none() && look.beacon.is_none() && look.labels.is_empty());
    }

    #[test]
    fn tint_takes_the_rule_colour() {
        let r = rules("[[rule]]\nwhen = 'true'\neffect = 'tint'\ncolour = 'cyan'");
        let s = SeatInfo { name: "grok".into(), ..Default::default() };
        assert_eq!(seat_look(&r, &s, None).tint, Some([0.2, 0.9, 1.0]));
    }

    #[test]
    fn note_rules_shape_the_comet() {
        let (cfg, r) = mods::load_config(
            "[[rule]]\non = 'note'\nwhen = 'to_owner'\neffect = 'comet'\ncolour = 'gold'\nsize = 2.0\n\
             [[rule]]\non = 'note'\nwhen = 'to_owner'\neffect = 'shockwave'\n\
             [[rule]]\non = 'note'\nwhen = 'len > 1000'\neffect = 'warp'\nstrength = 0.5",
        )
        .unwrap();
        let to_y = NoteInfo { from: "claude".into(), to: vec!["owner".into()], len: 2000, ..Default::default() };
        let look = note_look(&r, &cfg, &to_y);
        assert_eq!(look.colour, [1.0, 0.78, 0.2]);
        assert_eq!(look.size, 2.0);
        assert!(look.shock.is_some());
        assert_eq!(look.warp, 0.5);
        let plain = NoteInfo { from: "claude".into(), to: vec!["vibe".into()], ..Default::default() };
        let look = note_look(&r, &cfg, &plain);
        assert_eq!(look.colour, seat_rgb("claude"));
        assert!(look.shock.is_none() && look.warp == 0.0);
        let all = NoteInfo { from: "claude".into(), all: true, ..Default::default() };
        assert_eq!(note_look(&r, &cfg, &all).warp, 1.0, "broadcasts still warp");
    }

    #[test]
    fn the_core_runs_hotter_as_meters_fill() {
        let seat = |state: &str, ctx: Option<f64>| SeatInfo { state: state.into(), ctx, ..Default::default() };
        assert_eq!(crew_heat(&[]), 0.0);
        assert_eq!(crew_heat(&[seat("busy", None)]), 0.0, "no meters is no heat");
        assert_eq!(crew_heat(&[seat("busy", Some(20.0)), seat("idle", Some(80.0))]), 0.5);
        assert_eq!(crew_heat(&[seat("busy", Some(20.0)), seat("away", Some(100.0))]), 0.2, "offline seats don't count");
    }

    #[test]
    fn the_hover_card_names_who_and_when_but_not_what() {
        let mut deck = Deck::default();
        deck.seats.push(SeatInfo {
            name: "vibe".into(),
            shown: "busy".into(),
            state: "busy".into(),
            waiting: 2,
            oldest_waiting_s: 4800.0,
            since_s: 90.0,
            ctx: Some(45.0),
            ..Default::default()
        });
        deck.history.push_front(Seen { at: 10.0, from: "claude".into(), to: vec!["vibe".into()], all: false });
        deck.history.push_front(Seen { at: 40.0, from: "vibe".into(), to: vec!["owner".into()], all: false });
        deck.history.push_front(Seen { at: 50.0, from: "grok".into(), to: vec!["devin".into()], all: false });
        let card = hover_card(&deck, "vibe", 100.0).unwrap();
        assert!(card.starts_with("vibe | busy for 1m"));
        assert!(card.contains("2 waiting, oldest 1h 20m"));
        assert!(card.contains("ctx 45% | 5h - | week -"));
        assert!(card.contains("-> Y  1m ago") && card.contains("<- claude  1m ago"));
        assert!(!card.contains("grok"), "other seats' notes aren't on it");
        assert!(hover_card(&deck, "nobody", 0.0).is_none());
    }


    #[test]
    fn stars_split_the_way_the_context_does() {
        let seat = |n: &str, t: Option<f64>| SeatInfo { name: n.into(), ctx_tokens: t, ..Default::default() };
        let seats = [seat("claude", Some(60000.0)), seat("vibe", Some(20000.0)), seat("devin", Some(20000.0)), seat("grok", None)];
        let shares = context_shares(&seats);
        assert_eq!(shares.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["claude", "vibe", "devin"]);
        assert!((shares[0].1 - 0.6).abs() < 1e-6);
        assert_eq!(seat_for_slot(&shares, 0.1), Some("claude"));
        assert_eq!(seat_for_slot(&shares, 0.7), Some("vibe"));
        assert_eq!(seat_for_slot(&shares, 0.95), Some("devin"));
        assert_eq!(seat_for_slot(&shares, 1.0), Some("devin"), "the top edge belongs to the last seat");
        assert!(context_shares(&[seat("grok", None)]).is_empty(), "nobody reporting: plain stars");
        assert_eq!(seat_for_slot(&[], 0.5), None);
    }

    #[test]
    fn the_panel_keeps_a_readable_width() {
        let cfg = mods::ChatCfg::default();
        assert_eq!(panel_width(2000.0, &cfg), 600.0);
        assert_eq!(panel_width(1100.0, &cfg), 352.0);
        assert!((panel_width(400.0, &cfg) - 240.0).abs() < 0.01, "never more than 60% of a narrow window");
        assert_eq!(panel_width(1100.0, &mods::ChatCfg { show: false, ..cfg }), 0.0);
    }

    #[test]
    fn chat_lines_read_like_the_deck() {
        let mut notes = VecDeque::new();
        for (i, (from, to, quiet)) in [("claude", "all", false), ("owner", "vibe, devin", false), ("vibe", "owner", true)].iter().enumerate() {
            notes.push_back(ChatNote {
                from: from.to_string(),
                to: to.to_string(),
                time: format!("2026-11-27 03:0{i}:00"),
                text: format!("note {i}"),
                quiet: *quiet,
            });
        }
        let text: String = chat_spans(&notes, 0, 60).into_iter().map(|(t, _)| t).collect();
        assert_eq!(text, "03:00 claude -> all  note 0\n03:01 Y -> vibe, devin  note 1\n03:02 vibe -> Y  (quiet) note 2");
        let back: String = chat_spans(&notes, 1, 60).into_iter().map(|(t, _)| t).collect();
        assert!(back.ends_with("note 1"), "scrolled back one note");
        let capped: String = chat_spans(&notes, 0, 1).into_iter().map(|(t, _)| t).collect();
        assert_eq!(capped, "03:02 vibe -> Y  (quiet) note 2");
    }

    #[test]
    fn a_line_that_doesnt_route_stays_in_the_box() {
        let mut chat = Chat { input: "@nobody hi".into(), ..Default::default() };
        let mut deck = Deck::default();
        deck.seats.push(SeatInfo { name: "vibe".into(), ..Default::default() });
        send_line(&mut chat, &deck, 0.0);
        assert_eq!(chat.input, "@nobody hi");
        assert!(chat.status.as_ref().is_some_and(|(m, err, _)| *err && m.contains("@nobody")));
        assert_eq!(chat.sending, 0, "nothing went out");
    }

    #[test]
    fn typing_builds_thrust_up_to_full() {
        assert!((add_thrust(0.0, 10, 0.02) - 0.2).abs() < 1e-6);
        assert_eq!(add_thrust(0.9, 50, 0.02), 1.0, "capped at full");
        // A steady typist (5 keys a second) out-runs the drain (0.05 a second).
        let cfg = mods::TypingCfg::default();
        assert!(5.0 * cfg.kick > cfg.decay);
    }

    #[test]
    fn ages_read_short() {
        assert_eq!(ago(42.0), "42s");
        assert_eq!(ago(4800.0), "1h 20m");
        assert_eq!(ago(241499.0), "2d 19h");
    }
}
