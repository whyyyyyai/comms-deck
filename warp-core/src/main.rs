//! warp-core: the comms-deck drawn as a ship's reactor, on the GPU (Bevy).
//!
//! The seats sit in a ring around a pulsing core, each in its deck colour,
//! bright when busy and dim when away. Every note sent on the deck flies as a
//! comet from sender to receiver through the core; a broadcast to `all`
//! sends one to everyone and throws the starfield into warp. Around each
//! seat, fuel rings show its context / 5-hour / weekly meters and moons show
//! its waiting notes, whitening to red as the oldest one ages. The core runs
//! hotter as the crew's meters fill. The load balancer's task board shows
//! too: open tasks orbit the core on their own ring, a held task is a bolt
//! from its holder to the core, and both burn in the colour of whoever
//! assigned the task (feed.rs reads the board's state files directly).
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
//! HUD, c fuel rings, m moons, t tasks, f freeze, r reload mods, Space a test
//! broadcast, PgUp/PgDn scroll, Enter or Tab back to the box. No key
//! quits: it runs as a service (`systemctl --user stop warp-core`).
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
    text::{TextBounds, TextLayoutInfo},
    post_process::bloom::Bloom,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{Shader, ShaderRef},
    sprite::Anchor,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
    winit::{UpdateMode, WinitSettings},
};
use feed::{Activity, ChatNote, Msg, Notice, NoteInfo, SeatInfo, TaskInfo};
use mods::{Config, Rule, Val};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Mutex};

const STARS: usize = 260;
const CORE_SHADER: Handle<Shader> = uuid_handle!("6f1d2c3a-6b0e-4c55-9a1e-2f4c8d7b9e01");
const RING_SHADER: Handle<Shader> = uuid_handle!("0b9e3f4d-1a2c-4e6f-8b7d-5c3a2e1f0d02");
const PLANET_SHADER: Handle<Shader> = uuid_handle!("3c7a1e9b-5d2f-4a8c-9e6b-1f0d2c3a4b03");
/// Fuel ring diameters around a seat: context, 5-hour, week.
const FUEL_D: [f32; 3] = [70.0, 82.0, 94.0];
const HALO_D: f32 = 116.0;

fn main() {
    install_crash_log();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || feed::run(tx));
    App::new()
        .insert_resource(Splash::default())
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
        .add_plugins((
            Material2dPlugin::<RingMaterial>::default(),
            Material2dPlugin::<CoreMaterial>::default(),
            Material2dPlugin::<PlanetMaterial>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (watch_mods, read_feed, layout_view, apply_rules, layout_seats, layout_tasks, pointer, chat_input, keys).chain(),
                (
                    glow_seats,
                    draw_rings,
                    pulse_core,
                    fly_comets,
                    move_stars,
                    colour_stars,
                    draw_splash,
                    draw_links,
                    drive_planets,
                    draw_task_bolts,
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

#[derive(ShaderType, Clone, Copy, Debug, Default)]
struct PlanetUniform {
    colour: Vec4,
    /// time, work, busy, seed
    params: Vec4,
}

/// mods/shaders/planet.wgsl: inside a seat, what it's doing.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
struct PlanetMaterial {
    #[uniform(0)]
    u: PlanetUniform,
}

impl Material2d for PlanetMaterial {
    fn fragment_shader() -> ShaderRef {
        PLANET_SHADER.into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Component)]
struct PlanetFace;

/// A packet of light running down a conduit: a job just finished.
struct Packet {
    seat: String,
    at: f32,
    colour: [f32; 3],
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
    /// Every seat's fixed slot on the ring, online or not.
    roster: Vec<String>,
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
    /// The ring's spin (radians a second), kicked by notes between agents.
    spin: f32,
    /// What the ring is turning at this frame, spin and warp together.
    turning: f32,
    /// 0.., kicked by agents writing to Y, decaying: the stars' boost.
    boost: f32,
    packets: Vec<Packet>,
    /// Each seat's last state from activity, to see a job finish.
    last_state: HashMap<String, String>,
    /// The load balancer's board: open tasks orbit, held ones are bolts.
    tasks: Vec<TaskInfo>,
    /// How far round the task ring has turned (radians).
    task_orbit: f32,
    /// The task ids the ring was last built from.
    tasks_built: Vec<String>,
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
    tasks: bool,
}
impl Default for View {
    fn default() -> Self {
        View { hud: true, fuel: true, moons: true, tasks: true }
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

/// What came back from a send (Some(the line)) or a wake (None).
type SendResult = (Option<String>, Result<String, String>);

/// The panel's tabs: the chat screen's notes, activity and notifications.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Tab {
    #[default]
    Comms,
    Activity,
    Notify,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Comms, Tab::Activity, Tab::Notify];
    fn name(self) -> &'static str {
        match self {
            Tab::Comms => "COMMS",
            Tab::Activity => "ACTIVITY",
            Tab::Notify => "NOTIFY",
        }
    }
}

/// The chat panel: the notes, the line being typed, and sends in flight.
#[derive(Resource)]
struct Chat {
    notes: VecDeque<ChatNote>,
    activity: VecDeque<Activity>,
    notices: VecDeque<Notice>,
    tab: Tab,
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
            activity: VecDeque::new(),
            notices: VecDeque::new(),
            tab: Tab::Comms,
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

/// Light soaked into the scene's walls: four edges (top, right, bottom,
/// left), each a row of cells holding linear RGB energy.
#[derive(Resource, Default)]
struct Splash {
    edges: [Vec<Vec3>; 4],
}

impl Splash {
    fn fit(&mut self, cells: usize) {
        let cells = cells.clamp(4, 512);
        for e in &mut self.edges {
            if e.len() != cells {
                *e = vec![Vec3::ZERO; cells];
            }
        }
    }

    /// A star of `colour` hits edge `edge` at `t` (0..1 along it).
    fn hit(&mut self, edge: usize, t: f32, colour: Vec3, cfg: &mods::SplashCfg) {
        let cells = self.edges[edge].len();
        if cells == 0 {
            return;
        }
        let at = ((t.clamp(0.0, 1.0) * cells as f32) as usize).min(cells - 1);
        let add = colour * cfg.energy;
        for k in 0..=cfg.spill.min(16) {
            let w = 0.5f32.powi(k as i32);
            if at + k < cells {
                self.edges[edge][at + k] += add * w;
            }
            if k > 0 && at >= k {
                self.edges[edge][at - k] += add * w;
            }
        }
    }

    /// The glow fades, and spreads a little along each wall.
    fn settle(&mut self, dt: f32, cfg: &mods::SplashCfg) {
        let keep = (-cfg.fade.max(0.0) * dt).exp();
        let d = (cfg.spread.max(0.0) * dt).min(0.3);
        for e in &mut self.edges {
            let before = e.clone();
            let n = before.len();
            for i in 0..n {
                let l = before[if i == 0 { 0 } else { i - 1 }];
                let r = before[if i + 1 == n { i } else { i + 1 }];
                e[i] = (before[i] * (1.0 - 2.0 * d) + (l + r) * d) * keep;
                e[i] = e[i].min(Vec3::splat(8.0));
            }
        }
    }
}

/// Where a ray from the core leaves the scene's rectangle (half-size
/// `half`): which edge (0 top, 1 right, 2 bottom, 3 left) and how far along
/// it (0..1, left to right / bottom to top).
fn wall_hit(p: Vec2, half: Vec2) -> Option<(usize, f32)> {
    if p.x.abs() <= half.x && p.y.abs() <= half.y {
        return None;
    }
    // The edge it crossed first is the one it overshot by the larger ratio.
    let rx = p.x.abs() / half.x.max(1.0);
    let ry = p.y.abs() / half.y.max(1.0);
    let along = |v: f32, h: f32| (v / h.max(1.0) * 0.5 + 0.5).clamp(0.0, 1.0);
    Some(if ry >= rx {
        let q = p * (half.y / p.y.abs());
        (if p.y > 0.0 { 0 } else { 2 }, along(q.x, half.x))
    } else {
        let q = p * (half.x / p.x.abs());
        (if p.x > 0.0 { 1 } else { 3 }, along(q.y, half.y))
    })
}

/// Panics go to ~/.local/state/warp-core/crash.log as well as stderr, so a
/// crash leaves a reason behind even when nobody saw the terminal.
fn install_crash_log() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        let dir = std::path::Path::new(&home).join(".local/state/warp-core");
        let _ = std::fs::create_dir_all(&dir);
        let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let thread = std::thread::current().name().unwrap_or("?").to_string();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            use std::io::Write;
            let _ = writeln!(f, "{when} thread {thread}: {info}");
        }
        default(info);
    }));
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
    /// The planet's body: its place and motion in the physics sim (t13).
    body: Body,
}

/// One planet in the simplified physics sim: where it is, how it moves.
#[derive(Clone, Copy, Default)]
struct Body {
    pos: Vec2,
    vel: Vec2,
}

/// One step of the planets' physics (t13). The core is a singularity: its
/// pull is `gravity / r²` towards the centre. A busy seat has thrust enough
/// to climb to a higher orbit; an idle one keeps orbiting and drifts in
/// towards the core. The ring's turning (drift, traffic, warp, the notes'
/// spin kicks) steers each planet's tangential speed towards `turning * r`,
/// so the whole ring still answers the deck the way it did. No planet is
/// ever lost over the screen's edge: past the margin it is reflected back
/// in, keeping `bounce` of its speed. Inside `min_radius` nothing falls
/// further in: the planet skims the singularity and keeps circling.
fn step_body(body: &mut Body, busy: bool, turning: f32, scene: Vec2, dt: f32, cfg: &mods::PhysicsCfg) {
    let dt = dt.min(0.1); // a stall mustn't fling anything
    let p = body.pos;
    let r = p.length().max(1.0);
    let out = p / r;
    // Clockwise round the ring (the way the ring turns as its angle grows).
    let tang = Vec2::new(out.y, -out.x);
    // The core's pull, softened at the floor so the sim stays stable there.
    let g = cfg.gravity / (r.max(cfg.min_radius) * r.max(cfg.min_radius));
    // Thrust rides on the local pull (a multiple of it), so even a planet
    // that sank all the way in has climb enough to get back out. It fades
    // out on the approach to the screen's edge — measured in the planet's
    // own direction — so a working agent parks on the highest orbit there
    // is room for.
    let half = scene * 0.5;
    let edge = (1.0 / (out.x.abs() / half.x).max(out.y.abs() / half.y).max(1e-4)) - cfg.edge_margin;
    let fade = 1.0 - ((r - 0.75 * edge) / (0.25 * edge.max(1.0))).clamp(0.0, 1.0);
    let thrust = if busy { cfg.thrust * g * fade } else { 0.0 };
    let sink = if busy { 0.0 } else { cfg.drift };
    // Steer the tangential speed onto the ring's turning, capped, so the
    // spin kicks and warp swing are felt but can't throw a planet.
    let v_t = body.vel.dot(tang);
    let steer = ((turning * r - v_t) * cfg.steering).clamp(-cfg.max_steer, cfg.max_steer);
    body.vel += (out * (thrust - sink - g) + tang * steer) * dt;
    body.pos += body.vel * dt;
    // Skim, don't fall in: on the floor of the core's well the inward part
    // of the speed is taken away, leaving the planet to circle.
    let r2 = body.pos.length();
    if r2 < cfg.min_radius {
        let out2 = body.pos / r2.max(1e-4);
        let tang2 = Vec2::new(out2.y, -out2.x);
        let v_t2 = body.vel.dot(tang2).max(0.0) * tang2 + body.vel.dot(out2).max(0.0) * out2;
        body.pos = out2 * cfg.min_radius;
        body.vel = v_t2;
    }
    // No planet is lost over the edge: it bounces back into the scene.
    let half = scene * 0.5 - cfg.edge_margin;
    let keep = cfg.bounce;
    if body.pos.x < -half.x {
        body.pos.x = -half.x;
        body.vel.x = body.vel.x.abs() * keep;
    } else if body.pos.x > half.x {
        body.pos.x = half.x;
        body.vel.x = -body.vel.x.abs() * keep;
    }
    if body.pos.y < -half.y {
        body.pos.y = -half.y;
        body.vel.y = body.vel.y.abs() * keep;
    } else if body.pos.y > half.y {
        body.pos.y = half.y;
        body.vel.y = -body.vel.y.abs() * keep;
    }
}

/// One open task orbiting the core, coloured by who assigned it.
#[derive(Component)]
struct TaskNode {
    id: String,
    from: String,
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
    /// Its colour (linear), for the light it leaves on the wall.
    colour: Vec3,
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
            Star { dir, dist: 40.0 + rng.next() * 1400.0, speed: 0.4 + rng.next() * 1.2, slot: rng.next(), colour: Vec3::new(0.9, 0.95, 1.4) },
            Mesh2d(assets.star.clone()),
            MeshMaterial2d(star_mat.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
    }

    commands.spawn((
        Hud,
        Text2d::new("warp core: waiting for the deck"),
        TextFont { font_size: FontSize::Px(15.0), ..default() },
        TextBounds::new_horizontal(800.0),
        TextColor(Color::srgb(0.55, 0.65, 0.8)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 5.0),
    ));
    commands.spawn((
        Dock,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(13.0), ..default() },
        TextBounds::new_horizontal(800.0),
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
                let frame = |fps: f32| std::time::Duration::from_secs_f32(1.0 / fps.clamp(5.0, 120.0));
                winit.focused_mode = UpdateMode::reactive(frame(c.look.fps));
                winit.unfocused_mode = UpdateMode::reactive(frame(c.look.fps_unfocused));
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
        ("planet.wgsl", PLANET_SHADER, mods::DEFAULT_PLANET_SHADER),
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

fn read_feed(
    mut feed: ResMut<Feed>,
    mut deck: ResMut<Deck>,
    mut chat: ResMut<Chat>,
    mods: Res<Mods>,
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut last_restart: Local<f32>,
) {
    let now = time.elapsed_secs();
    let mut dead = false;
    let msgs: Vec<Msg> = {
        let Ok(rx) = feed.0.lock() else { return };
        let mut v = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(m) => v.push(m),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    dead = true;
                    break;
                }
            }
        }
        v
    };
    // The feed thread died (a panic, logged to crash.log): start a new one,
    // at most every 10 s, and let it reload the panel's backlog.
    if dead && real.elapsed_secs() - *last_restart > 10.0 {
        *last_restart = real.elapsed_secs();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || feed::run(tx));
        feed.0 = Mutex::new(rx);
        chat.notes.clear();
        chat.activity.clear();
        chat.notices.clear();
        chat.dirty = true;
        deck.problem = Some("the feed stopped and was restarted (see ~/.local/state/warp-core/crash.log)".into());
    }
    for msg in msgs {
        match msg {
            Msg::Seats(seats) => {
                deck.seats = seats;
                deck.problem = None;
            }
            Msg::Problem(p) => deck.problem = Some(p),
            Msg::Activity(a, live) => {
                // A seat changing state ripples the ring in its colour; a
                // seat that stops being busy has finished a job: a packet.
                let was = deck.last_state.insert(a.seat.clone(), a.state.clone());
                if live {
                    deck.shocks.push(Shock { seat: a.seat.clone(), at: now, colour: seat_rgb(&a.seat), size: 0.45 });
                    if finished_job(was.as_deref(), &a.state) {
                        deck.packets.push(Packet { seat: a.seat.clone(), at: now, colour: seat_rgb(&a.seat) });
                    }
                }
                chat.activity.push_back(a);
                let over = chat.activity.len().saturating_sub(200);
                chat.activity.drain(..over);
                chat.dirty |= chat.tab == Tab::Activity;
            }
            Msg::Notice(n) => {
                chat.notices.push_back(n);
                let over = chat.notices.len().saturating_sub(100);
                chat.notices.drain(..over);
                chat.dirty |= chat.tab == Tab::Notify;
            }
            Msg::Chat(n) => {
                chat.notes.push_back(n);
                let over = chat.notes.len().saturating_sub(mods.cfg.chat.keep.max(10));
                chat.notes.drain(..over);
                // Scrolled back: stay on the same notes as new ones arrive.
                if chat.scroll > 0 && chat.tab == Tab::Comms {
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
            Msg::Tasks(tasks) => {
                // A held task gone from the board is done: its holder sends a
                // packet in the colour of whoever asked for it.
                let fired: Vec<Packet> = finished_tasks(&deck.tasks, &tasks)
                    .into_iter()
                    .filter_map(|d| Some(Packet { seat: d.holder.clone()?, at: now, colour: seat_rgb(&d.from) }))
                    .collect();
                deck.packets.extend(fired);
                deck.tasks = tasks;
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
    deck.boost = (deck.boost - time.delta_secs() * mods.cfg.stars.boost_decay).max(0.0);
    let gone = mods.cfg.conduits.packet_seconds.max(0.1);
    deck.packets.retain(|p| now - p.at < gone);
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

/// The fixed order of the ring: Y, then the agents in their desktop order
/// (Super+1..7), then any other seat by name. Never the service's
/// availability ranking, which reshuffles as seats change state.
const SEAT_ORDER: [&str; 10] = ["owner", "claude", "grok", "vibe", "devin", "gravity", "lumo", "alice", "codex", "mystra"];

fn seat_roster<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut all: Vec<String> = names.map(String::from).collect();
    all.sort_by_key(|n| (SEAT_ORDER.iter().position(|s| s == n).unwrap_or(SEAT_ORDER.len()), n.clone()));
    all.dedup();
    all
}

/// The orbit's speed in radians a second: a slow drift when quiet, faster
/// with traffic, and a hard swing in warp.
fn orbit_speed(cfg: &mods::OrbitCfg, notes_10min: usize, warp: f32, spin: f32) -> f32 {
    cfg.drift + (notes_10min.min(30) as f32) * cfg.traffic + warp * cfg.warp_swing + spin
}

/// Which way a note travels the short way round the ring (t14): +1.0
/// clockwise (the way the ring turns as its angle grows), -1.0 anticlockwise,
/// 0.0 when the seats aren't both on the ring, are the same, or sit opposite.
fn ring_travel(ring: &[String], from: &str, to: &str) -> f32 {
    let (Some(i), Some(j)) = (ring.iter().position(|s| s == from), ring.iter().position(|s| s == to)) else {
        return 0.0;
    };
    let n = ring.len() as i64;
    let d = (j as i64 - i as i64).rem_euclid(n);
    if d == 0 || 2 * d == n {
        0.0
    } else if 2 * d < n {
        1.0
    } else {
        -1.0
    }
}

/// The spin a note from `from` to `to` adds: `kick` in the direction it
/// travels round the ring the short way (positive = clockwise, the way the
/// ring turns as its angle grows), nothing between seats not on the ring.
fn spin_kick(ring: &[String], from: &str, to: &str, kick: f32) -> f32 {
    ring_travel(ring, from, to) * kick
}

/// A job finished: the seat was busy and now isn't (stuck or spent aren't
/// finishing).
fn finished_job(was: Option<&str>, now: &str) -> bool {
    was == Some("busy") && matches!(now, "idle" | "ready" | "away")
}

/// Tasks held on the old board that are gone from (or reopened on) the new.
fn finished_tasks<'a>(old: &'a [feed::TaskInfo], new: &[feed::TaskInfo]) -> Vec<&'a feed::TaskInfo> {
    old.iter()
        .filter(|t| t.holder.is_some() && !t.open)
        .filter(|t| !new.iter().any(|n| n.id == t.id && !n.open))
        .collect()
}

/// Points along a conduit, seat (first) to core (last), bent by the ring's
/// turning: the inner end leads, the way a rotating mass drags space round
/// with it.
fn conduit_path(seat: Vec2, bend: f32, n: usize) -> Vec<Vec2> {
    let (r0, r1) = (seat.length() * 0.9, 70.0);
    let a0 = seat.to_angle();
    (0..=n)
        .map(|i| {
            let s = i as f32 / n as f32;
            Vec2::from_angle(a0 - bend * s * s) * (r0 + (r1 - r0) * s)
        })
        .collect()
}

fn conduit_bend(cfg: &mods::ConduitsCfg, turning: f32) -> f32 {
    (turning * cfg.bend).clamp(-cfg.max_bend, cfg.max_bend)
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
    mut planets: ResMut<Assets<PlanetMaterial>>,
    layout: Res<Layout>,
    time: Res<Time>,
    mut nodes: Query<(Entity, &mut SeatNode, &mut Transform)>,
) {
    let pace = deck.notes_last(time.elapsed_secs(), 600.0);
    let warp = deck.warp;
    let oc = &mods.cfg.orbit;
    let dt = time.delta_secs();
    deck.spin = (deck.spin * (-oc.friction.max(0.0) * dt).exp()).clamp(-oc.max_spin, oc.max_spin);
    let turning = orbit_speed(oc, pace, warp, deck.spin);
    deck.turning = turning;
    deck.orbit = (deck.orbit + dt * turning).rem_euclid(std::f32::consts::TAU);
    // Every seat keeps one fixed slot on the ring, in a fixed order, so the
    // planets never change places relative to each other (Y, t8): offline
    // seats leave their slot empty rather than the others closing up.
    let roster = seat_roster(deck.seats.iter().map(|s| s.name.as_str()));
    let names: Vec<String> = roster.iter().filter(|n| deck.seat(n).is_some_and(|s| s.online())).cloned().collect();
    let pos: HashMap<String, (Vec2, f32)> =
        ring_positions(&roster, layout.scene, deck.orbit).into_iter().filter(|(n, _)| names.contains(n)).collect();
    deck.roster = roster;
    if names != deck.built {
        for (e, _, _) in &nodes {
            commands.entity(e).despawn();
        }
        for name in &names {
            let (p, depth) = pos[name];
            // A planet joins the ring in step with it: circling at the
            // ring's current turning (t13).
            let r = p.length().max(1.0);
            let tang = Vec2::new(p.y / r, -p.x / r);
            commands
                .spawn((
                    SeatNode { name: name.clone(), flash: 0.0, depth, body: Body { pos: p, vel: tang * turning * r } },
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
                        PlanetFace,
                        Mesh2d(assets.quad.clone()),
                        MeshMaterial2d(planets.add(PlanetMaterial::default())),
                        Transform::from_xyz(0.0, 0.0, 0.05).with_scale(Vec3::new(52.0, 52.0, 1.0)),
                    ));
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
    // The planets fly the sim now (t13): gravity, thrust, drift, steering —
    // each one on its own, so working agents climb to higher orbits while
    // the idle ones sink towards the core. Depth follows the sim too: the
    // higher a planet stands on the screen (the far side of the tilted
    // orbit), the further behind the core it slides.
    let ry = layout.scene.y * 0.30;
    for (_, mut node, mut tf) in &mut nodes {
        let busy = deck.seat(&node.name).is_some_and(|s| s.shown == "busy");
        step_body(&mut node.body, busy, turning, layout.scene, dt, &mods.cfg.physics);
        let p = node.body.pos;
        node.depth = (0.5 - 0.5 * (p.y / ry)).clamp(0.0, 1.0);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        tf.translation.z = seat_z(node.depth);
    }
}

/// Where an open task sits on its ring: evenly spaced, turning clockwise,
/// the task ring inside the seat ring by `[tasks] radius`.
fn task_positions(n: usize, scene: Vec2, orbit: f32, radius: f32) -> Vec<Vec2> {
    let seat_rx = scene.x * 0.38 * radius;
    let seat_ry = scene.y * 0.30 * radius;
    let n = n.max(1) as f32;
    (0..n as usize)
        .map(|i| {
            let a = std::f32::consts::FRAC_PI_2 - i as f32 / n * std::f32::consts::TAU - orbit;
            Vec2::new(a.cos() * seat_rx, a.sin() * seat_ry)
        })
        .collect()
}

/// Open tasks orbit the core on their own ring; each burns in the colour of
/// the seat that assigned it, and shows its id (t1, t2, ...) underneath.
fn layout_tasks(
    mut commands: Commands,
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    view: Res<View>,
    layout: Res<Layout>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
    mut nodes: Query<(Entity, &mut TaskNode, &mut Transform)>,
) {
    let tc = &mods.cfg.tasks;
    let open: Vec<TaskInfo> = deck.tasks.iter().filter(|t| t.open).cloned().collect();
    let ids: Vec<String> = open.iter().map(|t| t.id.clone()).collect();
    let shown = tc.show && view.tasks;
    if ids != deck.tasks_built || !shown {
        // The board changed (or tasks were switched off): rebuild the ring.
        for (e, _, _) in &nodes {
            commands.entity(e).despawn();
        }
        deck.tasks_built = if shown { ids.clone() } else { Vec::new() };
        if !shown {
            return;
        }
        let pos = task_positions(open.len(), layout.scene, deck.task_orbit, tc.radius);
        for (i, task) in open.iter().enumerate() {
            let Some(p) = pos.get(i) else { continue };
            let colour = seat_colour(&task.from, tc.bright);
            commands
                .spawn((
                    TaskNode { id: task.id.clone(), from: task.from.clone() },
                    Mesh2d(assets.dot.clone()),
                    MeshMaterial2d(materials.add(colour)),
                    Transform::from_xyz(p.x, p.y, 1.5).with_scale(Vec3::splat(tc.size)),
                ))
                .with_children(|task_node| {
                    task_node.spawn((
                        Text2d::new(task.id.clone()),
                        TextFont { font_size: FontSize::Px(12.0), ..default() },
                        TextColor(seat_colour(&task.from, 0.9)),
                        Anchor::TOP_CENTER,
                        Transform::from_xyz(0.0, -9.0, 0.1),
                    ));
                });
        }
        return;
    }
    deck.task_orbit = (deck.task_orbit + time.delta_secs() * tc.speed) % std::f32::consts::TAU;
    let pos = task_positions(open.len(), layout.scene, deck.task_orbit, tc.radius);
    for (i, (_, mut node, mut tf)) in nodes.iter_mut().enumerate() {
        if let Some(p) = pos.get(i) {
            tf.translation.x = p.x;
            tf.translation.y = p.y;
            tf.translation.z = 1.5;
        }
        if let Some(task) = open.get(i) {
            node.from = task.from.clone();
        }
    }
}

/// A held task is a bolt from its holder's seat to the core, in the
/// assigner's colour, with a packet of light running down it — the task
/// literally travelling from where it was asked into the work.
fn draw_task_bolts(
    deck: Res<Deck>,
    mods: Res<Mods>,
    view: Res<View>,
    time: Res<Time>,
    nodes: Query<(&SeatNode, &Transform)>,
    mut gizmos: Gizmos,
) {
    let tc = &mods.cfg.tasks;
    if !tc.show || !view.tasks {
        return;
    }
    let t = time.elapsed_secs();
    let bend = conduit_bend(&mods.cfg.conduits, deck.turning);
    let positions: HashMap<&str, Vec2> =
        nodes.iter().map(|(n, tf)| (n.name.as_str(), tf.translation.truncate())).collect();
    for task in deck.tasks.iter().filter(|t| !t.open) {
        let Some(holder) = task.holder.as_deref().and_then(|h| positions.get(h)) else { continue };
        let dim = if deck.focus.as_ref().is_some_and(|f| *f != task.holder.as_deref().unwrap_or("")) {
            0.25
        } else {
            1.0
        };
        // Along the holder's conduit, bent by the spin like it, in the
        // colour of whoever asked. No packet until the task is done (Y):
        // that one is fired when it leaves the board (read_feed).
        let path = conduit_path(*holder, bend, 18);
        let n = path.len() as f32;
        let _ = t;
        gizmos.linestrip_gradient_2d(path.iter().enumerate().map(|(i, p)| {
            (*p, seat_colour(&task.from, tc.bolt_bright * dim * (1.0 - 0.7 * i as f32 / n)))
        }));
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
            let w = world.unwrap_or_default();
            if w.y > layout.scene.y * 0.5 - 38.0 {
                // The header: a click picks a tab by thirds.
                let k = ((w.x - layout.scene.x * 0.5) / layout.panel.max(1.0) * 3.0).clamp(0.0, 2.0) as usize;
                set_tab(&mut chat, Tab::ALL[k]);
            } else {
                chat.focus = true;
            }
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
        // Clicking an agent takes Y to its own window (Y, 2026-11-27).
        if let Some(h) = deck.hover.clone().filter(|h| h != "owner") {
            chat.sending += 1;
            chat.status = Some((format!("going to {}...", label_for(&h)), false, f32::MAX));
            let tx = chat.tx.clone();
            std::thread::spawn(move || {
                let r = chat::focus_window(&h);
                let _ = tx.send((None, r));
            });
        } else if deck.hover.is_none() {
            deck.focus = None;
        }
    }
    // Right-click wakes a seat, through comms-deck wake and its rules.
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(h) = deck.hover.clone().filter(|h| h != "owner") {
            chat.sending += 1;
            chat.status = Some((format!("waking {}...", label_for(&h)), false, f32::MAX));
            let tx = chat.tx.clone();
            std::thread::spawn(move || {
                let r = chat::wake(&h);
                let _ = tx.send((None, r));
            });
        }
    }
}

fn set_tab(chat: &mut Chat, tab: Tab) {
    if chat.tab != tab {
        chat.tab = tab;
        chat.scroll = 0;
        chat.dirty = true;
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
        let hovered = cfg.hud.hover_card && deck.hover.as_ref() == Some(&node.name);
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
    // No containment rings round the core: Y found the two arc patterns
    // spinning round it unnecessary (2026-11-27) — the black hole alone
    // marks the centre, the planets and their orbits do the rest.
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
/// Conduits from each planet to the core, bent by the ring's turning. A
/// packet of light runs down one only when that seat finishes a job.
fn draw_links(deck: Res<Deck>, mods: Res<Mods>, nodes: Query<(&SeatNode, &Transform)>, time: Res<Time>, mut gizmos: Gizmos) {
    let t = time.elapsed_secs();
    let bend = conduit_bend(&mods.cfg.conduits, deck.turning);
    let secs = mods.cfg.conduits.packet_seconds.max(0.1);
    for (node, tf) in &nodes {
        let Some(seat) = deck.seat(&node.name) else { continue };
        let path = conduit_path(tf.translation.truncate(), bend, 18);
        let dim = if deck.focus.as_ref().is_some_and(|f| *f != node.name) { 0.25 } else { 1.0 };
        let b = ((mods.cfg.brightness(&seat.shown) * 0.18).min(0.6) + node.flash) * dim;
        let n = path.len() as f32;
        gizmos.linestrip_gradient_2d(
            path.iter().enumerate().map(|(i, p)| (*p, seat_colour(&node.name, b * (1.0 - 0.6 * i as f32 / n)))),
        );
        for pk in deck.packets.iter().filter(|pk| pk.seat == node.name) {
            let k = ((t - pk.at) / secs).clamp(0.0, 1.0);
            let at = |k: f32| path[((k * (path.len() - 1) as f32).round() as usize).min(path.len() - 1)];
            gizmos.circle_2d(Isometry2d::from_translation(at(k)), 3.0, lin(pk.colour, 5.0 * dim));
            gizmos.line_2d(at((k - 0.12).max(0.0)), at(k), lin(pk.colour, 2.5 * dim));
        }
    }
}

/// Drives each planet's inside: work grows the longer it has been busy.
fn drive_planets(
    deck: Res<Deck>,
    mods: Res<Mods>,
    time: Res<Time>,
    nodes: Query<(&SeatNode, &Children)>,
    mut faces: Query<(&MeshMaterial2d<PlanetMaterial>, &mut Visibility), With<PlanetFace>>,
    mut mats: ResMut<Assets<PlanetMaterial>>,
) {
    let pc = &mods.cfg.planets;
    let t = time.elapsed_secs();
    for (node, children) in &nodes {
        let Some(seat) = deck.seat(&node.name) else { continue };
        for child in children.iter() {
            let Ok((mat, mut vis)) = faces.get_mut(child) else { continue };
            *vis = if pc.show { Visibility::Inherited } else { Visibility::Hidden };
            let busy = seat.shown == "busy";
            let work = if busy { planet_work(seat.since_s, pc.full_minutes) } else { 0.0 };
            let seed = (node.name.bytes().map(|b| b as u32).sum::<u32>() % 97) as f32 / 97.0;
            if let Some(mut m) = mats.get_mut(&mat.0) {
                m.u = PlanetUniform {
                    colour: v4(seat_rgb(&node.name), 1.0),
                    params: Vec4::new(t, work, if busy { 1.0 } else { 0.0 }, seed),
                };
            }
        }
    }
}

/// 0..1: how much a seat has been working, from how long it's been busy.
fn planet_work(busy_secs: f64, full_minutes: f32) -> f32 {
    (1.0 - (-(busy_secs as f32) / (full_minutes.max(0.1) * 60.0)).exp()).clamp(0.0, 1.0)
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
            // An agent writing to Y boosts the stars; notes between agents
            // speed the ring up. Always clockwise, the way the core's disk
            // turns: a note never reverses the orbit (Y, t11).
            if note.from != "owner" && note.to.iter().any(|t| t == "owner") {
                deck.boost += mods.cfg.stars.boost_per_note;
            }
            if !note.all && note.from != "owner" {
                let ring = deck.roster.clone();
                for to in note.to.iter().filter(|t| *t != "owner") {
                    deck.spin += spin_kick(&ring, &note.from, to, mods.cfg.orbit.spin_kick).abs();
                }
            }
            let from = positions.get(&note.from).copied().unwrap_or(Vec2::ZERO);
            let targets: Vec<String> = if note.all {
                positions.keys().filter(|k| **k != note.from).cloned().collect()
            } else {
                note.to.clone()
            };
            let involved = |seat: &str| deck.focus.as_deref().is_none_or(|f| f == seat || f == note.from);
            for to in targets {
                let Some(to_pos) = positions.get(&to).copied() else { continue };
                // Through the core, bowed the way the note travels round the
                // ring (t14), so the tail's arc follows the spin instead of
                // fighting it. Bowed at all so two-way traffic doesn't overlap.
                let side = match ring_travel(&deck.roster, &note.from, &to) {
                    0.0 => 1.0,
                    s => s,
                };
                let via = (from + to_pos) * 0.12 + (to_pos - from).perp().normalize_or_zero() * (40.0 * side);
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
    mut splash: ResMut<Splash>,
    mut stars: Query<(&mut Star, &mut Transform)>,
) {
    let sc = &mods.cfg.stars;
    let spl = &mods.cfg.splash;
    splash.fit(spl.cells);
    // The walls stars splash on: the scene's edges, the panel's left edge
    // included.
    let walls = layout.half();
    // The sky fills the whole window (the panel covers its right side).
    let half = Vec2::new(layout.scene.x * 0.5 + layout.panel, window_size(&windows).y * 0.5);
    let reach = half.length() + 20.0;
    let pace = (deck.notes_last(time.elapsed_secs(), 600.0) as f32).min(30.0);
    let tc = &mods.cfg.typing;
    // Still when nobody works, slow with a few at it, boosted by notes to Y.
    let busy = deck.seats.iter().filter(|s| s.online() && s.shown == "busy").count() as f32;
    let speed = sc.speed + busy * sc.per_busy + deck.boost * sc.boost_speed + deck.warp * sc.warp_speed + deck.thrust * tc.star_speed;
    let dt = time.delta_secs();
    for (mut star, mut tf) in &mut stars {
        star.dist += speed * star.speed * dt * (0.3 + star.dist / reach);
        let hit = if spl.show { wall_hit(star.dir * star.dist, walls) } else { None };
        if let Some((edge, t)) = hit {
            splash.hit(edge, t, star.colour, spl);
        }
        if hit.is_some() || star.dist > reach {
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

/// The light the stars left on the walls: a thin glow just inside each
/// edge, with a fainter halo a little further in.
fn draw_splash(mut splash: ResMut<Splash>, mods: Res<Mods>, time: Res<Time>, layout: Res<Layout>, mut gizmos: Gizmos) {
    let cfg = &mods.cfg.splash;
    if !cfg.show {
        return;
    }
    splash.settle(time.delta_secs(), cfg);
    let h = layout.half();
    for (edge, cells) in splash.edges.iter().enumerate() {
        let n = cells.len();
        for (i, e) in cells.iter().enumerate() {
            if e.max_element() < 0.01 {
                continue;
            }
            let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
            let seg = |inset: f32| -> (Vec2, Vec2) {
                match edge {
                    0 => (Vec2::new(-h.x + a * 2.0 * h.x, h.y - inset), Vec2::new(-h.x + b * 2.0 * h.x, h.y - inset)),
                    2 => (Vec2::new(-h.x + a * 2.0 * h.x, -h.y + inset), Vec2::new(-h.x + b * 2.0 * h.x, -h.y + inset)),
                    1 => (Vec2::new(h.x - inset, -h.y + a * 2.0 * h.y), Vec2::new(h.x - inset, -h.y + b * 2.0 * h.y)),
                    _ => (Vec2::new(-h.x + inset, -h.y + a * 2.0 * h.y), Vec2::new(-h.x + inset, -h.y + b * 2.0 * h.y)),
                }
            };
            let c = *e * cfg.glow;
            let (p, q) = seg(1.5);
            gizmos.line_2d(p, q, Color::linear_rgb(c.x, c.y, c.z));
            let (p, q) = seg(5.0);
            let c = c * 0.35;
            gizmos.line_2d(p, q, Color::linear_rgb(c.x, c.y, c.z));
        }
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
    mut hud: Query<(&mut Text2d, &mut Transform, &mut TextBounds), (With<Hud>, Without<Dock>)>,
    mut dock: Query<(&mut Text2d, &mut Transform, &mut Visibility, &mut TextBounds), (With<Dock>, Without<Hud>)>,
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
        let costs: Vec<f64> = deck.seats.iter().filter_map(|s| s.cost_usd).collect();
        if !costs.is_empty() {
            line.push_str(&format!(" | ~${:.2} (estimate)", costs.iter().sum::<f64>()));
        }
        if deck.warp > 0.05 {
            line.push_str(&format!("    >> WARP {:.1}", deck.warp));
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
    // Both wrap inside the scene, never under the chat panel.
    let wrap = Some((layout.scene.x - 36.0).max(100.0));
    for (mut text, mut tf, mut bounds) in &mut hud {
        if bounds.width != wrap {
            bounds.width = wrap;
        }
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
        format!("off the ring: {}\nclick a seat: its window | right-click: wake | 1-9 | h c m f r | space: test", rows.join("\n              "))
    } else {
        String::new()
    };
    for (mut text, mut tf, mut vis, mut bounds) in &mut dock {
        if bounds.width != wrap {
            bounds.width = wrap;
        }
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
    mods: Res<Mods>,
    time: Res<Time>,
    layout: Res<Layout>,
    mut tip: Query<(&mut Text2d, &mut Transform, &mut Visibility, &mut Anchor), With<Tooltip>>,
) {
    let half = layout.half();
    let card = if mods.cfg.hud.hover_card {
        deck.hover.as_deref().and_then(|h| hover_card(&deck, h, time.elapsed_secs()))
    } else {
        None
    };
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
    // No key quits (Y, 2026-11-27: survival first; a stray q took it down at
    // 04:22). It runs as a service: `systemctl --user stop warp-core` stops it.
    let _ = &mut exit;
    // While the chat box has the keys, letters are typing, not commands.
    if chat.focus {
        return;
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
    if keys.just_pressed(KeyCode::KeyT) {
        view.tasks = !view.tasks;
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
    mut stars: Query<(&mut Star, &mut MeshMaterial2d<ColorMaterial>)>,
) {
    let sc = &mods.cfg.stars;
    let shares = if sc.by_context { context_shares(&deck.seats) } else { Vec::new() };
    let key = format!(
        "{}|{:?}|{:?}",
        sc.context_bright,
        sc.colour,
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
    for (mut star, mut mat) in &mut stars {
        let seat = seat_for_slot(&shares, star.slot);
        star.colour = match seat {
            Some(n) => Vec3::from(seat_rgb(n)) * sc.context_bright,
            None => Vec3::from(sc.colour),
        };
        let want = seat.and_then(|n| mats.seats.get(n)).unwrap_or(&mats.default).clone();
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
        // Tabs and scrolling work from either side.
        match ev.key_code {
            KeyCode::F1 | KeyCode::F2 | KeyCode::F3 => {
                let k = match ev.key_code {
                    KeyCode::F1 => 0,
                    KeyCode::F2 => 1,
                    _ => 2,
                };
                set_tab(&mut chat, Tab::ALL[k]);
                continue;
            }
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
                let len = chat.input.trim().chars().count();
                let seats: Vec<String> = deck.seats.iter().map(|s| s.name.clone()).collect();
                let reach = chat::route(chat.input.trim(), &seats).map(|to| recipients(&to, &seats)).unwrap_or(1);
                send_line(&mut chat, &deck, real.elapsed_secs());
                // A note went out: jump to warp in proportion to its length.
                if chat.sending > before && mods.cfg.typing.jump_on_send {
                    deck.warp = deck.warp.max(send_jump(len, reach, &mods.cfg.typing));
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

/// The warp a sent note jumps to: proportional to its length, capped.
/// Warp drains at a steady rate, so the coast is proportional too.
fn send_jump(len: usize, reach: usize, cfg: &mods::TypingCfg) -> f32 {
    // The more agents a note reaches, the bigger the jump (Y, t9).
    let more = 1.0 + cfg.per_recipient * (reach.max(1) - 1) as f32;
    (len as f32 * cfg.jump_per_char * more).clamp(0.0, cfg.jump_max.max(0.0))
}

/// How many agents a routed note reaches: its named seats, or for `all`
/// every agent seat (not Y) less the ones dropped with -seat.
fn recipients(to: &str, seats: &[String]) -> usize {
    let parts: Vec<&str> = to.split([',', ' ']).filter(|p| !p.is_empty()).collect();
    let drops = parts.iter().filter(|p| p.starts_with('-')).count();
    if parts.first() == Some(&"all") {
        seats.iter().filter(|s| s.as_str() != "owner").count().saturating_sub(drops).max(1)
    } else {
        parts.iter().filter(|p| !p.starts_with('-')).count().max(1)
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
                let _ = tx.send((Some(text), r));
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
        let Some(text) = text else {
            // A wake's answer.
            chat.status = Some(match r {
                Ok(say) => (feed::scrub(&say), false, now),
                Err(e) => (feed::scrub(&e), true, now),
            });
            continue;
        };
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

/// The default font has no dashes, arrows or curly quotes: swap the common
/// ones for ASCII so notes don't show boxes.
fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{2014}' | '\u{2015}' => out.push_str("--"),
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2212}' => out.push('-'),
            '\u{2192}' | '\u{27f6}' => out.push_str("->"),
            '\u{2190}' => out.push_str("<-"),
            '\u{2194}' => out.push_str("<->"),
            '\u{21d2}' => out.push_str("=>"),
            '\u{2026}' => out.push_str("..."),
            '\u{00b7}' | '\u{2022}' | '\u{2219}' => out.push('*'),
            '\u{201c}' | '\u{201d}' | '\u{201e}' => out.push('"'),
            '\u{2018}' | '\u{2019}' | '\u{201a}' => out.push('\''),
            '\u{00d7}' => out.push('x'),
            '\u{2265}' => out.push_str(">="),
            '\u{2264}' => out.push_str("<="),
            '\u{2260}' => out.push_str("!="),
            '\u{2248}' => out.push('~'),
            '\u{2713}' | '\u{2714}' => out.push_str("[ok]"),
            '\u{00a0}' | '\u{202f}' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
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
        let mut text: String = plain(&n.text).chars().take(1500).collect();
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

/// The ACTIVITY tab: each change of a seat's state, newest last.
fn activity_spans(rows: &VecDeque<Activity>, scroll: usize, cap: usize, now: f64) -> Vec<(String, Color)> {
    let end = rows.len().saturating_sub(scroll);
    let start = end.saturating_sub(cap);
    let dim = Color::srgb(0.42, 0.47, 0.58);
    let mut out = Vec::new();
    for (i, a) in rows.range(start..end).enumerate() {
        if i > 0 {
            out.push(("\n".to_string(), dim));
        }
        out.push((format!("{:>7} ", ago(now - a.time)), dim));
        out.push((label_for(&a.seat), seat_colour(&a.seat, 1.1)));
        let state_colour = match a.state.as_str() {
            "busy" => Color::srgb(0.95, 0.8, 0.35),
            "idle" | "ready" => Color::srgb(0.5, 0.85, 0.55),
            "away" | "offline" => Color::srgb(0.5, 0.52, 0.6),
            _ => Color::srgb(0.85, 0.6, 0.6),
        };
        out.push((format!(" {}", a.state), state_colour));
        if !a.doing.is_empty() {
            let doing: String = plain(&a.doing).chars().take(160).collect();
            out.push((format!("  {doing}"), Color::srgb(0.72, 0.75, 0.84)));
        }
    }
    out
}

/// The NOTIFY tab: the desktop notifications the top bar scrolls.
fn notice_spans(rows: &VecDeque<Notice>, scroll: usize, cap: usize, now: f64) -> Vec<(String, Color)> {
    let end = rows.len().saturating_sub(scroll);
    let start = end.saturating_sub(cap);
    let dim = Color::srgb(0.42, 0.47, 0.58);
    let mut out = Vec::new();
    for (i, n) in rows.range(start..end).enumerate() {
        if i > 0 {
            out.push(("\n".to_string(), dim));
        }
        out.push((format!("{:>7}  ", ago(now - n.time)), dim));
        let text: String = plain(&n.text).chars().take(300).collect();
        out.push((text, Color::srgb(0.82, 0.85, 0.93)));
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
    mut line: Query<(&mut Text2d, &mut Transform, &mut TextBounds, &mut Visibility, &TextLayoutInfo), (With<ChatLine>, Without<ChatPanel>, Without<ChatHeader>, Without<ChatLog>, Without<ChatStatus>, Without<ChatTitle>)>,
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
    // A long draft wraps upward: the status line and the notes sit above it.
    let line_h = line
        .iter()
        .next()
        .map(|(.., info)| info.size.y / info.scale_factor.max(0.1))
        .filter(|y| *y > 1.0)
        .unwrap_or(20.0);
    let input_h = line_h + 46.0;
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
        let mut t = Tab::ALL
            .iter()
            .enumerate()
            .map(|(i, tab)| if *tab == chat.tab { format!("[F{} {}]", i + 1, tab.name()) } else { format!(" F{} {} ", i + 1, tab.name()) })
            .collect::<Vec<_>>()
            .join(" ");
        if chat.scroll > 0 {
            t.push_str(&format!("  (back {}, PgDn)", chat.scroll));
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
            let now_epoch = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            let spans = match chat.tab {
                Tab::Comms => chat_spans(&chat.notes, chat.scroll, 60),
                Tab::Activity => activity_spans(&chat.activity, chat.scroll, 80, now_epoch),
                Tab::Notify => notice_spans(&chat.notices, chat.scroll, 50, now_epoch),
            };
            commands.entity(e).with_children(|p| {
                for (text, colour) in spans {
                    p.spawn((TextSpan::new(text), TextColor(colour), TextFont { font_size: FontSize::Px(cfg.font), ..default() }));
                }
            });
        }
    }

    let now = real.elapsed_secs();
    let blink = (now * 2.0) as i64 % 2 == 0;
    for (mut text, mut tf, mut bounds, mut v, _) in &mut line {
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
            None => ("Enter sends as Y | Esc clears | Tab: ring keys | F1-F3 tabs".into(), false),
        };
        let t: String = t.chars().take(120).collect();
        if text.0 != t {
            text.0 = t;
        }
        colour.0 = if err { Color::srgb(1.0, 0.45, 0.4) } else { Color::srgb(0.5, 0.56, 0.68) };
        tf.translation = Vec3::new(left + pad, bottom + 12.0 + line_h + 8.0, 10.0);
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
        assert_eq!(orbit_speed(&cfg, 0, 0.0, 0.0), 0.0, "still with no notes between agents");
        assert!(orbit_speed(&cfg, 0, 0.0, 0.3) > 0.0 && orbit_speed(&cfg, 0, 0.0, -0.3) < 0.0, "spin either way");
        assert!(orbit_speed(&cfg, 0, 1.0, 0.0) > 1.0, "warp swings it");
    }

    #[test]
    fn a_note_between_agents_travels_the_short_way() {
        let ring: Vec<String> = ["owner", "claude", "grok", "vibe", "devin"].map(String::from).to_vec();
        assert_eq!(ring_travel(&ring, "owner", "claude"), 1.0, "forward is clockwise");
        assert_eq!(ring_travel(&ring, "claude", "owner"), -1.0, "backward is anticlockwise");
        assert_eq!(ring_travel(&ring, "owner", "devin"), -1.0, "the long way round is not the way");
        let even: Vec<String> = ["owner", "claude", "grok", "vibe"].map(String::from).to_vec();
        assert_eq!(ring_travel(&even, "owner", "grok"), 0.0, "opposite seats, no way is shorter");
        assert_eq!(ring_travel(&ring, "owner", "owner"), 0.0);
        assert_eq!(ring_travel(&ring, "owner", "ghost"), 0.0, "off the ring");
        assert_eq!(spin_kick(&ring, "owner", "claude", 0.1), 0.1);
        assert_eq!(spin_kick(&ring, "claude", "owner", 0.1), -0.1);
    }

    #[test]
    fn a_free_orbit_stays_an_orbit() {
        // The physics check (t12): with nothing steering, pushing or dragging
        // it, a planet on a circular orbit must keep orbiting — the
        // integrator is symplectic (speed first, then position), so the
        // radius may breathe but mustn't run away or collapse.
        let mut cfg = mods::PhysicsCfg::default();
        // Nothing but the core's pull: no steering, no sink, no thrust.
        cfg.drift = 0.0;
        cfg.steering = 0.0;
        let scene = Vec2::new(1280.0, 800.0);
        let r = 300.0;
        let v = (cfg.gravity / r).sqrt();
        let mut b = Body { pos: Vec2::new(r, 0.0), vel: Vec2::new(0.0, -v) }; // clockwise
        let (mut lo, mut hi) = (r, r);
        let (mut went_down, mut went_left) = (false, false);
        for _ in 0..5400 {
            step_body(&mut b, false, 0.0, scene, 1.0 / 60.0, &cfg);
            lo = lo.min(b.pos.length());
            hi = hi.max(b.pos.length());
            went_down |= b.pos.y < -100.0;
            went_left |= b.pos.x < -100.0;
        }
        assert!(lo > 250.0 && hi < 360.0, "the orbit breathes between {lo} and {hi}, it doesn't decay or escape");
        assert!(went_down && went_left, "it really went round, not just wobbled in place");
    }

    #[test]
    fn busy_planets_climb_and_idle_ones_sink() {
        let cfg = mods::PhysicsCfg::default();
        let scene = Vec2::new(1280.0, 800.0);
        // A planet circling steadily at the old ring's radius, steered onto
        // a slow turn.
        let start = Body { pos: Vec2::new(0.0, 240.0), vel: Vec2::new(24.0, 0.0) };
        let circling = |busy| {
            let mut b = start;
            let r0 = b.pos.length();
            for _ in 0..600 {
                step_body(&mut b, busy, 0.1, scene, 1.0 / 60.0, &cfg);
            }
            (r0, b.pos.length())
        };
        let (r0, busy_r) = circling(true);
        assert!(busy_r > r0 + 30.0, "thrust carries a working planet to a higher orbit: {r0} -> {busy_r}");
        let (_, idle_r) = circling(false);
        assert!(idle_r < 150.0, "an idle planet keeps orbiting but drifts in: {r0} -> {idle_r}");
        // Even one that sank all the way in can climb back out at work.
        let mut sunken = Body { pos: Vec2::new(0.0, cfg.min_radius), vel: Vec2::new(9.0, 0.0) };
        for _ in 0..1800 {
            step_body(&mut sunken, true, 0.1, scene, 1.0 / 60.0, &cfg);
        }
        assert!(sunken.pos.length() > 200.0, "thrust is a multiple of the local pull, so it escapes: {}", sunken.pos.length());
    }

    #[test]
    fn a_planet_never_falls_in_the_core_or_off_the_screen() {
        let cfg = mods::PhysicsCfg::default();
        let scene = Vec2::new(1280.0, 800.0);
        // Straight at the singularity: it stops at the floor and keeps
        // whatever sideways motion it had.
        let mut falling = Body { pos: Vec2::new(200.0, 0.0), vel: Vec2::new(-80.0, 30.0) };
        for _ in 0..1200 {
            step_body(&mut falling, false, 0.1, scene, 1.0 / 60.0, &cfg);
        }
        assert!(falling.pos.length() >= cfg.min_radius - 0.5, "stops at the floor: {}", falling.pos.length());
        // Flung at the screen's edge: reflected back into the scene.
        let mut flung = Body { pos: Vec2::new(0.0, 100.0), vel: Vec2::new(0.0, 400.0) };
        for _ in 0..600 {
            step_body(&mut flung, false, 0.1, scene, 1.0 / 60.0, &cfg);
        }
        assert!(flung.pos.y <= scene.y * 0.5 - cfg.edge_margin + 0.5, "never over the edge: {}", flung.pos.y);
        assert!(flung.vel.y < 0.0, "it bounces back in, not stuck at the wall");
        // Whatever happens, a planet stays inside the scene's margins.
        let mut wild = Body { pos: Vec2::new(-500.0, 300.0), vel: Vec2::new(600.0, -600.0) };
        for _ in 0..3600 {
            step_body(&mut wild, true, 1.5, scene, 1.0 / 60.0, &cfg);
        }
        assert!(wild.pos.x.abs() <= scene.x * 0.5 && wild.pos.y.abs() <= scene.y * 0.5);
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
    fn a_longer_note_jumps_further_and_coasts_longer() {
        let cfg = mods::TypingCfg::default();
        let short = send_jump(50, 1, &cfg);
        let long = send_jump(200, 1, &cfg);
        assert!((long - 4.0 * short).abs() < 1e-5, "proportional to length");
        assert_eq!(send_jump(10_000, 1, &cfg), cfg.jump_max, "capped");
        assert_eq!(send_jump(0, 1, &cfg), 0.0);
        // Steady drain: the coast time is the jump over the decay rate.
        let decay = mods::WarpCfg::default().decay;
        assert!((long / decay - 4.0 * short / decay).abs() < 1e-4);
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
    fn notes_lose_the_glyphs_the_font_lacks() {
        assert_eq!(plain("on \u{2014} off \u{2192} done\u{2026} \u{201c}ok\u{201d}"), "on -- off -> done... \"ok\"");
        assert_eq!(plain("plain ascii stays"), "plain ascii stays");
    }

    #[test]
    fn activity_and_notify_tabs_read_like_the_chat_screen() {
        let mut rows = VecDeque::new();
        rows.push_back(Activity { time: 1000.0, seat: "vibe".into(), state: "busy".into(), doing: "note from Y: hi\u{2026}".into() });
        rows.push_back(Activity { time: 1090.0, seat: "owner".into(), state: "idle".into(), doing: String::new() });
        let text: String = activity_spans(&rows, 0, 80, 1100.0).into_iter().map(|(t, _)| t).collect();
        assert_eq!(text, "     1m vibe busy  note from Y: hi...\n    10s Y idle");
        let mut notes = VecDeque::new();
        notes.push_back(Notice { time: 40.0, text: "Note from claude \u{2014} done".into() });
        let text: String = notice_spans(&notes, 0, 50, 100.0).into_iter().map(|(t, _)| t).collect();
        assert_eq!(text, "     1m  Note from claude -- done");
    }

    #[test]
    fn stars_splash_on_the_wall_they_reach() {
        let half = Vec2::new(400.0, 300.0);
        assert_eq!(wall_hit(Vec2::new(10.0, 10.0), half), None, "inside: no splash");
        assert_eq!(wall_hit(Vec2::new(0.0, 310.0), half), Some((0, 0.5)), "straight up: the top, in the middle");
        assert_eq!(wall_hit(Vec2::new(410.0, 0.0), half), Some((1, 0.5)));
        assert_eq!(wall_hit(Vec2::new(0.0, -301.0), half).unwrap().0, 2);
        let (edge, t) = wall_hit(Vec2::new(-420.0, -150.0), half).unwrap();
        assert_eq!(edge, 3);
        assert!(t < 0.5, "lower half of the left wall");
    }

    #[test]
    fn splashed_light_spills_spreads_and_fades() {
        let cfg = mods::SplashCfg::default();
        let mut s = Splash::default();
        s.fit(16);
        s.hit(0, 0.5, Vec3::ONE, &cfg);
        let mid = s.edges[0][8].x;
        assert!(mid > 0.0 && s.edges[0][9].x > 0.0 && s.edges[0][9].x < mid, "spills to its neighbours, less");
        assert_eq!(s.edges[1][8], Vec3::ZERO, "other walls untouched");
        let total = |s: &Splash| s.edges[0].iter().map(|v| v.x).sum::<f32>();
        let before = total(&s);
        s.settle(0.1, &cfg);
        assert!(total(&s) < before, "fades");
        assert!(s.edges[0][5].x > 0.0, "spreads along the wall");
        for _ in 0..600 {
            s.settle(0.1, &cfg);
        }
        assert!(total(&s) < 0.01, "and is gone after a while");
    }

    #[test]
    fn notes_spin_the_ring_the_way_they_travel() {
        let ring: Vec<String> = ["owner", "claude", "grok", "vibe", "devin"].map(String::from).to_vec();
        assert_eq!(spin_kick(&ring, "claude", "grok", 0.1), 0.1, "one step clockwise");
        assert_eq!(spin_kick(&ring, "grok", "claude", 0.1), -0.1, "and back");
        assert_eq!(spin_kick(&ring, "devin", "claude", 0.1), 0.1, "the short way round wraps");
        assert_eq!(spin_kick(&ring, "claude", "codex", 0.1), 0.0, "off the ring: no spin");
        assert_eq!(spin_kick(&ring, "claude", "claude", 0.1), 0.0);
    }

    #[test]
    fn packets_fire_only_when_work_is_done() {
        assert!(finished_job(Some("busy"), "idle"));
        assert!(!finished_job(Some("idle"), "busy"), "starting isn't finishing");
        assert!(!finished_job(Some("busy"), "stuck"));
        assert!(!finished_job(None, "idle"));
        let t = |id: &str, holder: Option<&str>, open: bool| feed::TaskInfo {
            id: id.into(),
            from: "owner".into(),
            holder: holder.map(String::from),
            open,
        };
        let old = [t("t1", Some("vibe"), false), t("t2", None, true), t("t3", Some("claude"), false)];
        let new = [t("t3", Some("claude"), false)];
        let done: Vec<&str> = finished_tasks(&old, &new).iter().map(|t| t.id.as_str()).collect();
        assert_eq!(done, ["t1"], "held and gone: done; unheld or still held: not");
    }

    #[test]
    fn conduits_bend_with_the_spin_and_end_at_the_core() {
        let seat = Vec2::new(300.0, 0.0);
        let straight = conduit_path(seat, 0.0, 10);
        assert!((straight[0] - seat * 0.9).length() < 0.01 && (straight[10] - Vec2::new(70.0, 0.0)).length() < 0.01);
        let bent = conduit_path(seat, 0.8, 10);
        assert!((bent[10].length() - 70.0).abs() < 0.01, "still reaches the core");
        assert!(bent[10].y < 0.0, "a clockwise spin drags the inner end clockwise");
        let cfg = mods::ConduitsCfg::default();
        assert_eq!(conduit_bend(&cfg, 100.0), cfg.max_bend, "capped");
    }

    #[test]
    fn planets_work_up_the_longer_they_are_busy() {
        assert_eq!(planet_work(0.0, 20.0), 0.0);
        assert!(planet_work(600.0, 20.0) > 0.3 && planet_work(600.0, 20.0) < planet_work(3600.0, 20.0));
        assert!(planet_work(1e9, 20.0) <= 1.0);
    }

    #[test]
    fn the_ring_keeps_a_fixed_order_whatever_the_ranking() {
        let ranked = ["devin", "claude", "owner", "zeta", "vibe"];
        assert_eq!(seat_roster(ranked.into_iter()), ["owner", "claude", "vibe", "devin", "zeta"]);
        let reshuffled = ["vibe", "zeta", "owner", "devin", "claude"];
        assert_eq!(seat_roster(reshuffled.into_iter()), seat_roster(ranked.into_iter()), "ranking changes nothing");
        // Each seat's slot ignores who else is online.
        let roster = seat_roster(ranked.into_iter());
        let a = ring_positions(&roster, Vec2::new(1000.0, 1000.0), 0.3);
        let b = ring_positions(&roster, Vec2::new(1000.0, 1000.0), 0.3);
        assert_eq!(a["vibe"].0, b["vibe"].0);
    }

    #[test]
    fn a_note_to_more_agents_jumps_further() {
        let cfg = mods::TypingCfg::default();
        let seats: Vec<String> = ["owner", "claude", "grok", "vibe", "devin"].map(String::from).to_vec();
        assert_eq!(recipients("vibe", &seats), 1);
        assert_eq!(recipients("vibe, devin", &seats), 2);
        assert_eq!(recipients("all", &seats), 4, "every agent, not Y");
        assert_eq!(recipients("all -grok", &seats), 3);
        assert!(send_jump(100, 3, &cfg) > send_jump(100, 1, &cfg) * 2.5);
    }

    #[test]
    fn ages_read_short() {
        assert_eq!(ago(42.0), "42s");
        assert_eq!(ago(4800.0), "1h 20m");
        assert_eq!(ago(241499.0), "2d 19h");
    }
}
