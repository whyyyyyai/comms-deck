//! Live mods: everything you would want to tune or extend without a rebuild.
//!
//! The mods folder ($WARP_CORE_MODS, else warp-core/mods next to the source)
//! holds `warp-core.toml` (the settings and the effect rules) and
//! `shaders/*.wgsl` (how the core and the rings are drawn). The running warp
//! core looks at them twice a second and takes in whatever changed. A file
//! that doesn't parse, a rule that doesn't compile, or a shader that doesn't
//! validate is refused: the last good version stays on screen and the error
//! goes in the HUD. A mod can never take the window down.
//!
//! Rules are `when` expressions in a small language of their own, so there
//! is nothing to install and nothing that can run outside the warp core:
//!   numbers, 'strings', true/false, fields (see SEAT_FIELDS / NOTE_FIELDS),
//!   + - * /, == != < <= > >=, && || ! (or and/or/not), ( ), and
//!   `list contains 'x'` for a comma list like a note's `to`.
//!
//! Written by: claude

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub const DEFAULT_CONFIG: &str = include_str!("../mods/warp-core.toml");
pub const DEFAULT_CORE_SHADER: &str = include_str!("../mods/shaders/core.wgsl");
pub const DEFAULT_RING_SHADER: &str = include_str!("../mods/shaders/ring.wgsl");
pub const DEFAULT_PLANET_SHADER: &str = include_str!("../mods/shaders/planet.wgsl");

pub fn mods_dir() -> PathBuf {
    match std::env::var("WARP_CORE_MODS") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/mods")),
    }
}

// ---------------------------------------------------------------- settings

/// A colour in a mod: a name ("amber", "seat" for the seat's own colour)
/// or linear [r, g, b], where values above 1 bloom.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Colour {
    Name(String),
    Rgb([f32; 3]),
}

impl Colour {
    /// Linear RGB; `seat` stands in for the seat's colour.
    pub fn rgb(&self, seat: [f32; 3]) -> Result<[f32; 3], String> {
        match self {
            Colour::Rgb(c) => Ok(*c),
            Colour::Name(n) => named(n, seat).ok_or_else(|| format!("unknown colour '{n}'")),
        }
    }
}

fn named(name: &str, seat: [f32; 3]) -> Option<[f32; 3]> {
    Some(match name.to_lowercase().as_str() {
        "seat" => seat,
        "white" => [1.0, 1.0, 1.0],
        "red" => [1.0, 0.12, 0.1],
        "amber" => [1.0, 0.55, 0.05],
        "gold" => [1.0, 0.78, 0.2],
        "green" => [0.2, 1.0, 0.4],
        "cyan" => [0.2, 0.9, 1.0],
        "blue" => [0.2, 0.45, 1.0],
        "violet" => [0.65, 0.35, 1.0],
        "pink" => [1.0, 0.4, 0.75],
        _ => return None,
    })
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub background: [f32; 3],
    pub bloom: f32,
    pub fps: f32,
    /// Frames a second while another window has the focus: less CPU.
    pub fps_unfocused: f32,
    /// Frames a second while the window is hidden (in the scratchpad or on
    /// a workspace no monitor shows): 24, so tabbing back is snappy (Y, 06:18).
    pub fps_hidden: f32,
    /// Multisampling: 1 (off), 2 or 4. Each step multiplies the GPU's
    /// memory traffic on the 4480x2520 display; the shapes are drawn smooth
    /// by their shaders, so only thin lines gain from it.
    pub msaa: u32,
}
impl Default for Look {
    fn default() -> Self {
        Look { background: [0.01, 0.012, 0.03], bloom: 0.28, fps: 60.0, fps_unfocused: 60.0, fps_hidden: 24.0, msaa: 1 }
    }
}

/// Stars that reach the edge of the scene splash against it: their light
/// is absorbed into a glow along the border that spreads a little and fades.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SplashCfg {
    pub show: bool,
    /// How much light one star leaves on the wall.
    pub energy: f32,
    /// How fast the glow fades (per second).
    pub fade: f32,
    /// How fast it spreads along the wall (per second).
    pub spread: f32,
    /// Neighbouring cells a splash spills into at once.
    pub spill: usize,
    /// Cells per edge (more is finer, and costs a little more).
    pub cells: usize,
    /// Glow brightness on screen; above 1 blooms.
    pub glow: f32,
}
impl Default for SplashCfg {
    fn default() -> Self {
        SplashCfg { show: true, energy: 0.12, fade: 0.7, spread: 1.5, spill: 2, cells: 64, glow: 1.0 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CoreCfg {
    pub cool: [f32; 3],
    pub hot: [f32; 3],
    pub size: f32,
    pub breath: f32,
    /// t29: what the core takes in governs it. Energy each packet adds.
    pub per_packet: f32,
    /// How fast that energy fades (per second), and the most it holds.
    pub energy_decay: f32,
    pub max_energy: f32,
    /// Resting size added per unit of energy, and with the whole crew busy.
    pub grow_per_energy: f32,
    pub grow_busy: f32,
    /// Breathing: base rate (radians a second) and what quickens it.
    pub breath_rate: f32,
    pub rate_per_energy: f32,
    pub rate_per_note: f32,
    pub rate_per_heat: f32,
    /// How much deeper each breath is per unit of energy.
    pub depth_per_energy: f32,
    /// The gulp: outward kick per packet, the ring-down's frequency (Hz)
    /// and damping ratio (1 = no overshoot).
    pub kick: f32,
    pub ring_freq: f32,
    pub ring_damping: f32,
    /// Seconds the core takes to ease to a new resting size; it no longer
    /// swells in and out (Y, 06:19).
    pub settle: f32,
}
impl Default for CoreCfg {
    fn default() -> Self {
        CoreCfg {
            cool: [0.25, 0.9, 3.2],
            hot: [5.0, 2.2, 0.8],
            size: 1.0,
            breath: 0.07,
            per_packet: 0.35,
            energy_decay: 0.08,
            max_energy: 3.0,
            grow_per_energy: 0.08,
            grow_busy: 0.12,
            breath_rate: 0.8,
            rate_per_energy: 0.9,
            rate_per_note: 0.25,
            rate_per_heat: 0.6,
            depth_per_energy: 0.5,
            kick: 1.6,
            ring_freq: 1.4,
            ring_damping: 0.25,
            settle: 3.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OrbitCfg {
    pub drift: f32,
    pub traffic: f32,
    pub warp_swing: f32,
    /// Spin a note between two agents adds, in the direction it travels
    /// round the ring (radians a second).
    pub spin_kick: f32,
    /// How fast the spin dies away without notes (per second).
    pub friction: f32,
    pub max_spin: f32,
}
impl Default for OrbitCfg {
    fn default() -> Self {
        OrbitCfg { drift: 0.0, traffic: 0.0, warp_swing: 1.2, spin_kick: 0.08, friction: 0.12, max_spin: 1.5 }
    }
}

/// The planets' motion (t13): each seat is a body in a simplified physics
/// sim — gravity pulls it towards the singularity, an agent at work climbs
/// to a stable ring orbit (t52), an idle one keeps a fairly
/// stable orbit (t18), and no planet is ever lost over the screen's edge.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhysicsCfg {
    /// The core's pull: the inward acceleration at radius r is
    /// `gravity / r²` (px and seconds), so inner orbits run faster.
    pub gravity: f32,
    /// Where a working agent's planet orbits (t52): a ring at this fraction
    /// of the room between the core and the nearest wall (0..1), so it
    /// circles clear of the screen's edges.
    pub ring: f32,
    /// How hard a busy planet is pulled onto its ring, per second² on the
    /// gap in px.
    pub ring_pull: f32,
    /// How fast any planet's in-and-out speed dies away (per second), so a
    /// kicked planet settles back onto a circle.
    pub radial_damping: f32,
    /// Inward drift while a seat isn't working (px/s²). 0 keeps every
    /// planet on a fairly stable orbit (Y, t18); raise it to have idle
    /// planets sink slowly towards the core again.
    pub drift: f32,
    /// The planets' pull on each other (t18): the acceleration between two
    /// planets is `mutual * mass / dist²`, mass from the context pool.
    pub mutual: f32,
    /// Pairs closer than this (px) pull with the strength at this distance,
    /// so a close pass can't sling a planet away.
    pub soften: f32,
    /// How strongly a body's share of the context pool drives its drawn
    /// size, for planets and stars alike (t18). An even split draws at 1.0;
    /// the span is how far a body may shrink or grow from that, as a
    /// fraction (0.5 = half to one and a half times).
    pub size_span: f32,
    /// How much of the ring's turning rides on top of each planet's own
    /// circular speed (t18): the planets mostly hold Keplerian orbits,
    /// and the notes' spin kicks still stir them round the ring.
    pub turn_share: f32,
    /// How hard the planets are steered onto the ring's turning, as a
    /// rate (per second) on the gap between their tangential speed and the
    /// ring's; this is what the notes' spin kicks and warp swing ride on.
    pub steering: f32,
    /// The hardest the steering may push or pull (px/s²), so one note
    /// between two seats can't fling a planet across the screen.
    pub max_steer: f32,
    /// Inside this radius from the core nothing falls further in (px):
    /// the planets skim the singularity and keep circling it.
    pub min_radius: f32,
    /// Kept speed (as a fraction) when a planet is reflected off the
    /// screen's edge.
    pub bounce: f32,
    /// The gap a planet keeps from the screen's edge (px).
    pub edge_margin: f32,
    /// Two planets whose centres are closer than this (px) touch (t46):
    /// they are pushed apart and bounce, and don't pull on each other, so
    /// a pair can no longer clump into one and ride round stuck together.
    pub contact: f32,
    /// Kept speed (as a fraction) of the closing speed when two planets
    /// bounce off each other.
    pub restitution: f32,
}
impl Default for PhysicsCfg {
    fn default() -> Self {
        PhysicsCfg {
            gravity: 530_000.0,
            ring: 0.8,
            ring_pull: 0.2,
            radial_damping: 0.9,
            drift: 0.0,
            mutual: 60_000.0,
            soften: 60.0,
            size_span: 0.5,
            turn_share: 0.3,
            steering: 2.0,
            max_steer: 120.0,
            min_radius: 90.0,
            bounce: 0.5,
            edge_margin: 90.0,
            contact: 100.0,
            restitution: 0.8,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StarsCfg {
    pub speed: f32,
    pub warp_speed: f32,
    pub stretch: f32,
    pub colour: [f32; 3],
    /// Speed added for each agent at work: still when nobody is.
    pub per_busy: f32,
    /// A note from an agent to Y boosts the stars by this (0..), decaying.
    pub boost_per_note: f32,
    pub boost_speed: f32,
    pub boost_decay: f32,
    /// Colour the stars by how the crew's context is split: each seat gets
    /// its share of the stars in its own colour.
    pub by_context: bool,
    pub context_bright: f32,
}
impl Default for StarsCfg {
    fn default() -> Self {
        StarsCfg {
            speed: 0.0,
            per_busy: 7.0,
            boost_per_note: 0.4,
            boost_speed: 160.0,
            boost_decay: 0.3,
            warp_speed: 900.0,
            stretch: 40.0,
            colour: [0.9, 0.95, 1.4],
            by_context: true,
            context_bright: 1.6,
        }
    }
}

/// Typing in the chat box builds thrust: each key adds `kick`, it drains at
/// `decay` a second, so a long note gathers speed. Sending jumps to warp in
/// proportion to the note's length (`jump_per_char`, up to `jump_max`), and
/// warp drains at a steady rate, so a longer note also coasts for longer.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TypingCfg {
    pub kick: f32,
    pub decay: f32,
    pub star_speed: f32,
    pub stretch: f32,
    pub jump_on_send: bool,
    pub jump_per_char: f32,
    pub jump_max: f32,
    /// Each agent past the first a note reaches adds this much of the jump
    /// again (1 = proportional to the recipients).
    pub per_recipient: f32,
}
impl Default for TypingCfg {
    fn default() -> Self {
        TypingCfg { kick: 0.02, decay: 0.05, star_speed: 420.0, stretch: 14.0, jump_on_send: true, jump_per_char: 0.004, jump_max: 3.0, per_recipient: 1.0 }
    }
}

/// The conduits from the planets to the core: bent by the ring's spin, and
/// carrying a packet of light only when a job is finished.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConduitsCfg {
    /// How far the spin bends them (radians of bend per radian a second).
    pub bend: f32,
    pub max_bend: f32,
    pub packet_seconds: f32,
}
impl Default for ConduitsCfg {
    fn default() -> Self {
        ConduitsCfg { bend: 2.2, max_bend: 1.4, packet_seconds: 1.4 }
    }
}

/// The planets' insides (mods/shaders/planet.wgsl): calm when idle, churning
/// while working, more the longer they've been at it.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlanetsCfg {
    pub show: bool,
    /// Minutes busy to reach most of full churn.
    pub full_minutes: f32,
}
impl Default for PlanetsCfg {
    fn default() -> Self {
        PlanetsCfg { show: true, full_minutes: 20.0 }
    }
}

/// Lumo's quantum kitten (t78): her own movement law, poses, hops, yarn and
/// the event queue are all tuned here. The drawing itself lives in kitten.rs
/// and main.rs; this is only the numbers.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KittenCfg {
    pub enable: bool,
    /// The kitten's body size in px (gizmo line recipes scale from it).
    pub size: f32,
    /// How far she may hop from her anchor, as a share of the seat spacing.
    pub reach: f32,
    /// Seconds she dwells between hops when nothing waits.
    pub base_dwell_s: f32,
    /// Dwell multiplier floor: the most restlessness age can buy (×0.6 at 1 h+).
    pub dwell_floor: f32,
    /// Dwell never drops below this many seconds, however restless.
    pub dwell_min_s: f32,
    /// How long the ghost kittens show before a hop collapses.
    pub ghost_s: f32,
    /// Ghost opacity.
    pub ghost_alpha: f32,
    /// Ghosts only draw while the measured fps is at least this.
    pub ghost_min_fps: f32,
    /// Yarn balls drawn beside the anchor before "+N" takes over.
    pub yarn_max: u8,
    /// Seconds each queue event animates for.
    pub catch_s: f32,
    pub perch_s: f32,
    pub rattle_s: f32,
    pub relay_s: f32,
    /// Title words that mark a held task as physics work (the busy pose bats
    /// at an orbit preview). Case-insensitive substring match.
    pub physics_words: Vec<String>,
}
impl Default for KittenCfg {
    fn default() -> Self {
        KittenCfg {
            enable: true,
            size: 30.0,
            reach: 0.22,
            base_dwell_s: 4.0,
            dwell_floor: 0.6,
            dwell_min_s: 1.5,
            ghost_s: 0.5,
            ghost_alpha: 0.25,
            ghost_min_fps: 30.0,
            yarn_max: 3,
            catch_s: 1.5,
            perch_s: 0.8,
            rattle_s: 0.4,
            relay_s: 1.2,
            physics_words: ["physic", "orbit", "gravity", "warp", "solver"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

/// The left panel (t87, Y: "Let's start building the left panel in the
/// warp core. The right panel is the comms, activity, and notify."): the
/// machine, the task board and the crew, on F4-F6.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeftCfg {
    pub show: bool,
    /// Share of the window (kept 260..520 px).
    pub width: f32,
    pub font: f32,
}
impl Default for LeftCfg {
    fn default() -> Self {
        LeftCfg { show: true, width: 0.22, font: 14.0 }
    }
}

/// Ghost kittens cost draw calls the M1 can ill afford during stalls: they
/// only appear while the measured frame rate clears the configured floor.
pub fn ghosts_allowed(fps: f32, cfg: &KittenCfg) -> bool {
    cfg.enable && fps >= cfg.ghost_min_fps
}

/// Where the chat lives: `bottom` keeps F1 Comms in the strip and, with
/// `side`, F2/F3 in the right panel; `right` is the old full-height column
/// on the right holding all three tabs (Devin's t97 fold, claude's ask).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatDock {
    #[default]
    Bottom,
    Right,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChatCfg {
    pub show: bool,
    /// The right panel's width share (F2 Activity, F3 Notify; Y, 02:50),
    /// or the whole chat column's when dock = "right".
    pub width: f32,
    /// Show the right panel. Off: F2/F3 show in the bottom strip instead.
    /// Ignored while dock = "right" (the column holds every tab).
    pub side: bool,
    /// The bottom strip's share of the window's height.
    pub height: f32,
    /// Where the chat docks.
    pub dock: ChatDock,
    pub font: f32,
    /// How many notes the panel keeps (older ones scroll away).
    pub keep: usize,
}
impl Default for ChatCfg {
    fn default() -> Self {
        ChatCfg { show: true, width: 0.22, side: true, height: 0.2, dock: ChatDock::Bottom, font: 14.0, keep: 200 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CometsCfg {
    pub seconds: f32,
    pub trail: usize,
    pub brightness: f32,
    pub size: f32,
}
impl Default for CometsCfg {
    fn default() -> Self {
        CometsCfg { seconds: 1.8, trail: 28, brightness: 5.0, size: 1.0 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WarpCfg {
    pub decay: f32,
    pub on_broadcast: f32,
    /// Warp every note adds, on top of a broadcast's kick (Y, t88: "more
    /// messages warp the core further"): notes compound remaining warp.
    pub per_note: f32,
    /// The most warp can stack to.
    pub max: f32,
}
impl Default for WarpCfg {
    fn default() -> Self {
        WarpCfg { decay: 0.22, on_broadcast: 1.0, per_note: 0.15, max: 3.0 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeatsCfg {
    pub size: f32,
    /// How brightly each shown state burns; above 1 blooms.
    pub brightness: HashMap<String, f32>,
    pub other: f32,
}
impl Default for SeatsCfg {
    fn default() -> Self {
        let brightness = [("busy", 3.2), ("stuck", 2.0), ("ready", 1.1), ("idle", 1.1), ("spent", 0.35)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        SeatsCfg { size: 1.0, brightness, other: 0.22 }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FuelCfg {
    pub show: bool,
    /// At or over this percentage a ring turns warn, then danger.
    pub warn: f32,
    pub danger: f32,
    pub ok_colour: [f32; 3],
    pub warn_colour: [f32; 3],
    pub danger_colour: [f32; 3],
    /// How bright the empty part of a ring is (0 hides it).
    pub empty: f32,
}
impl Default for FuelCfg {
    fn default() -> Self {
        FuelCfg {
            show: true,
            warn: 70.0,
            danger: 90.0,
            ok_colour: [0.2, 1.3, 0.55],
            warn_colour: [1.8, 1.0, 0.1],
            danger_colour: [3.0, 0.25, 0.15],
            empty: 0.08,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MoonsCfg {
    pub show: bool,
    pub max: usize,
    pub fresh: [f32; 3],
    pub old: [f32; 3],
    /// How old (hours) the oldest waiting note is when moons are fully `old`.
    pub old_hours: f32,
    pub speed: f32,
}
impl Default for MoonsCfg {
    fn default() -> Self {
        MoonsCfg { show: true, max: 24, fresh: [1.2, 1.2, 1.3], old: [2.5, 0.25, 0.1], old_hours: 6.0, speed: 0.6 }
    }
}

/// t30: subagents an agent says it spawned, orbiting it as a closer moon band.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SubagentsCfg {
    pub show: bool,
    pub speed: f32,
    /// The innermost orbit radius, in screen pixels before scene scale.
    pub radius: f32,
    /// Extra radius per band of eight.
    pub spacing: f32,
    pub size: f32,
    pub bright: f32,
    pub max: usize,
    /// Standing orbitals circle the core on their own ellipses (t106):
    /// the innermost semi-major axis and the step each further one takes,
    /// as shares of the scene's short side.
    pub ellipse_in: f32,
    pub ellipse_step: f32,
    /// Semi-minor / semi-major.
    pub ellipse_flat: f32,
    pub ellipse_speed: f32,
    /// A stopped orbital's brightness, as a share of `bright` (the dim
    /// idle cue on a standing ellipse).
    pub idle: f32,
}
impl Default for SubagentsCfg {
    fn default() -> Self {
        SubagentsCfg {
            show: true,
            speed: 1.6,
            radius: 34.0,
            spacing: 7.0,
            size: 2.0,
            bright: 1.2,
            max: 8,
            ellipse_in: 0.14,
            ellipse_step: 0.035,
            ellipse_flat: 0.7,
            ellipse_speed: 0.22,
            idle: 0.35,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HudCfg {
    pub show: bool,
    /// The line of seats that aren't on the ring, with their queues.
    pub dock: bool,
    /// The card under the pointer when it rests on a seat (off: Y, 2026-11-27).
    pub hover_card: bool,
    /// The system stats down the scene's left side: GPU frames, RAM, swap,
    /// load, power and each seat's context (t53).
    pub stats: bool,
    /// The current tasks at the bottom left: id, title, holder, how long.
    pub tasks_panel: bool,
}
impl Default for HudCfg {
    fn default() -> Self {
        HudCfg { show: true, dock: true, hover_card: false, stats: true, tasks_panel: true }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TasksCfg {
    pub show: bool,
    /// Radians a second the task ring turns.
    pub speed: f32,
    /// The ring's radius as a share of the seat ring's.
    pub radius: f32,
    pub size: f32,
    pub bright: f32,
}
impl Default for TasksCfg {
    fn default() -> Self {
        TasksCfg { show: true, speed: 0.22, radius: 0.42, size: 0.5, bright: 1.1 }
    }
}

/// One effect rule as written in the toml.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuleSpec {
    /// "seat" (checked every frame for each seat on the ring) or "note"
    /// (checked once for each note as it flies).
    pub on: String,
    pub when: String,
    pub effect: String,
    pub colour: Option<Colour>,
    pub pulse: f32,
    pub size: f32,
    pub bright: f32,
    pub text: String,
    pub seconds: Option<f32>,
    pub strength: f32,
}
impl Default for RuleSpec {
    fn default() -> Self {
        RuleSpec {
            on: "seat".into(),
            when: "true".into(),
            effect: String::new(),
            colour: None,
            pulse: 0.0,
            size: 1.0,
            bright: 1.0,
            text: String::new(),
            seconds: None,
            strength: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub look: Look,
    pub core: CoreCfg,
    pub orbit: OrbitCfg,
    pub physics: PhysicsCfg,
    pub stars: StarsCfg,
    pub comets: CometsCfg,
    pub warp: WarpCfg,
    pub seats: SeatsCfg,
    pub fuel: FuelCfg,
    pub moons: MoonsCfg,
    pub subagents: SubagentsCfg,
    pub hud: HudCfg,
    pub tasks: TasksCfg,
    pub chat: ChatCfg,
    pub typing: TypingCfg,
    pub splash: SplashCfg,
    pub conduits: ConduitsCfg,
    pub planets: PlanetsCfg,
    pub kitten: KittenCfg,
    pub left: LeftCfg,
    pub rule: Vec<RuleSpec>,
}

impl Config {
    pub fn brightness(&self, shown: &str) -> f32 {
        self.seats.brightness.get(shown).copied().unwrap_or(self.seats.other)
    }
}

pub const SEAT_EFFECTS: &[&str] = &["halo", "tint", "glow", "beacon", "label"];
pub const NOTE_EFFECTS: &[&str] = &["comet", "warp", "shockwave"];

pub const SEAT_FIELDS: &[&str] = &[
    "name", "state", "shown", "online", "busy", "idle", "ready", "stuck", "spent", "waiting",
    "waiting_normal", "oldest_waiting_s", "oldest_waiting_h", "since_s", "held_by_draft",
    "paused", "ctx", "five_hour", "week", "fullest", "has_usage", "focused",
];
pub const NOTE_FIELDS: &[&str] = &["from", "to", "all", "to_owner", "to_count", "quiet", "len"];

#[derive(Clone, Debug)]
pub struct Rule {
    pub spec: RuleSpec,
    pub when: Expr,
}

/// Parses the toml and compiles every rule; any mistake refuses the lot.
pub fn load_config(text: &str) -> Result<(Config, Vec<Rule>), String> {
    let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut rules = Vec::new();
    for (i, spec) in cfg.rule.iter().enumerate() {
        let n = i + 1;
        let (effects, fields) = match spec.on.as_str() {
            "seat" => (SEAT_EFFECTS, SEAT_FIELDS),
            "note" => (NOTE_EFFECTS, NOTE_FIELDS),
            other => return Err(format!("rule {n}: on = '{other}', expected 'seat' or 'note'")),
        };
        if !effects.contains(&spec.effect.as_str()) {
            return Err(format!(
                "rule {n}: effect '{}' isn't one for {}s (try {})",
                spec.effect,
                spec.on,
                effects.join(", ")
            ));
        }
        if let Some(c) = &spec.colour {
            c.rgb([1.0; 3]).map_err(|e| format!("rule {n}: {e}"))?;
        }
        let when = Expr::compile(&spec.when, fields).map_err(|e| format!("rule {n} when: {e}"))?;
        rules.push(Rule { spec: spec.clone(), when });
    }
    Ok((cfg, rules))
}

/// Checks a WGSL shader the way the GPU driver will see it, so a broken
/// edit is refused here instead of failing the pipeline on screen.
pub fn check_wgsl(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.emit_to_string(source))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map_err(|e| e.emit_to_string(source))?;
    Ok(())
}

/// The first line of an error, for the HUD; the whole thing goes to stderr.
pub fn short(err: &str) -> String {
    let line = err.lines().find(|l| !l.trim().is_empty()).unwrap_or(err).trim();
    line.chars().take(140).collect()
}

/// Notices when a mod file has changed since it was last looked at.
#[derive(Default)]
pub struct Watch {
    stamps: HashMap<PathBuf, Option<SystemTime>>,
}

impl Watch {
    /// True the first time it sees a path, and whenever its mtime moves
    /// (including the file appearing or disappearing).
    pub fn changed(&mut self, path: &Path) -> bool {
        let now = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        match self.stamps.insert(path.to_path_buf(), now) {
            None => true,
            Some(before) => before != now,
        }
    }
}

/// A mod file's text, or the built-in default when there is no such file.
pub fn read_or(path: &Path, default: &'static str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| default.to_string())
}

// ---------------------------------------------------------------- the rule language

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Val {
    pub fn truthy(&self) -> bool {
        match self {
            Val::Bool(b) => *b,
            Val::Num(n) => *n != 0.0,
            Val::Str(s) => !s.is_empty(),
        }
    }
    fn num(&self) -> f64 {
        match self {
            Val::Num(n) => *n,
            Val::Bool(b) => *b as u8 as f64,
            Val::Str(_) => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Contains,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Lit(Val),
    Var(String),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Bin(Op, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Sym(&'static str),
}

fn tokens(src: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.' || cs[i] == '_') {
                i += 1;
            }
            let text: String = cs[start..i].iter().filter(|c| **c != '_').collect();
            out.push(Tok::Num(text.parse().map_err(|_| format!("bad number '{text}'"))?));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(cs[start..i].iter().collect()));
        } else if c == '\'' || c == '"' {
            let start = i + 1;
            i += 1;
            while i < cs.len() && cs[i] != c {
                i += 1;
            }
            if i >= cs.len() {
                return Err("a string is missing its closing quote".into());
            }
            out.push(Tok::Str(cs[start..i].iter().collect()));
            i += 1;
        } else {
            let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
            let sym = ["||", "&&", "==", "!=", "<=", ">="]
                .into_iter()
                .find(|s| *s == two)
                .or_else(|| ["!", "<", ">", "+", "-", "*", "/", "(", ")"].into_iter().find(|s| s.starts_with(c)));
            let Some(sym) = sym else { return Err(format!("unexpected '{c}'")) };
            i += sym.len();
            out.push(Tok::Sym(sym));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    at: usize,
    fields: &'a [&'a str],
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn eat_sym(&mut self, syms: &[&str]) -> Option<&'static str> {
        if let Some(Tok::Sym(s)) = self.peek() {
            if syms.contains(s) {
                let s = *s;
                self.at += 1;
                return Some(s);
            }
        }
        None
    }
    fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Ident(w)) if w == word) {
            self.at += 1;
            return true;
        }
        false
    }
    fn or(&mut self) -> Result<Expr, String> {
        let mut e = self.and()?;
        while self.eat_sym(&["||"]).is_some() || self.eat_word("or") {
            e = Expr::Bin(Op::Or, Box::new(e), Box::new(self.and()?));
        }
        Ok(e)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut e = self.not()?;
        while self.eat_sym(&["&&"]).is_some() || self.eat_word("and") {
            e = Expr::Bin(Op::And, Box::new(e), Box::new(self.not()?));
        }
        Ok(e)
    }
    fn not(&mut self) -> Result<Expr, String> {
        if self.eat_sym(&["!"]).is_some() || self.eat_word("not") {
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let e = self.sum()?;
        let op = match self.eat_sym(&["==", "!=", "<=", ">=", "<", ">"]) {
            Some("==") => Op::Eq,
            Some("!=") => Op::Ne,
            Some("<=") => Op::Le,
            Some(">=") => Op::Ge,
            Some("<") => Op::Lt,
            Some(">") => Op::Gt,
            _ if self.eat_word("contains") => Op::Contains,
            _ => return Ok(e),
        };
        Ok(Expr::Bin(op, Box::new(e), Box::new(self.sum()?)))
    }
    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.prod()?;
        while let Some(s) = self.eat_sym(&["+", "-"]) {
            let op = if s == "+" { Op::Add } else { Op::Sub };
            e = Expr::Bin(op, Box::new(e), Box::new(self.prod()?));
        }
        Ok(e)
    }
    fn prod(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        while let Some(s) = self.eat_sym(&["*", "/"]) {
            let op = if s == "*" { Op::Mul } else { Op::Div };
            e = Expr::Bin(op, Box::new(e), Box::new(self.unary()?));
        }
        Ok(e)
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_sym(&["-"]).is_some() {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.atom()
    }
    fn atom(&mut self) -> Result<Expr, String> {
        let Some(tok) = self.peek().cloned() else { return Err("the expression ends too soon".into()) };
        self.at += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Lit(Val::Num(n))),
            Tok::Str(s) => Ok(Expr::Lit(Val::Str(s))),
            Tok::Ident(w) if w == "true" => Ok(Expr::Lit(Val::Bool(true))),
            Tok::Ident(w) if w == "false" => Ok(Expr::Lit(Val::Bool(false))),
            Tok::Ident(w) => {
                if !self.fields.contains(&w.as_str()) {
                    return Err(format!("no field '{w}' (there's {})", self.fields.join(", ")));
                }
                Ok(Expr::Var(w))
            }
            Tok::Sym("(") => {
                let e = self.or()?;
                if self.eat_sym(&[")"]).is_none() {
                    return Err("a '(' is missing its ')'".into());
                }
                Ok(e)
            }
            Tok::Sym(s) => Err(format!("unexpected '{s}'")),
        }
    }
}

impl Expr {
    /// Parses `src`, allowing only the named fields, so a typo is caught
    /// when the mod loads rather than silently never matching.
    pub fn compile(src: &str, fields: &[&str]) -> Result<Expr, String> {
        let mut p = Parser { toks: tokens(src)?, at: 0, fields };
        let e = p.or()?;
        if let Some(t) = p.peek() {
            return Err(format!("unexpected {t:?} after the expression"));
        }
        Ok(e)
    }

    pub fn eval(&self, get: &dyn Fn(&str) -> Val) -> Val {
        match self {
            Expr::Lit(v) => v.clone(),
            Expr::Var(name) => get(name),
            Expr::Not(e) => Val::Bool(!e.eval(get).truthy()),
            Expr::Neg(e) => Val::Num(-e.eval(get).num()),
            Expr::Bin(Op::Or, a, b) => Val::Bool(a.eval(get).truthy() || b.eval(get).truthy()),
            Expr::Bin(Op::And, a, b) => Val::Bool(a.eval(get).truthy() && b.eval(get).truthy()),
            Expr::Bin(op, a, b) => {
                let (a, b) = (a.eval(get), b.eval(get));
                match op {
                    Op::Eq => Val::Bool(same(&a, &b)),
                    Op::Ne => Val::Bool(!same(&a, &b)),
                    Op::Lt => Val::Bool(a.num() < b.num()),
                    Op::Le => Val::Bool(a.num() <= b.num()),
                    Op::Gt => Val::Bool(a.num() > b.num()),
                    Op::Ge => Val::Bool(a.num() >= b.num()),
                    Op::Add => Val::Num(a.num() + b.num()),
                    Op::Sub => Val::Num(a.num() - b.num()),
                    Op::Mul => Val::Num(a.num() * b.num()),
                    Op::Div => Val::Num(if b.num() == 0.0 { 0.0 } else { a.num() / b.num() }),
                    Op::Contains => Val::Bool(match (&a, &b) {
                        (Val::Str(list), Val::Str(x)) => list.split(',').any(|p| p.trim() == x),
                        _ => false,
                    }),
                    Op::Or | Op::And => unreachable!(),
                }
            }
        }
    }

    pub fn matches(&self, get: &dyn Fn(&str) -> Val) -> bool {
        self.eval(get).truthy()
    }
}

fn same(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Str(x), Val::Str(y)) => x.eq_ignore_ascii_case(y),
        (Val::Str(_), _) | (_, Val::Str(_)) => false,
        _ => a.num() == b.num(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(name: &str) -> Val {
        match name {
            "name" => Val::Str("codex".into()),
            "waiting" => Val::Num(103.0),
            "oldest_waiting_h" => Val::Num(67.0),
            "busy" => Val::Bool(false),
            "spent" => Val::Bool(true),
            _ => Val::Num(-1.0),
        }
    }

    #[test]
    fn rules_read_like_conditions() {
        let t = |src: &str| Expr::compile(src, SEAT_FIELDS).unwrap().matches(&seat);
        assert!(t("waiting > 100"));
        assert!(t("spent && oldest_waiting_h >= 24"));
        assert!(t("spent and not busy"));
        assert!(!t("busy || waiting < 10"));
        assert!(t("name == 'CODEX'"), "names compare without case");
        assert!(t("waiting / 2 > 50 && (busy || spent)"));
        assert!(t("-ctx == 1"), "unknown meters read as -1");
    }

    #[test]
    fn typos_are_caught_at_load_not_silently_false() {
        assert!(Expr::compile("wating > 3", SEAT_FIELDS).unwrap_err().contains("no field 'wating'"));
        assert!(Expr::compile("waiting >", SEAT_FIELDS).is_err());
        assert!(Expr::compile("(waiting > 3", SEAT_FIELDS).is_err());
        assert!(Expr::compile("waiting > 3 3", SEAT_FIELDS).is_err());
        assert!(Expr::compile("name == 'x", SEAT_FIELDS).is_err());
    }

    #[test]
    fn contains_reads_comma_lists() {
        let e = Expr::compile("to contains 'owner'", NOTE_FIELDS).unwrap();
        let get = |f: &str| if f == "to" { Val::Str("vibe, owner".into()) } else { Val::Num(0.0) };
        assert!(e.matches(&get));
        let get = |f: &str| if f == "to" { Val::Str("ownerish".into()) } else { Val::Num(0.0) };
        assert!(!e.matches(&get), "whole names only");
    }

    #[test]
    fn the_shipped_mods_load_and_validate() {
        let (cfg, rules) = load_config(DEFAULT_CONFIG).expect("the default toml loads");
        assert!(!rules.is_empty(), "it ships with example rules");
        assert_eq!(cfg.look.fps, 60.0);
        check_wgsl(DEFAULT_CORE_SHADER).expect("core.wgsl validates");
        check_wgsl(DEFAULT_RING_SHADER).expect("ring.wgsl validates");
        check_wgsl(DEFAULT_PLANET_SHADER).expect("planet.wgsl validates");
    }

    #[test]
    fn bad_mods_are_refused_with_a_reason() {
        assert!(load_config("[look]\nbloom = 'lots'").is_err());
        assert!(load_config("[look]\nblom = 0.3").unwrap_err().contains("blom"), "unknown keys are named");
        let e = load_config("[[rule]]\nwhen = 'busy'\neffect = 'sparkle'").unwrap_err();
        assert!(e.contains("sparkle") && e.contains("halo"));
        let e = load_config("[[rule]]\nwhen = 'busy'\neffect = 'tint'\ncolour = 'mauve'").unwrap_err();
        assert!(e.contains("mauve"));
        assert!(check_wgsl("fn broken( {").is_err());
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        let (cfg, rules) = load_config("").unwrap();
        assert!(rules.is_empty());
        assert_eq!(cfg.comets.seconds, CometsCfg::default().seconds);
    }

    #[test]
    fn the_watch_sees_a_file_change() {
        let dir = std::env::temp_dir().join(format!("warp-core-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.toml");
        let mut w = Watch::default();
        assert!(w.changed(&p), "first look counts, even for a missing file");
        assert!(!w.changed(&p));
        std::fs::write(&p, "x").unwrap();
        assert!(w.changed(&p), "appearing counts");
        assert!(!w.changed(&p));
        std::fs::remove_dir_all(&dir).ok();
        assert!(w.changed(&p), "disappearing counts");
    }
}
