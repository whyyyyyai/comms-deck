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
mod kitten;
mod mods;
mod sys;

use bevy::{
    asset::uuid_handle,
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseWheel,
    },
    text::{TextBounds, TextLayout, TextLayoutInfo},
    post_process::bloom::Bloom,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::{Shader, ShaderRef},
    sprite::Anchor,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin},
    winit::{UpdateMode, WinitSettings},
};
use feed::{Activity, ChatNote, Msg, Notice, NoteInfo, SeatInfo, SubagentInfo, TaskInfo};
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

mod kitten_view;
mod left_panel;
mod right_panel;
use kitten_view::KittenView;

fn main() {
    install_crash_log();
    watch_visibility();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || feed::run(tx));
    App::new()
        .insert_resource(Splash::default())
        .insert_resource(ClearColor(Color::srgb(0.01, 0.012, 0.03)))
        .insert_resource(Feed(Mutex::new(rx)))
        .insert_resource(Deck::default())
        .insert_resource(KittenView::default())
        .insert_resource(left_panel::Left::default())
        .insert_resource(Mods::default())
        .insert_resource(View::default())
        .insert_resource(Looks::default())
        .insert_resource(Chat::default())
        .insert_resource(Layout::default())
        .insert_resource(Rng(0x9E37_79B9_7F4A_7C15))
        // 30 fps until the mods say otherwise ([look] fps): it is a display,
        // not a game, and half the CPU of drawing at the refresh rate.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "warp core".into(),
                        // WARP_CORE_SCALE=1 draws at 1x on the 2x display: a
                        // quarter of the pixels for the GPU (a test, 06:31).
                        resolution: match std::env::var("WARP_CORE_SCALE").ok().and_then(|v| v.parse::<f32>().ok()) {
                            Some(x) => bevy::window::WindowResolution::default().with_scale_factor_override(x.clamp(0.5, 4.0)),
                            None => default(),
                        },
                        // The Wayland app id: the scratchpad keeper places windows by it.
                        name: Some("warp-core".into()),
                        ..default()
                    }),
                    ..default()
                })
                // t28: a few small systems at 30 fps don't need a thread per
                // core; the idle workers' waking and parking cost more CPU
                // than they saved. WARP_CORE_THREADS overrides.
                .set(TaskPoolPlugin { task_pool_options: TaskPoolOptions::with_num_threads(worker_threads()) }),
        )
        .add_plugins(DrainOnStop)
        .add_plugins((
            Material2dPlugin::<RingMaterial>::default(),
            Material2dPlugin::<CoreMaterial>::default(),
            Material2dPlugin::<PlanetMaterial>::default(),
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                (watch_mods, pace_frames, read_feed, layout_view, apply_rules, layout_seats, kitten_view::kitten_step, left_panel::left_keys, layout_tasks, layout_held_tasks, pointer, chat_input, keys).chain(),
                (
                    glow_seats,
                    draw_rings,
                    pulse_core,
                    fly_comets,
                    kitten_view::draw_kitten,
                    move_stars,
                    colour_stars,
                    draw_splash,
                    draw_links,
                    drive_planets,
                    draw_moons_and_marks,
                    hud,
                    sys_panel,
                    task_panel,
                    left_panel::left_panel,
                    tooltip,
                    chat_results,
                    draw_chat,
                    right_panel::right_panel,
                    health_log,
                )
                    .chain(),
            )
                .chain(),
        )
        .run();
}

/// Once a minute, to the journal: what the warp core is carrying, so a cost
/// that grows the longer it runs shows which part is growing (Y, 06:20).
fn health_log(
    real: Res<Time<Real>>,
    all: Query<Entity>,
    mats: Res<Assets<ColorMaterial>>,
    meshes: Res<Assets<Mesh>>,
    mut acc: Local<(u32, f32, f32, f32)>,
) {
    let dt = real.delta_secs();
    acc.0 += 1;
    acc.1 += dt;
    acc.2 = acc.2.max(dt);
    acc.3 += dt;
    if dt > 0.25 && real.elapsed_secs() > 5.0 {
        let shown = VISIBLE.load(std::sync::atomic::Ordering::Relaxed);
        eprintln!("warp-core hitch: {:.0} ms frame, {}", dt * 1000.0, if shown { "shown" } else { "hidden" });
    }
    if acc.3 < 60.0 {
        return;
    }
    let shown = VISIBLE.load(std::sync::atomic::Ordering::Relaxed);
    eprintln!(
        "warp-core health: up {:.0} min, {} entities, {} colour materials, {} meshes, {:.1} fps, worst frame {:.0} ms, {}",
        real.elapsed_secs() / 60.0,
        all.iter().count(),
        mats.len(),
        meshes.len(),
        acc.0 as f32 / acc.1,
        acc.2 * 1000.0,
        if shown { "shown" } else { "hidden" }
    );
    *acc = (0, 0.0, 0.0, 0.0);
}

// ---------------------------------------------------------------- stopping

/// Set by SIGTERM/SIGINT; the render thread sees it after its next submit.
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_stop(_: libc::c_int) {
    STOP.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Stop without crashing the GPU. A `systemctl stop` used to kill the warp
/// core mid-frame, its GPU jobs still in flight, and the M1's driver faulted
/// on the torn-down memory: 2026-11-27 05:19 (then a restart loop and a
/// reboot at 05:31) and 05:58, both at the moment the old copy was stopped.
/// Now a stop waits for the frame in hand to be submitted and the GPU to go
/// idle, then leaves at once, before anything is freed under the GPU.
struct DrainOnStop;

impl Plugin for DrainOnStop {
    fn build(&self, app: &mut App) {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            libc::signal(libc::SIGTERM, on_stop as *const () as libc::sighandler_t);
            libc::signal(libc::SIGINT, on_stop as *const () as libc::sighandler_t);
        }
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render.add_systems(bevy::render::Render, drain_on_stop.in_set(bevy::render::RenderSystems::Cleanup));
        }
    }
}

fn drain_on_stop(device: Res<bevy::render::renderer::RenderDevice>) {
    if !STOP.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let wait = bevy::render::render_resource::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(3)),
    };
    let idle = device.poll(wait);
    eprintln!("warp-core: stopping; GPU drained: {}", idle.is_ok());
    std::process::exit(0);
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
    /// has a logo, paint it in the seat's colour, brightness
    logo: Vec4,
}

/// mods/shaders/planet.wgsl: inside a seat, what it's doing.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
struct PlanetMaterial {
    #[uniform(0)]
    u: PlanetUniform,
    /// The seat's logo, drawn on the surface (mods/logos/<seat>.png).
    #[texture(1)]
    #[sampler(2)]
    logo: Option<Handle<Image>>,
}

/// The seats' logos from mods/logos/<seat>.png, read once at start. `true`
/// when the logo is one colour, so the planet paints it in the seat's own.
#[derive(Resource, Default)]
struct Logos(HashMap<String, (Handle<Image>, bool)>);

/// Every mods/logos/*.png that decodes, by seat name.
fn load_logos(dir: &std::path::Path, images: &mut Assets<Image>) -> Logos {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
    let mut logos = Logos::default();
    let Ok(entries) = std::fs::read_dir(dir) else { return logos };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("png") {
            continue;
        }
        let Some(seat) = path.file_stem().and_then(|x| x.to_str()).map(String::from) else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(img) = Image::from_buffer(
            &bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::linear(),
            RenderAssetUsages::RENDER_WORLD,
        ) else {
            continue;
        };
        let mono = img.data.as_deref().is_some_and(one_colour);
        logos.0.insert(seat, (images.add(img), mono));
    }
    logos
}

/// True when every visible pixel of an RGBA8 image is grey: a one-colour
/// logo, painted in the seat's colour rather than its own.
fn one_colour(rgba: &[u8]) -> bool {
    rgba.chunks_exact(4).filter(|p| p[3] > 32).all(|p| {
        let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
        hi - lo < 24
    })
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
    /// Has it reached the core and been taken in (t29)?
    absorbed: bool,
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
    /// The core (t29): energy fed by the packets it takes in, decaying; its
    /// swell and swell speed (a damped spring each packet kicks); and the
    /// phase of its breathing, carried on so a change of pace never jumps.
    core_energy: f32,
    core_swell: f32,
    core_swell_v: f32,
    core_phase: f32,
    core_rest: f32,
    /// The accretion disk's angle (radians), carried on frame to frame so a
    /// change of pace bends its turning instead of jumping the swirl, and
    /// how fast it turns now. The stars leave the core on the same turn.
    disk_angle: f32,
    disk_rate: f32,
    /// Each seat's last state from activity, to see a job finish.
    last_state: HashMap<String, String>,
    /// The load balancer's board: open tasks orbit the core, held ones are
    /// electrons on their holder's planet (t20).
    tasks: Vec<TaskInfo>,
    /// Subagents seats say they spawned (t30), drawn as moons of their parent.
    subagents: Vec<SubagentInfo>,
    /// How far round the task ring has turned (radians).
    task_orbit: f32,
    /// The task ids the ring was last built from.
    tasks_built: Vec<String>,
    /// The held task ids the electron orbits were last built from (t20).
    held_built: Vec<String>,
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

/// Where things go this frame: the scene (ring, core, HUD) fills the top of
/// the central column, the chat strip runs along its bottom, the left panel
/// sits on the left. World x runs -scene.x/2 .. scene.x/2 across the central
/// column; the chat strip is the `panel`-high band just below the scene.
#[derive(Resource, Default)]
struct Layout {
    scene: Vec2,
    /// The chat strip's height.
    panel: f32,
    /// The left panel's width (t87), 0 when it's off.
    left: f32,
    /// The right edge's width: the Activity/Notify panel when docked
    /// bottom, the whole chat column when docked right.
    right: f32,
    /// Which layout the chat uses.
    dock: mods::ChatDock,
}

impl Layout {
    fn half(&self) -> Vec2 {
        self.scene * 0.5
    }
    /// Is this world point over the chat (strip or column)?
    fn in_panel(&self, p: Vec2) -> bool {
        match self.dock {
            mods::ChatDock::Bottom => {
                self.panel > 0.0
                    && p.x.abs() <= self.scene.x * 0.5
                    && p.y < -self.scene.y * 0.5
                    && p.y >= -self.scene.y * 0.5 - self.panel
            }
            mods::ChatDock::Right => self.right > 0.0 && p.x > self.scene.x * 0.5,
        }
    }
}

/// The chat column's width for a window: a share of it, kept readable.
fn panel_width(window_w: f32, cfg: &mods::ChatCfg) -> f32 {
    if !cfg.show {
        return 0.0;
    }
    (window_w * cfg.width).clamp(300.0, 600.0).min(window_w * 0.6)
}

/// The chat strip's height for a window: a share of it, kept readable.
fn strip_height(window_h: f32, cfg: &mods::ChatCfg) -> f32 {
    if !cfg.show {
        return 0.0;
    }
    (window_h * cfg.height).clamp(100.0, window_h * 0.45)
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
    drawn_size: f32,
    /// The right panel's tab (Y, 02:50: "you should still put F2 and F3 on
    /// the right"): Activity or Notify. Comms lives in the bottom strip.
    side: Tab,
    side_dirty: bool,
    /// Whether the right panel is shown (set by layout_view from [chat] side).
    side_on: bool,
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
            drawn_size: 0.0,
            side: Tab::Activity,
            side_dirty: true,
            side_on: false,
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
/// "1.x", x the iteration: install.sh counts every install into
/// `iteration` beside the binary (Y, 2026-11-27). "1.?" when it is missing.
fn version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let n = std::env::current_exe()
            .ok()
            .and_then(|e| std::fs::read_to_string(e.with_file_name("iteration")).ok())
            .and_then(|t| t.trim().parse::<u32>().ok());
        n.map_or_else(|| "1.?".to_string(), |n| format!("1.{n}"))
    })
}

/// Is the warp core's window on a workspace someone can see? Written by
/// `watch_visibility` once a second; true until it knows otherwise.
static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// t28: whether the warp-core window is on a shown workspace (its
/// scratchpad open, or its workspace active on a monitor). Hidden, it draws
/// at [look] fps_hidden. Hyprland's event socket says the moment a workspace
/// or the scratchpad changes, so tabbing back flips to full speed within a
/// frame (Y, 06:18: snappy); a check once a second covers anything missed.
fn watch_visibility() {
    std::thread::Builder::new()
        .name("visibility".into())
        .spawn(|| {
            use std::io::{BufRead, BufReader};
            bring_forward();
            let check = || {
                if let Some(v) = window_visible() {
                    VISIBLE.store(v, std::sync::atomic::Ordering::Relaxed);
                }
            };
            loop {
                check();
                let Some(sock) = hypr_events() else {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                };
                let _ = sock.set_read_timeout(Some(std::time::Duration::from_secs(1)));
                let mut lines = BufReader::new(sock);
                let mut line = String::new();
                loop {
                    line.clear();
                    match lines.read_line(&mut line) {
                        Ok(0) => break, // Hyprland went away: reconnect
                        Ok(_) if shows_change(&line) => check(),
                        Ok(_) => {}
                        Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => check(),
                        Err(_) => break,
                    }
                }
            }
        })
        .ok();
}

/// After a start (a restart, an install), show the warp core in front once
/// its window is up (Y, 06:19). Hyprland 0.57 takes Lua dispatchers: the old
/// `focuswindow class:...` form is a parse error there, which is why 1.36's
/// version never worked. A scratchpad is opened the way Super+S does it.
fn bring_forward() {
    for _ in 0..30 {
        let clients = hyprctl_json("clients");
        let monitors = hyprctl_json("monitors");
        let ws = clients.as_ref().and_then(|c| {
            c.as_array()?.iter().find(|c| c["class"] == "warp-core").and_then(|c| c["workspace"]["name"].as_str().map(String::from))
        });
        if let (Some(ws), Some(clients), Some(monitors)) = (ws, clients, monitors) {
            if !shown(&clients, &monitors) {
                if let Some(cmd) = show_dispatch(&ws) {
                    let _ = std::process::Command::new("hyprctl").args(["dispatch", &cmd]).output();
                }
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// The Lua dispatcher that shows workspace `ws`.
fn show_dispatch(ws: &str) -> Option<String> {
    let safe = |n: &str| n.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    match ws.strip_prefix("special:") {
        Some(name) if safe(name) => Some(format!("hl.dsp.workspace.toggle_special(\"{name}\")")),
        Some(_) => None,
        None => ws.parse::<i64>().ok().map(|n| format!("hl.dsp.focus({{ workspace = {n} }})")),
    }
}

fn hypr_events() -> Option<std::os::unix::net::UnixStream> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let run = std::env::var("XDG_RUNTIME_DIR").ok()?;
    std::os::unix::net::UnixStream::connect(format!("{run}/hypr/{sig}/.socket2.sock")).ok()
}

/// Hyprland events that can show or hide a window.
fn shows_change(line: &str) -> bool {
    let ev = line.split(">>").next().unwrap_or("");
    matches!(
        ev,
        "workspace" | "workspacev2" | "activespecial" | "activespecialv2" | "focusedmon" | "focusedmonv2" | "movewindow" | "movewindowv2" | "openwindow" | "closewindow" | "monitoradded" | "monitorremoved"
    )
}

fn hyprctl_json(what: &str) -> Option<serde_json::Value> {
    let out = std::process::Command::new("hyprctl").args([what, "-j"]).output().ok()?;
    out.status.success().then(|| serde_json::from_slice(&out.stdout).ok()).flatten()
}

/// None when Hyprland can't say (then nothing changes).
fn window_visible() -> Option<bool> {
    let clients = hyprctl_json("clients")?;
    let monitors = hyprctl_json("monitors")?;
    Some(shown(&clients, &monitors))
}

fn shown(clients: &serde_json::Value, monitors: &serde_json::Value) -> bool {
    let Some(ws) = clients
        .as_array()
        .and_then(|cs| cs.iter().find(|c| c["class"] == "warp-core"))
        .and_then(|c| c["workspace"]["name"].as_str())
    else {
        return true; // no window found: don't starve it
    };
    monitors.as_array().is_some_and(|ms| {
        ms.iter().any(|m| m["specialWorkspace"]["name"] == ws || m["activeWorkspace"]["name"] == ws)
    })
}

/// Sets the frame rate each frame: the mods' fps while shown, fps_hidden
/// while hidden.
///
/// At 60 fps or more it draws continuously and the display's vsync paces it:
/// a 16.7 ms timer on top of a 60 Hz vsync slips past every other refresh,
/// and 1.35 ran at 33 fps shown for exactly that reason (health log, 06:27).
fn pace_frames(mods: Res<Mods>, mut winit: ResMut<WinitSettings>) {
    let (focused, unfocused) = if VISIBLE.load(std::sync::atomic::Ordering::Relaxed) {
        (frame_mode(mods.cfg.look.fps), frame_mode(mods.cfg.look.fps_unfocused))
    } else {
        (frame_mode(mods.cfg.look.fps_hidden), frame_mode(mods.cfg.look.fps_hidden))
    };
    let want = (focused, unfocused);
    if winit.focused_mode != want.0 || winit.unfocused_mode != want.1 {
        winit.focused_mode = want.0;
        winit.unfocused_mode = want.1;
    }
}

fn msaa_for(n: u32) -> bevy::render::view::Msaa {
    use bevy::render::view::Msaa;
    match n {
        0 | 1 => Msaa::Off,
        2 | 3 => Msaa::Sample2,
        _ => Msaa::Sample4,
    }
}

fn frame_mode(fps: f32) -> UpdateMode {
    if fps >= 59.0 {
        UpdateMode::Continuous
    } else {
        UpdateMode::reactive(std::time::Duration::from_secs_f32(1.0 / fps.clamp(1.0, 120.0)))
    }
}

fn worker_threads() -> usize {
    std::env::var("WARP_CORE_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(2).clamp(1, 16)
}

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

/// What the planets' gravity on each other adds to each one (t18): the
/// pull between two is `mutual * mass / dist²`, softened under `soften`
/// and switched off for a pair that touches (t46), so it can't glue them.
fn mutual_pulls(bodies: &[Body], masses: &[f32], pc: &mods::PhysicsCfg) -> Vec<Vec2> {
    let mut pull = vec![Vec2::ZERO; bodies.len()];
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            let d = bodies[j].pos - bodies[i].pos;
            if d.length_squared() < pc.contact * pc.contact {
                continue;
            }
            let d2 = d.length_squared().max(pc.soften * pc.soften);
            let a = pc.mutual / d2 / d2.sqrt();
            pull[i] += d * (a * masses[j]);
            pull[j] -= d * (a * masses[i]);
        }
    }
    pull
}

/// Planets touch (t46): any two whose centres are closer than `contact`
/// are pushed apart until they just touch, and if they were closing they
/// bounce, keeping `restitution` of the closing speed. Without this the
/// mutual pull drew a pair together and the steering matched their speeds,
/// so they rode round the ring stuck to each other. Equal masses: a contact
/// is about the drawn planets, not their context weight. A pair sitting
/// exactly on top of each other is split along a fixed direction per pair,
/// so the result never depends on a random draw.
fn resolve_contacts(bodies: &mut [Body], contact: f32, restitution: f32) {
    if contact <= 0.0 {
        return;
    }
    for i in 0..bodies.len() {
        for j in (i + 1)..bodies.len() {
            let d = bodies[j].pos - bodies[i].pos;
            let dist = d.length();
            if dist >= contact {
                continue;
            }
            let n = if dist > 1e-3 {
                d / dist
            } else {
                let a = (i * 7 + j * 13) as f32;
                Vec2::new(a.cos(), a.sin())
            };
            let push = n * ((contact - dist) * 0.5);
            bodies[i].pos -= push;
            bodies[j].pos += push;
            let closing = (bodies[j].vel - bodies[i].vel).dot(n);
            if closing < 0.0 {
                let kick = n * (closing * (1.0 + restitution) * 0.5);
                bodies[i].vel += kick;
                bodies[j].vel -= kick;
            }
        }
    }
}

/// One planet's mass (t18): its share of the crew's context pool, scaled so
/// an even split weighs 1 a head. A seat on the ring that reports no
/// context weighs the average; with no figures at all everyone weighs 1.
fn body_mass(shares: &[(String, f32)], name: &str) -> f32 {
    let n = shares.len();
    if n == 0 {
        return 1.0;
    }
    match shares.iter().find(|(s, _)| s == name) {
        Some((_, share)) => share * n as f32,
        None => 1.0,
    }
}

/// How big a body holding `share` of the context pool is drawn (t18), for
/// planets and stars alike. Relative to the crew: an even split draws at
/// 1.0, and `span` is how far a body may shrink or grow from that (as a
/// fraction). With no figures at all everything draws at 1.0.
fn body_size(share: f32, seats: usize, span: f32) -> f32 {
    let even = 1.0 / seats.max(1) as f32;
    if seats == 0 || even <= 0.0 {
        return 1.0;
    }
    (1.0 + span * (share / even - 1.0)).clamp(1.0 - span * 0.5, 1.0 + span * 0.5)
}

/// The radius of the ring a working agent's planet climbs to and holds
/// (t52): `ring` of the room between the core and the nearest wall, so
/// the whole circle stays clear of the screen's edges.
fn ring_radius(scene: Vec2, cfg: &mods::PhysicsCfg) -> f32 {
    let wall = scene * 0.5 - cfg.edge_margin;
    (cfg.ring * wall.x.min(wall.y)).max(cfg.min_radius)
}

/// One step of the planets' physics (t13). The core is a singularity: its
/// pull is `gravity / r²` towards the centre. A busy seat climbs to the
/// ring (`ring_radius`) and holds a circle there (t52); an idle one keeps
/// its orbit (t18), and every planet's in-and-out swing is damped. The ring's turning (drift, traffic, warp, the notes' spin kicks)
/// rides on top of each planet's own circular speed (`turn_share` of
/// `turning * r`), so the whole ring still answers the deck the way it
/// did. `pull` is what the other planets' gravity
/// adds for this one (t18). No planet is ever lost over the screen's edge:
/// past the margin it is reflected back in, keeping `bounce` of its speed.
/// Inside `min_radius` nothing falls further in: the planet skims the
/// singularity and keeps circling.
fn step_body(body: &mut Body, busy: bool, turning: f32, scene: Vec2, dt: f32, cfg: &mods::PhysicsCfg, pull: Vec2) {
    let dt = dt.min(0.1); // a stall mustn't fling anything
    let p = body.pos;
    let r = p.length().max(1.0);
    let out = p / r;
    // Clockwise round the ring (the way the ring turns as its angle grows).
    let tang = Vec2::new(out.y, -out.x);
    // The core's pull, softened at the floor so the sim stays stable there.
    let g = cfg.gravity / (r.max(cfg.min_radius) * r.max(cfg.min_radius));
    // A working agent climbs to the ring and holds it (t52, Y: "a stable
    // ring orbit to the core"): a spring towards the ring's radius, a
    // circle that fits inside the walls. Until now thrust beat the pull
    // all the way out, so a busy planet rode the screen's edge and bounced.
    let half = scene * 0.5;
    let ring = ring_radius(scene, cfg);
    let lift = if busy { cfg.ring_pull * (ring - r) } else { -cfg.drift };
    // Radial speed is damped for every planet, so a kick from a note, a
    // contact or a neighbour's pull settles back onto a circle instead of
    // swinging in and out for ever.
    let calm = -cfg.radial_damping * body.vel.dot(out);
    // Steer the tangential speed onto the orbit's own circular speed plus
    // a share of the ring's turning (t18: fairly stable orbits, still
    // stirred by the deck), capped, so the spin kicks and warp swing are
    // felt but can't throw a planet.
    let v_t = body.vel.dot(tang);
    let target = (g * r).sqrt() + turning * r * cfg.turn_share;
    let steer = ((target - v_t) * cfg.steering).clamp(-cfg.max_steer, cfg.max_steer);
    // On the ring the inward pull is exactly what the circle needs at the
    // planet's speed, so the ring's turning can't push it wide of the ring.
    let hold = if busy { v_t * v_t / r } else { g };
    body.vel += (out * (lift + calm - hold) + tang * steer + pull) * dt;
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
    let wall = half - cfg.edge_margin;
    let keep = cfg.bounce;
    if body.pos.x < -wall.x {
        body.pos.x = -wall.x;
        body.vel.x = body.vel.x.abs() * keep;
    } else if body.pos.x > wall.x {
        body.pos.x = wall.x;
        body.vel.x = -body.vel.x.abs() * keep;
    }
    if body.pos.y < -wall.y {
        body.pos.y = -wall.y;
        body.vel.y = body.vel.y.abs() * keep;
    } else if body.pos.y > wall.y {
        body.pos.y = wall.y;
        body.vel.y = -body.vel.y.abs() * keep;
    }
}

/// One open task orbiting the core, coloured by who assigned it.
#[derive(Component)]
struct TaskNode {
    id: String,
    from: String,
}

/// A held task waiting on its holder: an electron on its own fast orbit of
/// the planet, passing in front of and behind it (t20, Y). `key` is hashed
/// from the id once, so the task always flies the same way.
#[derive(Component)]
struct HeldTask {
    holder: String,
    key: u32,
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

/// The system stats down the scene's left side (t53).
#[derive(Component)]
struct SysPanel;

/// The low-memory alert across the top of the scene (Y, 2026-11-27).
#[derive(Component)]
struct MemAlert;

/// The task list at the bottom of the scene's left side: id, title, holder, how long.
#[derive(Component)]
struct TaskPanel;

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
    /// Drawn size: how much of the context pool the star's seat holds (t18).
    size: f32,
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
    /// The note's log id, so the kitten catches each note once (t78).
    note_id: String,
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
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut core_materials: ResMut<Assets<CoreMaterial>>,
    mut rng: ResMut<Rng>,
) {
    commands.spawn((
        Camera2d,
        Tonemapping::TonyMcMapface,
        Bloom { intensity: 0.28, ..Bloom::NATURAL },
        DebandDither::Enabled,
        msaa_for(mods::Look::default().msaa),
    ));

    commands.insert_resource(load_logos(&mods::mods_dir().join("logos"), &mut images));
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
            Star { dir, dist: 40.0 + rng.next() * 1400.0, speed: 0.4 + rng.next() * 1.2, slot: rng.next(), size: 1.0, colour: Vec3::new(0.9, 0.95, 1.4) },
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
        SysPanel,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(0.5, 0.6, 0.75)),
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 5.0),
    ));
    commands.spawn((
        MemAlert,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(34.0), ..default() },
        TextLayout::justify(Justify::Center),
        TextColor(Color::srgb(1.0, 0.7, 0.3)),
        Anchor::TOP_CENTER,
        Transform::from_xyz(0.0, 0.0, 6.0),
        Visibility::Hidden,
    ));
    commands.spawn((
        TaskPanel,
        Text2d::new(""),
        TextFont { font_size: FontSize::Px(14.0), ..default() },
        TextBounds::new_horizontal(520.0),
        TextColor(Color::srgb(0.62, 0.7, 0.85)),
        Anchor::BOTTOM_LEFT,
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
    mut msaa: Query<&mut bevy::render::view::Msaa>,
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
                winit.focused_mode = frame_mode(c.look.fps);
                winit.unfocused_mode = frame_mode(c.look.fps_unfocused);
                for mut b in &mut bloom {
                    b.intensity = c.look.bloom;
                }
                for mut ms in &mut msaa {
                    let want = msaa_for(c.look.msaa);
                    if *ms != want {
                        *ms = want;
                    }
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
                        deck.packets.push(Packet { seat: a.seat.clone(), at: now, colour: seat_rgb(&a.seat), absorbed: false });
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
            Msg::Subagents(subs) => deck.subagents = subs,
            Msg::Tasks(tasks) => {
                // A held task gone from the board is done: its holder sends a
                // packet in the colour of whoever asked for it.
                let fired: Vec<Packet> = finished_tasks(&deck.tasks, &tasks)
                    .into_iter()
                    .filter_map(|d| Some(Packet { seat: d.holder.clone()?, at: now, colour: seat_rgb(&d.from), absorbed: false }))
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
    // Kept a moment past arrival so the core can take each one in (t29).
    deck.packets.retain(|p| now - p.at < gone + 1.0);
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

/// Splits the window into the scene and the chat strip, and moves the
/// camera so the core sits in the middle of the scene.
fn layout_view(
    windows: Query<&Window>,
    mods: Res<Mods>,
    mut layout: ResMut<Layout>,
    mut chat: ResMut<Chat>,
    mut camera: Query<&mut Transform, With<Camera2d>>,
) {
    let w = window_size(&windows);
    let chat_cfg = &mods.cfg.chat;
    let left = left_panel::left_width(w.x, &mods.cfg.left);
    // Bottom: the strip plus, with `side`, the Activity/Notify panel on the
    // right. Right: one full-height column holding every tab.
    let (strip, right, side_on) = match chat_cfg.dock {
        mods::ChatDock::Bottom => {
            let strip = strip_height(w.y, chat_cfg);
            let right = right_panel::right_width(w.x, chat_cfg);
            (strip, right, right > 0.0)
        }
        mods::ChatDock::Right => (0.0, panel_width(w.x, chat_cfg), false),
    };
    layout.scene = Vec2::new(w.x - left - right, w.y - strip);
    layout.panel = strip;
    layout.left = left;
    layout.right = right;
    layout.dock = chat_cfg.dock;
    if chat.side_on != side_on {
        chat.side_on = side_on;
        chat.dirty = true;
        chat.side_dirty = true;
    }
    if (chat.drawn_size - (strip + right)).abs() > 0.5 {
        chat.drawn_size = strip + right;
        chat.dirty = true;
    }
    for mut tf in &mut camera {
        tf.translation.x = (right - left) * 0.5;
        tf.translation.y = -strip * 0.5;
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
    logos: Res<Logos>,
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
    // Lumo is the quantum kitten (t78): no planet and no body in the sim.
    let kitten_on = kitten_view::on(&mods);
    let names: Vec<String> = roster
        .iter()
        .filter(|n| deck.seat(n).is_some_and(|s| s.online()))
        .filter(|n| !(kitten_on && n.as_str() == kitten_view::SEAT))
        .cloned()
        .collect();
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
                        MeshMaterial2d(planets.add(PlanetMaterial {
                            logo: logos.0.get(name).map(|l| l.0.clone()),
                            ..default()
                        })),
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
    // The planets fly the sim now (t13): gravity, the ring, steering — each
    // one on its own, so working agents climb to a stable ring while idle
    // ones hold a fairly stable circle. On top of the core's pull the
    // planets pull on each other (t18), each weighing its share of the
    // crew's context pool.
    let shares = context_shares(&deck.seats);
    let pc = &mods.cfg.physics;
    let gathered: Vec<(Body, bool, f32)> = nodes
        .iter()
        .map(|(_, node, _)| {
            let busy = deck.seat(&node.name).is_some_and(|s| s.shown == "busy");
            (node.body, busy, body_mass(&shares, &node.name))
        })
        .collect();
    let masses: Vec<f32> = gathered.iter().map(|g| g.2).collect();
    let pull = mutual_pulls(&gathered.iter().map(|g| g.0).collect::<Vec<_>>(), &masses, pc);
    // Depth follows the sim too: the higher a planet stands on the screen
    // (the far side of the tilted orbit), the further behind the core it
    // slides.
    let ry = layout.scene.y * 0.30;
    let mut bodies: Vec<Body> = gathered.iter().map(|g| g.0).collect();
    for (i, body) in bodies.iter_mut().enumerate() {
        step_body(body, gathered[i].1, turning, layout.scene, dt, pc, pull[i]);
    }
    resolve_contacts(&mut bodies, pc.contact, pc.restitution);
    for (i, (_, mut node, mut tf)) in nodes.iter_mut().enumerate() {
        node.body = bodies[i];
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

/// A task dot's z on the core ring (t20): behind the core on the far side
/// of the ellipse, in front on the near side — the same slide the planets
/// make, so waiting tasks pass both ways.
fn task_z(p: Vec2, scene: Vec2, radius: f32) -> f32 {
    let ry = scene.y * 0.30 * radius;
    seat_z((0.5 - 0.5 * (p.y / ry.max(1.0))).clamp(0.0, 1.0))
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
                    Transform::from_xyz(p.x, p.y, task_z(*p, layout.scene, tc.radius)).with_scale(Vec3::splat(tc.size)),
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
            tf.translation.z = task_z(*p, layout.scene, tc.radius);
        }
        if let Some(task) = open.get(i) {
            node.from = task.from.clone();
        }
    }
}

/// A stable hash of a task id (FNV-1a): the electron's phase, pace,
/// direction and radius all come off it, so a task always flies the same
/// way while it waits (t20).
fn task_key(id: &str) -> u32 {
    let mut h = 2166136261u32;
    for b in id.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(16777619);
    }
    h
}

/// Held tasks are electrons (t20, Y): each waits on its own fast orbit of
/// its holder's planet, passing in front of and behind it — the orbit is
/// tipped so the dot slides under the planet on the far side and over it
/// on the near side. Rebuilt when the board's held set changes, like the
/// open ring; a holder that leaves the ring takes its electron with it.
fn layout_held_tasks(
    mut commands: Commands,
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    view: Res<View>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    time: Res<Time>,
    seats: Query<(&SeatNode, &Transform), Without<HeldTask>>,
    mut nodes: Query<(Entity, &mut HeldTask, &mut Transform, &mut Visibility), Without<SeatNode>>,
) {
    let tc = &mods.cfg.tasks;
    let held: Vec<TaskInfo> = deck.tasks.iter().filter(|t| !t.open).cloned().collect();
    let ids: Vec<String> = held.iter().map(|t| t.id.clone()).collect();
    let shown = tc.show && view.tasks;
    if ids != deck.held_built || !shown {
        for (e, _, _, _) in &nodes {
            commands.entity(e).despawn();
        }
        deck.held_built = if shown { ids.clone() } else { Vec::new() };
        if !shown {
            return;
        }
        for task in &held {
            let Some(holder) = task.holder.clone() else { continue };
            commands
                .spawn((
                    HeldTask { holder, key: task_key(&task.id) },
                    Mesh2d(assets.dot.clone()),
                    MeshMaterial2d(materials.add(seat_colour(&task.from, tc.bright))),
                    Transform::from_xyz(0.0, 0.0, 1.0).with_scale(Vec3::splat(tc.size)),
                    Visibility::Inherited,
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
    let t = time.elapsed_secs();
    for (_, node, mut tf, mut vis) in &mut nodes {
        let holder = seats.iter().find(|(n, _)| n.name == node.holder).map(|(_, tf)| *tf);
        let Some(htf) = holder else {
            *vis = Visibility::Hidden;
            continue;
        };
        *vis = Visibility::Inherited;
        let h = node.key;
        let phase = (h % 4096) as f32 / 4096.0 * std::f32::consts::TAU;
        let pace = (1.1 + ((h >> 12) % 2048) as f32 / 2048.0 * 1.9) * if (h >> 23) & 1 == 0 { 1.0 } else { -1.0 };
        let s = htf.scale.x.max(0.1);
        let rad = (44.0 + (h >> 24) as f32 / 255.0 * 20.0) * s;
        let a = phase + pace * t;
        let p = htf.translation.truncate() + Vec2::new(a.cos() * rad, a.sin() * rad * 0.35);
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        // Behind the planet's face on the far side of the orbit, in front on
        // the near side — like an electron dipping under and over.
        tf.translation.z = htf.translation.z - 0.55 * s * a.sin();
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
            // The chat header is the dock's top 38 px: a click picks a tab
            // by thirds along it.
            let header_base = match layout.dock {
                mods::ChatDock::Bottom => -layout.scene.y * 0.5,
                mods::ChatDock::Right => layout.scene.y * 0.5,
            };
            if w.y > header_base - 38.0 {
                let k = match layout.dock {
                    mods::ChatDock::Bottom => ((w.x + layout.scene.x * 0.5) / layout.scene.x.max(1.0) * 3.0).clamp(0.0, 2.0) as usize,
                    mods::ChatDock::Right => ((w.x - layout.scene.x * 0.5) / layout.right.max(1.0) * 3.0).clamp(0.0, 2.0) as usize,
                };
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
    // F1 is the strip's Comms; F2/F3 pick the right panel's tab (when it's
    // on; without it they show in the strip as before).
    if tab != Tab::Comms && chat.side_on {
        if chat.side != tab {
            chat.side = tab;
            chat.side_dirty = true;
        }
        return;
    }
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
    logos: Res<Logos>,
    time: Res<Time>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut nodes: Query<(&mut SeatNode, &MeshMaterial2d<ColorMaterial>, &mut Transform, &Children)>,
    mut labels: Query<&mut Text2d, With<SeatLabel>>,
) {
    let t = time.elapsed_secs();
    let cfg = &mods.cfg;
    // A planet is as big as its seat's share of the context pool (t18).
    let shares = context_shares(&deck.seats);
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
        let share = shares.iter().find(|(n, _)| n == &node.name).map(|(_, s)| *s).unwrap_or(1.0 / shares.len().max(1) as f32);
        tf.scale = Vec3::splat(
            cfg.seats.size * body_size(share, shares.len(), cfg.physics.size_span) * (0.72 + 0.4 * node.depth)
                * (1.0 + node.flash * 0.35)
                * if hovered { 1.12 } else { 1.0 },
        );
        for child in children.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                // No text on the planets (Y, 05:20): a seat with a logo
                // wears it instead of its name (04:42), and its state and
                // waiting notes show in how it looks. Only a seat with no
                // logo keeps its name, and a mod's own labels still show.
                let mut lines: Vec<String> = Vec::new();
                if !logos.0.contains_key(&node.name) {
                    lines.push(label_for(&node.name));
                }
                lines.extend(look.labels.iter().cloned());
                let line = lines.join("\n");
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
    mut deck: ResMut<Deck>,
    mods: Res<Mods>,
    time: Res<Time>,
    mut core: Query<(&mut Transform, &MeshMaterial2d<CoreMaterial>), With<Core>>,
    mut core_materials: ResMut<Assets<CoreMaterial>>,
) {
    let c = mods.cfg.core.clone();
    let t = time.elapsed_secs();
    let dt = time.delta_secs().min(0.1);
    let pace = deck.notes_last(t, 600.0) as f32;
    let heat = crew_heat(&deck.seats);
    let busy = deck.seats.iter().filter(|s| s.shown == "busy").count();
    let online = deck.seats.iter().filter(|s| s.online()).count();
    // Packets that reached the core this frame feed it (t29).
    let secs = mods.cfg.conduits.packet_seconds.max(0.1);
    let mut arrived = 0;
    for pk in deck.packets.iter_mut().filter(|pk| !pk.absorbed && t - pk.at >= secs) {
        pk.absorbed = true;
        arrived += 1;
    }
    let warp = deck.warp;
    let d = &mut *deck;
    d.disk_rate = disk_rate(pace, warp, d.core_energy);
    d.disk_angle = (d.disk_angle + d.disk_rate * dt).rem_euclid(std::f32::consts::TAU * 1024.0);
    let shape = core_step(
        &c,
        CoreState { energy: d.core_energy, swell: d.core_swell, swell_v: d.core_swell_v, phase: d.core_phase, rest: d.core_rest },
        CoreInputs { arrived, busy, online, pace, warp, heat },
        dt,
    );
    d.core_energy = shape.state.energy;
    d.core_swell = shape.state.swell;
    d.core_swell_v = shape.state.swell_v;
    d.core_phase = shape.state.phase;
    d.core_rest = shape.state.rest;
    for (mut tf, mat) in &mut core {
        tf.scale = Vec3::new(170.0 * shape.size, 170.0 * shape.size, 1.0);
        if let Some(mut m) = core_materials.get_mut(&mat.0) {
            m.u = CoreUniform {
                // cool.w carries the photon ring's flare (no new field, so
                // the uniform's size and the shader's binding stay as they are).
                cool: v4(c.cool, shape.flare),
                hot: v4(c.hot, 1.0),
                // The shader's churn runs on the core's own energy too.
                // x is the disk's carried angle, not the clock (t29/Y 05:23).
                params: Vec4::new(d.disk_angle, heat, warp, (pace.min(30.0) / 30.0).max(shape.state.energy.min(1.0))),
            };
        }
    }
    // No containment rings round the core: Y found the two arc patterns
    // spinning round it unnecessary (2026-11-27) — the black hole alone
    // marks the centre, the planets and their orbits do the rest.
}

/// 0..1: the mean of the online seats' fullest meters (seats without
/// usage files don't count either way).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct CoreState {
    energy: f32,
    swell: f32,
    swell_v: f32,
    phase: f32,
    /// The drawn size, eased toward where the energy and the crew put it.
    rest: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct CoreInputs {
    /// Packets taken in this frame.
    arrived: usize,
    busy: usize,
    online: usize,
    /// Notes in the last 10 minutes.
    pace: f32,
    warp: f32,
    /// 0..1, how full the crew's context meters are.
    heat: f32,
}

struct CoreShape {
    state: CoreState,
    /// The core's drawn size, 1.0 = `[core] size`.
    size: f32,
    /// 0..1: how bright the photon ring flares from packets just taken in.
    flare: f32,
}

/// One step of the core. Y didn't like it swelling in and out (06:19), so
/// its size no longer oscillates: it eases (over `[core] settle` seconds)
/// toward a resting size set by its energy and the share of the crew at
/// work, and never wobbles. What it takes in shows as light instead: each
/// packet adds energy and kicks a damped spring whose ringing is the photon
/// ring's flare, and energy turns the disk faster (`disk_rate`).
fn core_step(c: &mods::CoreCfg, s: CoreState, i: CoreInputs, dt: f32) -> CoreShape {
    let energy = (s.energy * (-c.energy_decay.max(0.0) * dt).exp() + i.arrived as f32 * c.per_packet).min(c.max_energy);
    // The spring: pulled back to rest, damped, kicked outward per packet.
    let w = c.ring_freq * std::f32::consts::TAU;
    let mut swell_v = s.swell_v + i.arrived as f32 * c.kick;
    swell_v += (-w * w * s.swell - 2.0 * c.ring_damping * w * swell_v) * dt;
    let swell = (s.swell + swell_v * dt).clamp(-0.5, 1.0);
    // The breathing phase still runs (the shader's churn can use it), but
    // no longer moves the size.
    let rate = c.breath_rate + c.rate_per_energy * energy + c.rate_per_note * i.pace.min(30.0) + c.rate_per_heat * i.heat;
    let phase = (s.phase + rate * dt).rem_euclid(std::f32::consts::TAU * 64.0);
    let busy_share = if i.online == 0 { 0.0 } else { i.busy as f32 / i.online as f32 };
    let target = 1.0 + c.grow_per_energy * energy + c.grow_busy * busy_share;
    let from = if s.rest > 0.0 { s.rest } else { target };
    let rest = from + (target - from) * (1.0 - (-dt / c.settle.max(0.05)).exp());
    let flare = (swell.max(0.0) * 2.0).min(1.0);
    CoreShape { state: CoreState { energy, swell, swell_v, phase, rest }, size: (c.size * rest).max(0.05), flare }
}

/// How fast the core's disk turns (radians a second at its edge): slow
/// when quiet, faster with traffic and in warp.
fn disk_rate(pace: f32, warp: f32, energy: f32) -> f32 {
    0.35 + (pace.min(30.0) / 30.0) * 1.2 + warp * 1.5 + energy * 0.3
}

fn crew_heat(seats: &[SeatInfo]) -> f32 {
    let full: Vec<f64> = seats.iter().filter(|s| s.online()).filter_map(|s| s.fullest()).collect();
    if full.is_empty() {
        return 0.0;
    }
    (full.iter().sum::<f64>() / full.len() as f64 / 100.0).clamp(0.0, 1.0) as f32
}

/// No standing lines to the core (Y, t19): a planet's path in is drawn only
/// while a packet is on it, as the packet's own fading tail. The path still
/// bends with the ring's turning, so the packet curves in the way it did.
fn draw_links(deck: Res<Deck>, mods: Res<Mods>, nodes: Query<(&SeatNode, &Transform)>, time: Res<Time>, mut gizmos: Gizmos) {
    if deck.packets.is_empty() {
        return;
    }
    let t = time.elapsed_secs();
    let bend = conduit_bend(&mods.cfg.conduits, deck.turning);
    let secs = mods.cfg.conduits.packet_seconds.max(0.1);
    for (node, tf) in &nodes {
        let mut flying = deck.packets.iter().filter(|pk| pk.seat == node.name && t - pk.at < secs).peekable();
        if flying.peek().is_none() {
            continue;
        }
        let path = conduit_path(tf.translation.truncate(), bend, 48);
        let dim = if deck.focus.as_ref().is_some_and(|f| *f != node.name) { 0.25 } else { 1.0 };
        for pk in flying {
            let k = ((t - pk.at) / secs).clamp(0.0, 1.0);
            let (head, tail) = packet_span(path.len(), k, 0.3);
            gizmos.linestrip_gradient_2d((tail..=head).map(|i| {
                let f = if head == tail { 1.0 } else { (i - tail) as f32 / (head - tail) as f32 };
                (path[i], lin(pk.colour, 3.0 * dim * f * f))
            }));
            gizmos.circle_2d(Isometry2d::from_translation(path[head]), 3.0, lin(pk.colour, 5.0 * dim));
        }
    }
}

/// Which points of an `n`-point path a packet `k` (0..1) of the way along
/// covers: its head, and the start of a tail `len` (a fraction of the path)
/// long behind it.
fn packet_span(n: usize, k: f32, len: f32) -> (usize, usize) {
    let last = n.saturating_sub(1);
    let head = ((k.clamp(0.0, 1.0) * last as f32).round() as usize).min(last);
    let tail = (((k - len).max(0.0) * last as f32).round() as usize).min(head);
    (head, tail)
}

/// Drives each planet's inside: work grows the longer it has been busy.
fn drive_planets(
    deck: Res<Deck>,
    mods: Res<Mods>,
    time: Res<Time>,
    nodes: Query<(&SeatNode, &Children)>,
    mut faces: Query<(&MeshMaterial2d<PlanetMaterial>, &mut Visibility), With<PlanetFace>>,
    mut mats: ResMut<Assets<PlanetMaterial>>,
    logos: Res<Logos>,
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
            // The logo dims with the planet: its state, the far side of the
            // orbit, another seat in focus.
            let mut bright = (mods.cfg.brightness(&seat.shown) / 2.0).clamp(0.25, 1.6) * (0.55 + 0.45 * node.depth);
            if deck.focus.as_ref().is_some_and(|f| *f != node.name) {
                bright *= 0.3;
            }
            if let Some(mut m) = mats.get_mut(&mat.0) {
                m.u = PlanetUniform {
                    colour: v4(seat_rgb(&node.name), 1.0),
                    params: Vec4::new(t, work, if busy { 1.0 } else { 0.0 }, seed),
                    logo: match logos.0.get(&node.name) {
                        Some((_, mono)) => Vec4::new(1.0, if *mono { 1.0 } else { 0.0 }, bright, 0.0),
                        None => Vec4::ZERO,
                    },
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
    let sc = &mods.cfg.subagents;
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
        if sc.show && view.moons {
            let mut subs: Vec<&SubagentInfo> =
                deck.subagents.iter().filter(|s| s.parent == node.name && s.active).collect();
            subs.sort_by(|a, b| a.id.cmp(&b.id));
            for (i, sub) in subs.iter().take(sc.max).enumerate() {
                let band = i / 8;
                let r = (sc.radius + band as f32 * sc.spacing + (i % 8) as f32 * 2.0) * s;
                let phase = (task_key(&sub.id) % 4096) as f32 / 4096.0 * std::f32::consts::TAU;
                let a = t * sc.speed * (if band % 2 == 0 { 1.0 } else { -0.75 }) + phase;
                let q = p + Vec2::new(a.cos() * r, a.sin() * r * 0.8);
                gizmos.circle_2d(Isometry2d::from_translation(q), sc.size * s, seat_colour(&node.name, sc.bright * dim));
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
    mut kit: ResMut<KittenView>,
    mut gizmos: Gizmos,
) {
    let mut positions: HashMap<String, Vec2> =
        nodes.iter().map(|(n, tf)| (n.name.clone(), tf.translation.truncate())).collect();
    // The kitten is where notes to and from Lumo start and land (t78).
    if kit.anchor.is_some() {
        positions.insert(kitten_view::SEAT.to_string(), kit.pos);
    }
    let now_k = time.elapsed_secs();

    // Launch what arrived. With no ring (before the first status poll, or
    // when every seat is offline) notes wait rather than vanish or burst
    // from the core (Vibe's reviews of #150).
    if !positions.is_empty() {
        let pending = std::mem::take(&mut deck.pending);
        for note in pending {
            let look = note_look(&mods.rules, &mods.cfg, &note);
            deck.warp = stack_warp(deck.warp, look.warp + mods.cfg.warp.per_note, mods.cfg.warp.max);
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
            if note.from == kitten_view::SEAT && kit.anchor.is_some() {
                let id = if note.id.is_empty() { format!("t{now_k}") } else { note.id.clone() };
                kit.sent(&id, &note.to, now_k);
            }
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
                        note_id: note.id.clone(),
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
        // Follow the receiver round the orbit mid-flight. Lumo's electron
        // quantum-jumps, so a comet to her eases after her instead of
        // snapping (a snap kinked its trail into a zigzag, t78).
        if let Some(p) = positions.get(&comet.to) {
            comet.to_pos = if comet.to == kitten_view::SEAT {
                comet.to_pos.lerp(*p, 1.0 - (-dt * 5.0).exp())
            } else {
                *p
            };
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
            if comet.to == kitten_view::SEAT && kit.anchor.is_some() {
                let id = if comet.note_id.is_empty() { format!("t{now}") } else { comet.note_id.clone() };
                kit.landed(&id, now);
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
    // The sky fills the whole window (the panels cover their edges).
    let win = window_size(&windows);
    let half = Vec2::new(win.x * 0.5 + layout.left * 0.5 + layout.right * 0.5, win.y * 0.5 + layout.panel * 0.5);
    let reach = half.length() + 20.0;
    let pace = (deck.notes_last(time.elapsed_secs(), 600.0) as f32).min(30.0);
    let tc = &mods.cfg.typing;
    // Still when nobody works, slow with a few at it, boosted by notes to Y.
    let busy = deck.seats.iter().filter(|s| s.online() && s.shown == "busy").count() as f32;
    let speed = sc.speed + busy * sc.per_busy + deck.boost * sc.boost_speed + deck.warp * sc.warp_speed + deck.thrust * tc.star_speed;
    let dt = time.delta_secs();
    for (mut star, mut tf) in &mut stars {
        star.dist += speed * star.speed * dt * (0.3 + star.dist / reach);
        // The stars are the core's radiation (Y, 05:23): they leave it on
        // the disk's turn, curving the way it spins, straightening as they
        // get away from it.
        let curl = deck.disk_rate * 0.6 / (1.0 + star.dist / 160.0);
        star.dir = Vec2::from_angle(star.dir.to_angle() - curl * dt);
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
        tf.scale = Vec3::new(stretch * (1.0 + near) * star.size, (0.6 + near) * star.size, 1.0);
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
/// The system stats down the left of the scene (t53, Y: "system stats,
/// like gpu, memory, ram, context, on the left side"). The machine is read
/// every 2 s (a few /proc and hwmon files); the frame record every frame.
/// It turns amber when a figure is where trouble starts.
fn sys_panel(
    deck: Res<Deck>,
    left: Res<left_panel::Left>,
    mods: Res<Mods>,
    view: Res<View>,
    real: Res<Time<Real>>,
    layout: Res<Layout>,
    mut panel: Query<(&mut Text2d, &mut Transform, &mut TextColor, &mut Visibility), (With<SysPanel>, Without<MemAlert>)>,
    mut alert: Query<(&mut Text2d, &mut Transform, &mut TextColor, &mut Visibility), (With<MemAlert>, Without<SysPanel>)>,
    mut gizmos: Gizmos,
    mut state: Local<(sys::Frames, sys::Machine, f32)>,
) {
    let now = real.elapsed_secs();
    // Only frames drawn while shown count: hidden, the compositor sends no
    // frame callbacks and every frame waits out a 1 s swap-chain timeout,
    // which isn't the GPU stalling (18:46-18:54 on 27 Nov, all hidden).
    if VISIBLE.load(std::sync::atomic::Ordering::Relaxed) {
        state.0.frame(now, real.delta_secs());
    }
    if state.2 <= 0.0 || now - state.2 >= 2.0 {
        state.1 = sys::read_machine();
        state.2 = now.max(1e-3);
    }
    let show = mods.cfg.hud.show && mods.cfg.hud.stats && view.hud;
    let context: Vec<(String, f64)> =
        deck.seats.iter().filter(|s| s.online()).filter_map(|s| s.ctx.map(|c| (label_for(&s.name), c))).collect();
    let text = sys::panel_text(&state.1, &state.0, &context);
    let warn = sys::warning(&state.1, state.0.stalls());
    let colour = if warn { Color::srgb(1.0, 0.7, 0.3) } else { Color::srgb(0.5, 0.6, 0.75) };
    for (mut t, mut tf, mut c, mut vis) in &mut panel {
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if t.0 != text {
            t.0 = text.clone();
        }
        if c.0 != colour {
            c.0 = colour;
        }
        let (p, here) = left_panel::place(&layout, &left, left_panel::LeftTab::System, Vec2::new(-layout.half().x + 18.0, layout.half().y - 70.0));
        if !here {
            *vis = Visibility::Hidden;
        }
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        // In front of the left panel's background (z 7) when inside it.
        tf.translation.z = if layout.left > 0.0 { 10.0 } else { 5.0 };
    }
    // The low-memory alert shows whatever the HUD toggles say: it is the
    // one thing on screen that must never be hidden. Amber and breathing
    // for a warning; red, flashing, with a red frame round the scene when
    // critical, like a game's low-health screen.
    // A demo on demand (Y, 22:49: "bring up the low memory alert so I can
    // see what it looks like"): `touch ~/.config/warp-core/alert-demo` shows
    // 10 s of the warning, then 10 s of critical, with made-up figures, and
    // removes the file. Checked with the 2 s machine read.
    let (mut level, mut mem) = (sys::memory_alert(&state.1.mem), state.1.mem.clone());
    if let Some(t0) = demo_since(now) {
        let gb = 1024 * 1024;
        let (lvl, avail, swap_free) =
            if now - t0 < 10.0 { (sys::Alert::Warning, gb, 6 * gb) } else { (sys::Alert::Critical, gb / 4, gb / 2) };
        mem = sys::Memory { total_kb: 7 * gb, available_kb: avail, swap_total_kb: 12 * gb, swap_free_kb: swap_free, shmem_kb: 0 };
        level = lvl;
    }
    let words = sys::alert_text(&mem, level);
    let (rgb, pulse) = match level {
        sys::Alert::Critical => ([1.0, 0.15, 0.1], 0.55 + 0.45 * (now * 7.0).sin().abs()),
        _ => ([1.0, 0.7, 0.25], 0.8 + 0.2 * (now * 2.0).sin()),
    };
    for (mut t, mut tf, mut c, mut vis) in &mut alert {
        let want = if level == sys::Alert::None { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
        if t.0 != words {
            t.0 = words.clone();
        }
        c.0 = Color::srgba(rgb[0], rgb[1], rgb[2], pulse);
        tf.translation.x = 0.0;
        tf.translation.y = layout.half().y - 40.0;
    }
    if level == sys::Alert::Critical {
        let h = layout.half();
        for (inset, a) in [(3.0, pulse), (9.0, pulse * 0.5), (16.0, pulse * 0.25)] {
            gizmos.rect_2d(
                Isometry2d::IDENTITY,
                Vec2::new(2.0 * (h.x - inset), 2.0 * (h.y - inset)),
                Color::linear_rgba(rgb[0] * 2.0, rgb[1], rgb[2], a),
            );
        }
    }
}

/// When the alert demo started (see sys_panel), while it runs: the file
/// ~/.config/warp-core/alert-demo starts it, 20 s ends it.
fn demo_since(now: f32) -> Option<f32> {
    use std::sync::Mutex;
    static START: Mutex<Option<f32>> = Mutex::new(None);
    static LAST_LOOK: Mutex<f32> = Mutex::new(-10.0);
    let mut start = START.lock().ok()?;
    if let Some(t0) = *start {
        if now - t0 < 20.0 {
            return Some(t0);
        }
        *start = None;
    }
    let mut last = LAST_LOOK.lock().ok()?;
    if now - *last >= 1.0 {
        *last = now;
        let f = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/warp-core/alert-demo");
        if std::fs::remove_file(&f).is_ok() {
            *start = Some(now);
        }
    }
    *start
}

/// A task's title cut to `max` characters at a word, with an ellipsis.
fn short_title(title: &str, max: usize) -> String {
    let t = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        return t;
    }
    let cut: String = t.chars().take(max).collect();
    let head = match cut.rfind(' ') {
        Some(i) if i > max / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", head.trim_end_matches(|c: char| c == ',' || c == '.' || c == ':' || c == ';'))
}

/// How long, the short way: 40s, 12m, 3h 05m, 2d 4h.
fn short_age(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86_399 => format!("{}h {:02}m", s / 3600, s / 60 % 60),
        _ => format!("{}d {}h", s / 86_400, s / 3600 % 24),
    }
}

/// The task panel's text (Y, 2026-11-27: "who they're with, and how long
/// they've been with them"): one line per task on the board, in id order.
fn task_panel_text(tasks: &[TaskInfo], now_epoch: f64) -> String {
    if tasks.is_empty() {
        return "TASKS
none on the board".to_string();
    }
    let mut s = String::from("TASKS");
    for t in tasks {
        let title = t.title.as_deref().map(|x| short_title(x, 40)).unwrap_or_else(|| "(private)".to_string());
        let who = match &t.holder {
            Some(h) => label_for(h),
            None => "open".to_string(),
        };
        let age = t.since.map(|at| format!(", {}", short_age(now_epoch - at))).unwrap_or_default();
        s.push_str(&format!("\n{}  {}  — {}{}", t.id, title, who, age));
    }
    s
}

/// The current tasks at the bottom of the scene's left side, above the dock.
/// Shown with the HUD and `[hud] tasks_panel`; refreshed once a second.
fn task_panel(
    deck: Res<Deck>,
    left: Res<left_panel::Left>,
    mods: Res<Mods>,
    view: Res<View>,
    real: Res<Time<Real>>,
    layout: Res<Layout>,
    mut panel: Query<(&mut Text2d, &mut Transform, &mut Visibility), With<TaskPanel>>,
    mut last: Local<f32>,
) {
    let now = real.elapsed_secs();
    let show = mods.cfg.hud.show && mods.cfg.hud.tasks_panel && view.hud;
    let refresh = *last <= 0.0 || now - *last >= 1.0;
    if refresh {
        *last = now.max(1e-3);
    }
    let epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    for (mut t, mut tf, mut vis) in &mut panel {
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if refresh || t.0.is_empty() {
            let text = task_panel_text(&deck.tasks, epoch);
            if t.0 != text {
                t.0 = text;
            }
        }
        let (p, here) = left_panel::place(&layout, &left, left_panel::LeftTab::Tasks, Vec2::new(-layout.half().x + 18.0, -layout.half().y + 70.0));
        if !here {
            *vis = Visibility::Hidden;
        }
        tf.translation.x = p.x;
        tf.translation.y = p.y;
        // In front of the left panel's background (z 7) when inside it.
        tf.translation.z = if layout.left > 0.0 { 10.0 } else { 5.0 };
    }
}

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
    mut fps: Local<(u32, f32, f32)>,
) {
    let half = layout.half();
    let now = time.elapsed_secs();
    // Frames drawn in the last whole second, from the real clock (Y wants 60
    // at least while shown; t28).
    fps.0 += 1;
    fps.1 += real.delta_secs();
    if fps.1 >= 1.0 {
        fps.2 = fps.0 as f32 / fps.1;
        *fps = (0, 0.0, fps.2);
    }
    let online: Vec<_> = deck.seats.iter().filter(|s| s.online()).collect();
    let busy = online.iter().filter(|s| s.shown == "busy").count();
    // Every seat's queue, on the ring or not (Vibe's review of #150).
    let waiting: u64 = deck.seats.iter().map(|s| s.waiting).sum();
    let mut line = String::new();
    if mods.cfg.hud.show && view.hud {
        line = format!(
            "WARP CORE {}  {:.0} fps    {} online | {} busy | {} waiting | {} notes in the last hour",
            version(),
            fps.2,
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
        }
        if deck.thrust > 0.05 {
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
/// Each seat's share of the context pool, among the seats that are online:
/// an agent that is away holds no stars (Y, 05:20).
fn context_shares(seats: &[SeatInfo]) -> Vec<(String, f32)> {
    let have: Vec<(&str, f64)> =
        seats.iter().filter(|s| s.online()).filter_map(|s| s.ctx_tokens.filter(|t| *t > 0.0).map(|t| (s.name.as_str(), t))).collect();
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
        // The stars weigh in too (t18): the more of the context pool a
        // seat holds, the bigger its stars burn, updating as the shares do.
        let share = match seat.and_then(|n| shares.iter().find(|(s, _)| s == n)) {
            Some((_, s)) => *s,
            None => 1.0 / shares.len().max(1) as f32,
        };
        star.size = body_size(share, shares.len(), mods.cfg.physics.size_span);
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
                let reach = chat::route(chat.input.trim(), &seats, &available_seats(&deck)).map(|to| recipients(&to, &seats)).unwrap_or(1);
                send_line(&mut chat, &deck, real.elapsed_secs());
                // A note went out: jump to warp in proportion to its length.
                if chat.sending > before && mods.cfg.typing.jump_on_send {
                    // Sending stacks onto any warp already running, and the
                    // thrust built while typing keeps pushing (Y, t88:
                    // "Thrust should continue to apply even when in warp").
                    deck.warp = stack_warp(deck.warp, send_jump(len, reach, &mods.cfg.typing), mods.cfg.warp.max);
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
/// Warp after a kick of `add`: kicks stack (more notes, further warp, Y
/// t88), up to `max`.
fn stack_warp(warp: f32, add: f32, max: f32) -> f32 {
    (warp + add.max(0.0)).min(max.max(0.0)).max(warp.min(max.max(0.0)))
}

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
/// The agents signalling they're free: on, and idle or ready (not busy,
/// stuck or spent). A note without @names goes to them (Y, 06:11).
fn available_seats(deck: &Deck) -> Vec<String> {
    deck.seats.iter().filter(|s| s.online() && matches!(s.shown.as_str(), "idle" | "ready")).map(|s| s.name.clone()).collect()
}

fn send_line(chat: &mut Chat, deck: &Deck, now: f32) {
    let text = chat.input.trim().to_string();
    if text.is_empty() {
        return;
    }
    let seats: Vec<String> = deck.seats.iter().map(|s| s.name.clone()).collect();
    match chat::route(&text, &seats, &available_seats(deck)) {
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
/// How many of the newest notes (before `scroll`) fit in `lines` rows of
/// `cols` characters, each note taking its header plus its wrapped text.
/// The strip is short (t91), and the log grows upward from its input line:
/// drawn whole, 60 notes climbed over the whole scene.
fn notes_that_fit(notes: &VecDeque<ChatNote>, scroll: usize, cols: usize, lines: usize) -> usize {
    let end = notes.len().saturating_sub(scroll);
    let cols = cols.max(20);
    let mut used = 0;
    let mut n = 0;
    for note in notes.range(..end).rev() {
        let head = note.from.len() + note.to.len() + 12;
        let rows = (head + note.text.chars().count()).div_ceil(cols).max(1);
        if used + rows > lines && n > 0 {
            break;
        }
        used += rows;
        n += 1;
        if used >= lines {
            break;
        }
    }
    n.max(1)
}

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
    // Bottom dock: the strip spans the central column. Right dock: a
    // full-height column on the right edge holding every tab.
    let show = match layout.dock {
        mods::ChatDock::Bottom => layout.panel > 0.0,
        mods::ChatDock::Right => layout.right > 0.0,
    };
    let vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    let (dock_w, dock_h, cx, top, bottom) = match layout.dock {
        mods::ChatDock::Bottom => (layout.scene.x, layout.panel, 0.0, -layout.scene.y * 0.5, -layout.scene.y * 0.5 - layout.panel),
        mods::ChatDock::Right => (layout.right, layout.scene.y, layout.scene.x * 0.5 + layout.right * 0.5, layout.scene.y * 0.5, -layout.scene.y * 0.5),
    };
    let cy = (top + bottom) * 0.5;
    let pad = 14.0;
    let inner = (dock_w - pad * 2.0).max(50.0);
    let text_x = match layout.dock {
        mods::ChatDock::Bottom => -layout.scene.x * 0.5 + pad,
        mods::ChatDock::Right => layout.scene.x * 0.5 + pad,
    };
    let header_h = 38.0;
    // A long draft wraps upward: the status line and the notes sit above it.
    let line_h = line
        .iter()
        .next()
        .map(|(.., info)| info.size.y / info.scale_factor.max(0.1))
        .filter(|y| *y > 1.0)
        .unwrap_or(20.0);
    let input_h = line_h + 46.0;

    for (mut tf, mut v, header) in &mut panels {
        *v = vis;
        if header.is_some() {
            tf.translation = Vec3::new(cx, top - header_h * 0.5, 9.0);
            tf.scale = Vec3::new(dock_w, header_h, 1.0);
        } else {
            tf.translation = Vec3::new(cx, cy, 7.0);
            tf.scale = Vec3::new(dock_w, dock_h, 1.0);
        }
    }
    for (mut text, mut tf, mut v) in &mut title {
        *v = vis;
        let mut t = if chat.side_on {
            "[F1 COMMS]     F2 ACTIVITY · F3 NOTIFY on the right".to_string()
        } else {
            Tab::ALL
                .iter()
                .enumerate()
                .map(|(i, tab)| if *tab == chat.tab { format!("[F{} {}]", i + 1, tab.name()) } else { format!(" F{} {} ", i + 1, tab.name()) })
                .collect::<Vec<_>>()
                .join(" ")
        };
        if chat.scroll > 0 {
            t.push_str(&format!("  (back {}, PgDn)", chat.scroll));
        }
        if text.0 != t {
            text.0 = t;
        }
        tf.translation = Vec3::new(text_x, top - 12.0, 10.0);
    }
    for (e, mut tf, mut bounds, mut v, mut font) in &mut log {
        *v = vis;
        tf.translation = Vec3::new(text_x, bottom + input_h + 10.0, 8.0);
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
            // Only what fits between the header and the input line.
            let rows = ((dock_h - header_h - input_h - 14.0) / (cfg.font * 1.25)).max(1.0) as usize;
            let cols = (inner / (cfg.font * 0.6)) as usize;
            let spans = match chat.tab {
                Tab::Comms => chat_spans(&chat.notes, chat.scroll, notes_that_fit(&chat.notes, chat.scroll, cols, rows)),
                Tab::Activity => activity_spans(&chat.activity, chat.scroll, rows, now_epoch),
                Tab::Notify => notice_spans(&chat.notices, chat.scroll, rows, now_epoch),
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
            format!("> {}type a note: @seat to address it, @all for all, none for the free agents", if blink { "_" } else { " " })
        } else {
            format!("> {}{}", chat.input, if chat.focus && blink { "_" } else { " " })
        };
        if text.0 != t {
            text.0 = t;
        }
        if bounds.width != Some(inner) {
            bounds.width = Some(inner);
        }
        tf.translation = Vec3::new(text_x, bottom + 12.0, 10.0);
    }
    for (mut text, mut colour, mut tf, mut v) in &mut status {
        *v = vis;
        let seats: Vec<String> = deck.seats.iter().map(|s| s.name.clone()).collect();
        let fresh = chat.status.as_ref().filter(|(_, _, at)| now - at < 8.0);
        let (t, err) = match fresh {
            Some((msg, err, _)) => (msg.clone(), *err),
            None if !chat.input.trim().is_empty() => match chat::route(&chat.input, &seats, &available_seats(&deck)) {
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
        tf.translation = Vec3::new(text_x, bottom + 12.0 + line_h + 8.0, 10.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bevy refuses, at start, two queries in one system that could touch
    /// the same component mutably (B0001): 1.32 died in a restart loop on
    /// exactly that. Initialising the systems catches it without a GPU.
    #[test]
    fn workspace_and_scratchpad_events_wake_the_visibility_check() {
        assert!(shows_change("activespecial>>special:magic,DP-1\n"));
        assert!(shows_change("workspacev2>>2,nav\n"));
        assert!(!shows_change("activewindow>>kitty,claude\n"));
        assert_eq!(mods::Look::default().fps_hidden, 24.0);
    }

    #[test]
    fn sixty_and_up_is_paced_by_vsync_not_a_timer() {
        assert_eq!(frame_mode(60.0), UpdateMode::Continuous);
        assert!(matches!(frame_mode(24.0), UpdateMode::Reactive { .. }));
    }

    #[test]
    fn bringing_it_forward_speaks_hyprlands_lua() {
        assert_eq!(show_dispatch("special:magic").as_deref(), Some("hl.dsp.workspace.toggle_special(\"magic\")"));
        assert_eq!(show_dispatch("4").as_deref(), Some("hl.dsp.focus({ workspace = 4 })"));
        assert_eq!(show_dispatch("special:a\")b"), None, "nothing odd reaches the Lua");
    }

    #[test]
    fn the_systems_queries_do_not_clash() {
        let mut world = World::new();
        let mut init = |mut s: Box<dyn System<In = (), Out = ()>>| {
            s.initialize(&mut world);
        };
        init(Box::new(IntoSystem::into_system(layout_held_tasks)));
        init(Box::new(IntoSystem::into_system(layout_tasks)));
        init(Box::new(IntoSystem::into_system(layout_seats)));
        init(Box::new(IntoSystem::into_system(drive_planets)));
        init(Box::new(IntoSystem::into_system(draw_moons_and_marks)));
        init(Box::new(IntoSystem::into_system(hud)));
    }

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
            step_body(&mut b, false, 0.0, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
            lo = lo.min(b.pos.length());
            hi = hi.max(b.pos.length());
            went_down |= b.pos.y < -100.0;
            went_left |= b.pos.x < -100.0;
        }
        assert!(lo > 250.0 && hi < 360.0, "the orbit breathes between {lo} and {hi}, it doesn't decay or escape");
        assert!(went_down && went_left, "it really went round, not just wobbled in place");
    }

    #[test]
    fn touching_planets_bounce_apart() {
        // Head on, overlapping: pushed out to the contact distance and the
        // closing speed turned round (t46).
        let mut b = [
            Body { pos: Vec2::new(-20.0, 0.0), vel: Vec2::new(30.0, 0.0) },
            Body { pos: Vec2::new(20.0, 0.0), vel: Vec2::new(-30.0, 0.0) },
        ];
        resolve_contacts(&mut b, 100.0, 0.8);
        assert!(((b[1].pos - b[0].pos).length() - 100.0).abs() < 0.01, "pushed out to touching");
        assert!(b[0].vel.x < 0.0 && b[1].vel.x > 0.0, "they bounce: {:?} {:?}", b[0].vel, b[1].vel);
        assert!((b[1].vel.x - 24.0).abs() < 0.01, "keeping restitution of the speed: {}", b[1].vel.x);
        // Exactly on top of each other: still split, the same way every time.
        let mut c = [Body::default(), Body::default()];
        resolve_contacts(&mut c, 100.0, 0.8);
        assert!(((c[1].pos - c[0].pos).length() - 100.0).abs() < 0.01);
        let mut again = [Body::default(), Body::default()];
        resolve_contacts(&mut again, 100.0, 0.8);
        assert_eq!(c[0].pos, again[0].pos);
        // Separating pairs keep their speed; far pairs aren't touched.
        let mut d = [
            Body { pos: Vec2::new(0.0, 0.0), vel: Vec2::new(-5.0, 0.0) },
            Body { pos: Vec2::new(50.0, 0.0), vel: Vec2::new(5.0, 0.0) },
        ];
        resolve_contacts(&mut d, 100.0, 0.8);
        assert_eq!((d[0].vel.x, d[1].vel.x), (-5.0, 5.0));
        let far = [Body { pos: Vec2::new(0.0, 0.0), vel: Vec2::ZERO }, Body { pos: Vec2::new(300.0, 0.0), vel: Vec2::ZERO }];
        let mut f = far;
        resolve_contacts(&mut f, 100.0, 0.8);
        assert_eq!(f[1].pos, far[1].pos);
    }

    #[test]
    fn planets_dont_stick_together() {
        // Y, t46: two planets meeting on nearly the same orbit used to
        // clump into one and ride round together for good. Run the sim the
        // way the deck does (mutual pull, step, contacts) for two minutes.
        let cfg = mods::PhysicsCfg::default();
        let scene = Vec2::new(1280.0, 800.0);
        for busy in [false, true] {
            let mut b = vec![
                Body { pos: Vec2::new(0.0, 250.0), vel: Vec2::new(46.0, 0.0) },
                Body { pos: Vec2::new(30.0, 255.0), vel: Vec2::new(40.0, 0.0) },
                Body { pos: Vec2::new(-260.0, 0.0), vel: Vec2::new(0.0, 45.0) },
            ];
            let masses = [1.0, 1.0, 1.0];
            let mut min_gap = f32::MAX;
            for _ in 0..(120 * 60) {
                let pull = mutual_pulls(&b, &masses, &cfg);
                for (i, body) in b.iter_mut().enumerate() {
                    step_body(body, busy, 0.1, scene, 1.0 / 60.0, &cfg, pull[i]);
                }
                resolve_contacts(&mut b, cfg.contact, cfg.restitution);
                for i in 0..b.len() {
                    for j in (i + 1)..b.len() {
                        min_gap = min_gap.min((b[j].pos - b[i].pos).length());
                    }
                }
            }
            // A third planet can squeeze a pair a little within one frame.
            assert!(min_gap > cfg.contact * 0.9, "busy={busy}: planets overlapped, closest {min_gap}");
            assert!(b.iter().all(|x| x.pos.is_finite() && x.vel.is_finite()));
        }
    }

    #[test]
    fn busy_planets_ride_higher_and_idle_ones_hold() {
        let cfg = mods::PhysicsCfg::default();
        let scene = Vec2::new(1280.0, 800.0);
        // A planet circling at the old ring's radius, steered onto a slow
        // turn (t18: the target is the circular speed plus a share of the
        // ring's turning, so idle planets hold fairly stable orbits).
        let start = Body { pos: Vec2::new(0.0, 180.0), vel: Vec2::new(24.0, 0.0) };
        let circling = |busy| {
            let mut b = start;
            let mut sum = 0.0;
            let mut lo = f32::MAX;
            for _ in 0..600 {
                step_body(&mut b, busy, 0.1, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
                let r = b.pos.length();
                sum += r;
                lo = lo.min(r);
            }
            (sum / 600.0, lo)
        };
        let (idle_mean, idle_lo) = circling(false);
        assert!(idle_lo > 150.0, "an idle planet keeps a fairly stable orbit, never sinking in: lo {idle_lo}");
        let (busy_mean, _) = circling(true);
        assert!(busy_mean > idle_mean + 20.0, "a working planet rides higher on average: idle {idle_mean:.0} busy {busy_mean:.0}");
        // Even one that sank all the way in climbs back out to the ring.
        let mut sunken = Body { pos: Vec2::new(0.0, cfg.min_radius), vel: Vec2::new(9.0, 0.0) };
        for _ in 0..(60 * 60) {
            step_body(&mut sunken, true, 0.1, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
        }
        let ring = ring_radius(scene, &cfg);
        assert!((sunken.pos.length() - ring).abs() < 15.0, "it climbs to the ring: {} vs {ring}", sunken.pos.length());
    }

    #[test]
    fn the_task_panel_says_who_has_what_and_for_how_long() {
        assert_eq!(short_age(42.0), "42s");
        assert_eq!(short_age(47.0 * 60.0), "47m");
        assert_eq!(short_age(2.0 * 3600.0 + 5.0 * 60.0), "2h 05m");
        assert_eq!(short_age(3.0 * 86_400.0 + 4.0 * 3600.0), "3d 4h");
        assert_eq!(short_title("Make the warp core public", 40), "Make the warp core public");
        assert_eq!(short_title("We need system stats, like gpu, memory, ram, context, on the left side", 40), "We need system stats, like gpu, memory…");
        let tasks = vec![
            feed::TaskInfo { id: "t50".into(), title: Some("electrons".into()), since: Some(1000.0), from: "owner".into(), holder: Some("vibe".into()), open: false },
            feed::TaskInfo { id: "t54".into(), title: None, since: Some(4000.0), from: "owner".into(), holder: None, open: true },
        ];
        let text = task_panel_text(&tasks, 4000.0 + 300.0);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "TASKS");
        assert!(lines[1].starts_with("t50  electrons  — ") && lines[1].ends_with(", 55m"), "{}", lines[1]);
        assert_eq!(lines[2], "t54  (private)  — open, 5m");
        assert_eq!(task_panel_text(&[], 0.0), "TASKS\nnone on the board");
    }

    #[test]
    fn a_busy_planet_holds_a_stable_ring_clear_of_the_edges() {
        // t52 (Y: "get yourself on a stable ring orbit to the core"). Before,
        // thrust beat the pull all the way out and a busy planet rode the
        // screen's edge, bouncing. Now it settles on a circle and stays.
        let cfg = mods::PhysicsCfg::default();
        for scene in [Vec2::new(1280.0, 800.0), Vec2::new(2900.0, 2520.0)] {
            let ring = ring_radius(scene, &cfg);
            let wall = scene * 0.5 - cfg.edge_margin;
            for start in [
                Body { pos: Vec2::new(0.0, cfg.min_radius), vel: Vec2::new(10.0, 0.0) },
                Body { pos: Vec2::new(wall.x - 5.0, 0.0), vel: Vec2::new(60.0, -80.0) },
                Body { pos: Vec2::new(150.0, -150.0), vel: Vec2::new(-120.0, 40.0) },
            ] {
                let mut b = start;
                for _ in 0..(90 * 60) {
                    step_body(&mut b, true, 0.1, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
                }
                // The next half minute: on the ring, round, off the walls.
                let (mut lo, mut hi) = (f32::MAX, 0.0f32);
                let a0 = b.pos.y.atan2(b.pos.x);
                let mut turned = 0.0;
                let mut last = a0;
                for _ in 0..(30 * 60) {
                    step_body(&mut b, true, 0.1, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
                    let r = b.pos.length();
                    lo = lo.min(r);
                    hi = hi.max(r);
                    assert!(b.pos.x.abs() < wall.x - 1.0 && b.pos.y.abs() < wall.y - 1.0, "touched a wall at {:?}", b.pos);
                    let a = b.pos.y.atan2(b.pos.x);
                    let mut d = a - last;
                    if d > std::f32::consts::PI { d -= std::f32::consts::TAU; }
                    if d < -std::f32::consts::PI { d += std::f32::consts::TAU; }
                    turned += d;
                    last = a;
                }
                assert!((lo - ring).abs() < 10.0 && (hi - ring).abs() < 10.0,
                        "{scene:?} from {:?}: holds the ring {ring:.0}, swung {lo:.0}..{hi:.0}", start.pos);
                assert!(turned.abs() > 0.5, "and keeps circling it: turned {turned:.2} rad");
            }
        }
    }

    #[test]
    fn a_planet_never_falls_in_the_core_or_off_the_screen() {
        let cfg = mods::PhysicsCfg::default();
        let scene = Vec2::new(1280.0, 800.0);
        // Straight at the singularity: it stops at the floor and keeps
        // whatever sideways motion it had.
        let mut falling = Body { pos: Vec2::new(200.0, 0.0), vel: Vec2::new(-80.0, 30.0) };
        for _ in 0..1200 {
            step_body(&mut falling, false, 0.1, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
        }
        assert!(falling.pos.length() >= cfg.min_radius - 0.5, "stops at the floor: {}", falling.pos.length());
        // Flung at the screen's edge: reflected back into the scene.
        // (Checked at the bounce itself: ten seconds on, it has settled into
        // an orbit and its y speed may point either way.)
        let mut flung = Body { pos: Vec2::new(0.0, 100.0), vel: Vec2::new(0.0, 400.0) };
        let wall = scene.y * 0.5 - cfg.edge_margin;
        let mut bounced = None;
        for _ in 0..600 {
            step_body(&mut flung, false, 0.1, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
            assert!(flung.pos.y <= wall + 0.5, "never over the edge: {}", flung.pos.y);
            if bounced.is_none() && flung.pos.y >= wall - 1e-3 {
                bounced = Some(flung.vel.y);
            }
        }
        let v = bounced.expect("flung at 400 px/s it reaches the wall");
        assert!(v < 0.0, "it bounces back in, not stuck at the wall: {v}");
        // Whatever happens, a planet stays inside the scene's margins.
        let mut wild = Body { pos: Vec2::new(-500.0, 300.0), vel: Vec2::new(600.0, -600.0) };
        for _ in 0..3600 {
            step_body(&mut wild, true, 1.5, scene, 1.0 / 60.0, &cfg, Vec2::ZERO);
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

    fn run_core(c: &mods::CoreCfg, mut st: CoreState, i: CoreInputs, secs: f32) -> (CoreState, Vec<f32>) {
        let mut sizes = Vec::new();
        let mut first = true;
        for _ in 0..(secs * 60.0) as usize {
            let inputs = CoreInputs { arrived: if first { i.arrived } else { 0 }, ..i };
            first = false;
            let shape = core_step(c, st, inputs, 1.0 / 60.0);
            st = shape.state;
            sizes.push(shape.size);
        }
        (st, sizes)
    }

    #[test]
    fn the_core_knows_when_its_window_is_hidden() {
        let clients = serde_json::json!([{"class": "kitty", "workspace": {"name": "2"}},
                                          {"class": "warp-core", "workspace": {"name": "special:magic"}}]);
        let open = serde_json::json!([{"activeWorkspace": {"name": "2"}, "specialWorkspace": {"name": "special:magic"}}]);
        let closed = serde_json::json!([{"activeWorkspace": {"name": "2"}, "specialWorkspace": {"name": ""}}]);
        assert!(shown(&clients, &open));
        assert!(!shown(&clients, &closed));
        assert!(shown(&serde_json::json!([]), &closed), "no window found: keep drawing");
    }

    #[test]
    fn packets_feed_the_core_and_it_rings_down() {
        let c = mods::CoreCfg::default();
        let calm = CoreInputs { online: 4, ..Default::default() };
        let (_, quiet) = run_core(&c, CoreState::default(), calm, 2.0);
        let (st, fed) = run_core(&c, CoreState::default(), CoreInputs { arrived: 3, ..calm }, 2.0);
        let peak = |v: &[f32]| v.iter().cloned().fold(0.0, f32::max);
        assert!(peak(&fed) > peak(&quiet), "a gulp of packets grows it: {} vs {}", peak(&fed), peak(&quiet));
        // ...without swinging: it eases up, and as the energy fades eases
        // back down; its size turns round at most once, never in and out.
        let (_, long) = run_core(&c, CoreState::default(), CoreInputs { arrived: 3, ..calm }, 30.0);
        let steps: Vec<f32> = long.windows(2).map(|w| w[1] - w[0]).filter(|d| d.abs() > 1e-6).collect();
        let turns = steps.windows(2).filter(|d| d[0].signum() != d[1].signum()).count();
        assert!(turns <= 1, "no in-and-out: it turned {turns} times");
        let quiet_moves = quiet.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(quiet_moves < 1e-4, "a calm core holds still: {quiet_moves}");
        assert!(st.energy > 0.5, "and leaves it charged: {}", st.energy);
        // Left alone it rings down and the energy fades.
        let (st, _) = run_core(&c, st, calm, 60.0);
        assert!(st.swell.abs() < 0.01 && st.swell_v.abs() < 0.05, "rang down: {st:?}");
        assert!(st.energy < 0.05, "faded: {}", st.energy);
    }

    #[test]
    fn energy_quickens_the_breath_and_a_busy_crew_makes_it_bigger() {
        let c = mods::CoreCfg::default();
        let base = CoreInputs { online: 4, ..Default::default() };
        let still = core_step(&c, CoreState::default(), base, 1.0);
        let charged = core_step(&c, CoreState { energy: 2.0, ..Default::default() }, base, 1.0);
        assert!(charged.state.phase > still.state.phase * 2.0, "energy quickens the breathing");
        let idle = core_step(&c, CoreState::default(), base, 0.0).size;
        let busy = core_step(&c, CoreState::default(), CoreInputs { busy: 4, ..base }, 0.0).size;
        assert!(busy > idle, "a working crew grows the core: {busy} vs {idle}");
        let kicked = core_step(&c, CoreState::default(), CoreInputs { arrived: 2, ..base }, 0.05);
        assert!(kicked.flare > 0.0, "a packet flares the photon ring");
        let flood = core_step(&c, CoreState::default(), CoreInputs { arrived: 1000, ..base }, 0.016);
        assert!(flood.state.energy <= c.max_energy && flood.state.swell <= 1.0, "a flood can't blow it up");
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
        let seat = |n: &str, t: Option<f64>| SeatInfo { name: n.into(), state: "idle".into(), ctx_tokens: t, ..Default::default() };
        let away = SeatInfo { name: "codex".into(), state: "away".into(), ctx_tokens: Some(500000.0), ..Default::default() };
        let seats = [seat("claude", Some(60000.0)), seat("vibe", Some(20000.0)), seat("devin", Some(20000.0)), seat("grok", None), away];
        let shares = context_shares(&seats);
        assert_eq!(shares.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["claude", "vibe", "devin"]);
        assert!((shares[0].1 - 0.6).abs() < 1e-6, "an away seat's context counts for nothing");
        assert_eq!(seat_for_slot(&shares, 0.1), Some("claude"));
        assert_eq!(seat_for_slot(&shares, 0.7), Some("vibe"));
        assert_eq!(seat_for_slot(&shares, 0.95), Some("devin"));
        assert_eq!(seat_for_slot(&shares, 1.0), Some("devin"), "the top edge belongs to the last seat");
        assert!(context_shares(&[seat("grok", None)]).is_empty(), "nobody reporting: plain stars");
        assert_eq!(seat_for_slot(&[], 0.5), None);
    }

    #[test]
    fn the_context_pool_sets_mass_and_size() {
        let shares = vec![("claude".to_string(), 0.6), ("vibe".to_string(), 0.2), ("devin".to_string(), 0.2)];
        assert!((body_mass(&shares, "claude") - 1.8).abs() < 1e-6, "share of the pool, scaled to an even split");
        assert!((body_mass(&shares, "grok") - 1.0).abs() < 1e-6, "no figure: the average");
        assert_eq!(body_mass(&[], "claude"), 1.0);
        assert!(body_size(0.6, 3, 0.5) > body_size(0.2, 3, 0.5), "the bigger share draws bigger");
        assert!((body_size(1.0 / 3.0, 3, 0.5) - 1.0).abs() < 1e-6, "an even split draws at 1.0");
        assert_eq!(body_size(0.9, 0, 0.5), 1.0, "no figures at all: plain size");
    }

    #[test]
    fn the_strip_keeps_a_readable_height() {
        let cfg = mods::ChatCfg::default();
        assert!((strip_height(1000.0, &cfg) - 200.0).abs() < 0.01);
        assert!((strip_height(600.0, &cfg) - 120.0).abs() < 0.01);
        assert!((strip_height(300.0, &cfg) - 100.0).abs() < 0.01, "never under 100 px");
        assert!((strip_height(1000.0, &mods::ChatCfg { height: 0.9, ..cfg }) - 450.0).abs() < 0.01, "never over 45% of the window");
        assert_eq!(strip_height(1000.0, &mods::ChatCfg { show: false, ..cfg }), 0.0);
    }

    #[test]
    fn the_chat_area_sits_on_its_docked_edge() {
        let bottom = Layout { scene: Vec2::new(1000.0, 800.0), panel: 200.0, left: 0.0, right: 300.0, dock: mods::ChatDock::Bottom };
        assert!(bottom.in_panel(Vec2::new(0.0, -410.0)));
        assert!(bottom.in_panel(Vec2::new(490.0, -450.0)));
        assert!(!bottom.in_panel(Vec2::new(0.0, -390.0)), "above the strip is scene");
        assert!(!bottom.in_panel(Vec2::new(0.0, -610.0)), "below the strip is off-window");
        assert!(!bottom.in_panel(Vec2::new(510.0, -450.0)), "past the column's edge");
        assert!(!bottom.in_panel(Vec2::new(600.0, 0.0)), "the side panel isn't the strip");
        let right = Layout { scene: Vec2::new(800.0, 1000.0), panel: 0.0, left: 0.0, right: 300.0, dock: mods::ChatDock::Right };
        assert!(right.in_panel(Vec2::new(410.0, 0.0)));
        assert!(right.in_panel(Vec2::new(410.0, -490.0)), "the column runs the full height");
        assert!(!right.in_panel(Vec2::new(390.0, 0.0)), "left of the column is scene");
        assert!(!Layout { panel: 0.0, ..bottom }.in_panel(Vec2::new(0.0, -450.0)), "chat off: nothing is the panel");
        assert!(!Layout { right: 0.0, ..right }.in_panel(Vec2::new(410.0, 0.0)), "column off: nothing is the panel");
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
            ..Default::default()
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
    fn notes_stack_warp_up_to_the_max() {
        // t88: more notes, further warp; no longer just the biggest kick.
        let mut w = 0.0;
        for _ in 0..4 {
            w = stack_warp(w, 0.15, 3.0);
        }
        assert!((w - 0.6).abs() < 1e-5);
        assert_eq!(stack_warp(2.9, 1.0, 3.0), 3.0);
        // A lowered max never yanks a running warp up, and never adds past it.
        assert_eq!(stack_warp(0.5, 0.0, 3.0), 0.5);
        assert_eq!(stack_warp(1.0, -2.0, 3.0), 1.0);
    }

    #[test]
    fn only_the_notes_that_fit_the_strip_are_drawn() {
        let note = |text: &str| ChatNote { from: "claude".into(), to: "owner".into(), time: String::new(), text: text.into(), quiet: false };
        let notes: VecDeque<ChatNote> = (0..60).map(|_| note(&"x".repeat(170))).collect();
        // 200 columns: each note is 1 row of header+text; 6 rows fit 6 notes.
        assert_eq!(notes_that_fit(&notes, 0, 200, 6), 6);
        // 100 columns: each note wraps to 2 rows, so 3 fit.
        assert_eq!(notes_that_fit(&notes, 0, 100, 6), 3);
        // A single note too long for the strip is still shown.
        assert_eq!(notes_that_fit(&notes, 0, 20, 2), 1);
        assert_eq!(notes_that_fit(&VecDeque::new(), 0, 100, 6), 1);
    }

    #[test]
    fn ages_read_short() {
        assert_eq!(ago(42.0), "42s");
        assert_eq!(ago(4800.0), "1h 20m");
        assert_eq!(ago(241499.0), "2d 19h");
    }
}
