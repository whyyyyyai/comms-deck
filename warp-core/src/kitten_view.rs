//! Lumo's seat as a quantum kitten (t78, Y: "Lumo's job will be to improve
//! the physics engine of the warp core ... a quantum-kitten helper ... that
//! jumps around the warp core in a semi random semi deterministic way").
//! Designed in five rounds by Claude, Codex, Devin and Vibe; the logic is
//! `kitten` (Codex), the knobs `[kitten]` (Devin), the line recipes
//! docs/quantum-kitten.md (Vibe), and this file draws and wires it (Claude).
//!
//! She keeps a fixed anchor at Lumo's ring slot (label, routing, hover) and
//! never enters the planets' gravity sim: she hops round the anchor on her
//! own deterministic law, so the physics she is there to improve stays clean.
//!
//! Written by: claude

use super::*;
use crate::kitten::{self, Event, Pose};

pub const SEAT: &str = "lumo";
/// How long the arc of a hop takes, seconds.
const FLIGHT_S: f32 = 0.35;

#[derive(Resource)]
pub struct KittenView {
    /// Lumo's ring slot this frame (world), if she's on the roster.
    pub anchor: Option<Vec2>,
    /// Where the kitten is drawn this frame (world): comets to Lumo land here.
    pub pos: Vec2,
    pub pose: Pose,
    /// The current hop, with offsets relative to the anchor (the ring turns).
    hop: Option<kitten::Hop>,
    queue: kitten::Queue,
    seed: u64,
    /// The waiting count last frame; None until the first status sample (a baseline).
    last_waiting: Option<u64>,
    /// Smoothed fps, for the ghost gate.
    fps: f32,
    /// Where a relay perched (relative to the anchor), for the hop home.
    resume_from: Option<Vec2>,
    seed_logged: bool,
}

impl Default for KittenView {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            ^ 0x51_7C_C1_B7_27_22_0A_95;
        KittenView {
            anchor: None,
            pos: Vec2::ZERO,
            pose: Pose::Boxed,
            hop: None,
            queue: kitten::Queue::new(kitten::Durations { catch_s: 0.6, relay_s: 1.2, perch_s: 0.8, rattle_s: 0.4 }),
            seed,
            last_waiting: None,
            fps: 60.0,
            resume_from: None,
            seed_logged: false,
        }
    }
}

impl KittenView {
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// A note from Lumo has launched: the kitten chases it to `to`.
    pub fn sent(&mut self, note_id: &str, to: &[String], now: f32) {
        if let Some(target) = kitten::relay_target(to, SEAT) {
            let key = format!("r:{note_id}");
            let ev = if matches!(self.pose, Pose::Boxed | Pose::Spent) { Event::Flash } else { Event::Relay { to: target } };
            self.queue.push(&key, ev, now);
        }
    }
    /// A note to Lumo has landed on her: catch it (or rattle the box).
    pub fn landed(&mut self, note_id: &str, now: f32) {
        let key = format!("c:{note_id}");
        let ev = if self.pose == Pose::Boxed { Event::Rattle } else { Event::Catch };
        self.queue.push(&key, ev, now);
    }
}

/// The kitten is on when [kitten] enable says so.
pub fn on(mods: &Mods) -> bool {
    mods.cfg.kitten.enable
}

/// Tag and "+N" text under the kitten.
#[derive(Component)]
pub struct KittenText;

/// Each frame: find the anchor, read Lumo's seat, keep the hop schedule.
pub fn kitten_step(
    mut commands: Commands,
    mut kit: ResMut<KittenView>,
    deck: Res<Deck>,
    mods: Res<Mods>,
    layout: Res<Layout>,
    time: Res<Time>,
    real: Res<Time<Real>>,
    nodes: Query<(&SeatNode, &Transform)>,
    mut text: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<KittenText>, Without<SeatNode>)>,
    mut spawned: Local<bool>,
) {
    let cfg = &mods.cfg.kitten;
    if !*spawned {
        commands.spawn((
            KittenText,
            Text2d::new(""),
            TextFont { font_size: FontSize::Px(13.0), ..default() },
            TextColor(seat_colour(SEAT, 0.8)),
            Anchor::TOP_CENTER,
            Transform::from_xyz(0.0, 0.0, 4.0),
            Visibility::Hidden,
        ));
        *spawned = true;
    }
    let dt = real.delta_secs().max(1e-4);
    kit.fps = kit.fps * 0.95 + (1.0 / dt) * 0.05;
    let now = time.elapsed_secs();
    kit.anchor = if cfg.enable { ring_positions(&deck.roster, layout.scene, deck.orbit).get(SEAT).map(|(p, _)| *p) } else { None };
    let Some(anchor) = kit.anchor else {
        for (_, _, mut vis) in &mut text {
            *vis = Visibility::Hidden;
        }
        return;
    };
    if !kit.seed_logged {
        println!("warp-core kitten: session seed {:016x}", kit.seed);
        kit.seed_logged = true;
    }
    let seat = deck.seat(SEAT);
    let shown = seat.map(|s| s.shown.as_str()).unwrap_or("not set up");
    kit.pose = kitten::pose(shown);
    let waiting = seat.map(|s| s.waiting).unwrap_or(0);
    let oldest = seat.map(|s| s.oldest_waiting_s).unwrap_or(0.0);
    // A rise in waiting with no live landing behind it (an app seat polls):
    // rattle the box. The first sample is only a baseline.
    if let Some(before) = kit.last_waiting {
        if waiting > before && kit.pose == Pose::Boxed {
            let key = format!("w:{waiting}:{}", (now / 2.0) as u64);
            kit.queue.push(&key, Event::Rattle, now);
        }
    }
    kit.last_waiting = Some(waiting);

    // The hop schedule, relative to the anchor.
    let task = kitten::held_task(&deck.tasks, SEAT).map(|t| t.id.clone());
    let n = deck.roster.len().max(1) as f32;
    let spacing = std::f32::consts::TAU * anchor.length().max(1.0) / n;
    let reach = cfg.reach * spacing;
    let blocked: Vec<(Vec2, f32)> = nodes
        .iter()
        .map(|(_, tf)| (tf.translation.truncate() - anchor, 34.0))
        .chain(std::iter::once((-anchor, 90.0)))
        .collect();
    let tempo = kitten::Tempo { base_s: cfg.base_dwell_s, floor: cfg.dwell_floor, min_s: cfg.dwell_min_s };
    let key = kitten::stable_key(shown, waiting, task.as_deref());
    let relaying = matches!(kit.queue.peek(), Some(Event::Relay { .. }));
    if kitten::hops(kit.pose) && !relaying {
        let due = kit.hop.as_ref().is_none_or(|h| now >= h.start + FLIGHT_S);
        if due {
            let prev = match (kit.hop.take(), kit.resume_from.take()) {
                (_, Some(perch)) => Some(kitten::Hop { index: 0, from: perch, to: perch, ghosts: vec![], start: now, dwell: 0.0 }),
                (h, None) => h,
            };
            let seed = kit.seed;
            kit.hop = Some(kitten::next_hop(prev.as_ref(), now, Vec2::ZERO, reach, seed, key, &blocked, cfg.size, &tempo, oldest));
        }
    } else if !kitten::hops(kit.pose) {
        kit.hop = None;
    }
    let rel = match &kit.hop {
        Some(h) if now >= h.start => {
            let k = ((now - h.start) / FLIGHT_S).clamp(0.0, 1.0);
            // A little arc: up and over, the way a cat jumps.
            let lift = (k * std::f32::consts::PI).sin() * cfg.size * 1.4;
            h.from.lerp(h.to, k) + Vec2::Y * lift
        }
        Some(h) => h.from,
        None => Vec2::ZERO,
    };
    kit.pos = anchor + rel;

    // The tag (held task) and "+N" under her.
    let yarn = kitten::yarn(waiting, oldest, cfg.yarn_max);
    let mut words = Vec::new();
    if let Some(t) = &task {
        words.push(t.clone());
    }
    if yarn.extra > 0 {
        words.push(format!("+{}", yarn.extra));
    }
    for (mut t, mut tf, mut vis) in &mut text {
        // Her name, like every planet's label, then the tag and "+N".
        let line = if words.is_empty() { label_for(SEAT) } else { format!("{}\n{}", label_for(SEAT), words.join("  ")) };
        if t.0 != line {
            t.0 = line;
        }
        tf.translation.x = anchor.x;
        tf.translation.y = anchor.y - cfg.size * 3.2;
        *vis = Visibility::Inherited;
    }
}

/// Polyline helper.
fn poly(g: &mut Gizmos, pts: &[Vec2], c: Color) {
    for w in pts.windows(2) {
        g.line_2d(w[0], w[1], c);
    }
}

/// An arc from angle a0 to a1 (radians, 0 = +x, counter-clockwise).
fn arc(g: &mut Gizmos, centre: Vec2, r: f32, a0: f32, a1: f32, c: Color) {
    let steps = (((a1 - a0).abs() / 0.25).ceil() as usize).clamp(3, 32);
    let pts: Vec<Vec2> = (0..=steps).map(|i| centre + Vec2::from_angle(a0 + (a1 - a0) * i as f32 / steps as f32) * r).collect();
    poly(g, &pts, c);
}

/// The kitten's body in one pose at `p`, `s` = size, facing right.
/// `wiggle` 0..1 animates (tail flick, batting paw, box rattle).
fn draw_body(g: &mut Gizmos, pose: Pose, p: Vec2, s: f32, c: Color, wiggle: f32, physics: bool) {
    let head = |g: &mut Gizmos, h: Vec2, r: f32| {
        g.circle_2d(Isometry2d::from_translation(h), r, c);
        // Ears: two small triangles.
        poly(g, &[h + Vec2::new(-r * 0.8, r * 0.5), h + Vec2::new(-r * 0.5, r * 1.5), h + Vec2::new(-r * 0.1, r * 0.9)], c);
        poly(g, &[h + Vec2::new(r * 0.1, r * 0.9), h + Vec2::new(r * 0.5, r * 1.5), h + Vec2::new(r * 0.8, r * 0.5)], c);
    };
    match pose {
        Pose::Boxed => {
            let jig = (wiggle * 40.0).sin() * s * 0.12 * wiggle.min(1.0);
            let b = p + Vec2::new(jig, 0.0);
            g.rect_2d(Isometry2d::from_translation(b), Vec2::splat(s * 2.2), c);
            // Lid flaps and the "?" latch.
            poly(g, &[b + Vec2::new(-s * 1.1, s * 1.1), b + Vec2::new(-s * 0.4, s * 1.6), b + Vec2::new(0.0, s * 1.1)], c);
            poly(g, &[b + Vec2::new(0.0, s * 1.1), b + Vec2::new(s * 0.4, s * 1.6), b + Vec2::new(s * 1.1, s * 1.1)], c);
            arc(g, b + Vec2::new(0.0, s * 0.3), s * 0.35, -0.6, std::f32::consts::PI, c);
            g.line_2d(b + Vec2::new(s * 0.2, s * 0.05), b + Vec2::new(0.0, -s * 0.25), c);
            g.circle_2d(Isometry2d::from_translation(b + Vec2::new(0.0, -s * 0.55)), s * 0.06, c);
        }
        Pose::Ready | Pose::Stuck | Pose::Busy => {
            // Sitting: a pear body, head on top, tail rising.
            g.ellipse_2d(Isometry2d::from_translation(p), Vec2::new(s * 0.75, s * 0.95), c);
            head(g, p + Vec2::new(s * 0.15, s * 1.25), s * 0.55);
            let flick = if pose == Pose::Stuck { (wiggle * 18.0).sin() * 0.5 } else { (wiggle * 3.0).sin() * 0.15 };
            let t0 = p + Vec2::new(-s * 0.7, -s * 0.6);
            poly(g, &[t0, t0 + Vec2::new(-s * 0.6, s * 0.4), t0 + Vec2::new(-s * (0.5 + flick), s * 1.3)], c);
            if pose == Pose::Busy {
                // Batting: a paw reaching out and back.
                let reach = 0.6 + 0.5 * (wiggle * 9.0).sin().abs();
                let paw = p + Vec2::new(s * (0.6 + reach), s * 0.5);
                g.line_2d(p + Vec2::new(s * 0.5, s * 0.3), paw, c);
                g.circle_2d(Isometry2d::from_translation(paw), s * 0.12, c);
                if physics {
                    // Her job: batting at a little orbit preview.
                    let o = p + Vec2::new(s * 2.4, s * 0.6);
                    let dim = c.with_alpha(0.5);
                    g.ellipse_2d(Isometry2d::from_translation(o), Vec2::new(s * 0.9, s * 0.45), dim);
                    let a = wiggle * 5.0;
                    g.circle_2d(Isometry2d::from_translation(o + Vec2::new(a.cos() * s * 0.9, a.sin() * s * 0.45)), s * 0.1, c);
                }
            }
        }
        Pose::Away | Pose::Resting | Pose::Spent => {
            // Curled: a low oval with the tail wrapped round, head tucked.
            g.ellipse_2d(Isometry2d::from_translation(p), Vec2::new(s * 1.1, s * 0.6), c);
            head(g, p + Vec2::new(s * 0.8, s * 0.1), s * 0.45);
            arc(g, p, s * 1.25, std::f32::consts::PI * 1.05, std::f32::consts::PI * 1.95, c);
            if pose == Pose::Spent {
                // Asleep: a slow "z" and a dim cooldown arc above.
                let z = p + Vec2::new(s * 1.2, s * (1.2 + 0.2 * (wiggle * 1.5).sin()));
                poly(g, &[z, z + Vec2::new(s * 0.35, 0.0), z + Vec2::new(0.0, -s * 0.35), z + Vec2::new(s * 0.35, -s * 0.35)], c);
                arc(g, p, s * 1.8, 0.3, std::f32::consts::PI - 0.3, c.with_alpha(0.35));
            }
            if pose == Pose::Resting {
                // "idle until ...": a small clock.
                let k = p + Vec2::new(-s * 1.3, s * 0.9);
                g.circle_2d(Isometry2d::from_translation(k), s * 0.3, c);
                g.line_2d(k, k + Vec2::new(0.0, s * 0.22), c);
                g.line_2d(k, k + Vec2::new(s * 0.16, 0.0), c);
            }
        }
    }
}

/// Draw her: ghosts, body, yarn, collar, limit arcs, and the relay/catch beats.
pub fn draw_kitten(mut kit: ResMut<KittenView>, deck: Res<Deck>, mods: Res<Mods>, time: Res<Time>, nodes: Query<(&SeatNode, &Transform)>, mut g: Gizmos) {
    let cfg = &mods.cfg.kitten;
    let Some(anchor) = kit.anchor else { return };
    let now = time.elapsed_secs();
    let s = cfg.size;
    let base = seat_rgb(SEAT);
    let c = lin(base, 1.0);
    let seat = deck.seat(SEAT);
    let pose = kit.pose;
    let physics = kitten::held_task(&deck.tasks, SEAT).is_some_and(|t| kitten::is_physics(t.title.as_deref(), &cfg.physics_words));

    // The anchor marker (Vibe's recipe): a dim dashed circle at her ring
    // slot. Her label and routing hang off it; it never moves with her.
    for i in 0..12 {
        let a0 = i as f32 * std::f32::consts::TAU / 12.0;
        arc(&mut g, anchor, s * 0.9, a0, a0 + std::f32::consts::TAU / 24.0, c.with_alpha(0.25));
    }

    // Ghosts in the ghost window before a hop (superposition, then collapse).
    if let Some(h) = &kit.hop {
        let until = h.start - now;
        if until > 0.0 && until <= cfg.ghost_s && mods::ghosts_allowed(kit.fps, cfg) {
            let a = cfg.ghost_alpha * (1.0 - until / cfg.ghost_s.max(1e-3));
            for gp in &h.ghosts {
                draw_body(&mut g, pose, anchor + *gp, s, c.with_alpha(a), now, false);
            }
        }
    }

    // The queue's current beat.
    let beat = kit.queue.current(now).map(|(e, k)| (e.clone(), k));
    let mut body_at = kit.pos;
    let mut wiggle = now;
    match &beat {
        Some((Event::Relay { to }, k)) => {
            // Leap after the packet to the recipient's rim, perch, hold.
            let to_pos = nodes.iter().find(|(n, _)| n.name == *to).map(|(_, tf)| tf.translation.truncate());
            if let Some(tp) = to_pos {
                let rim = tp + (anchor - tp).normalize_or_zero() * 40.0;
                let fly = cfg.relay_s / (cfg.relay_s + cfg.perch_s).max(1e-3);
                let k2 = (k / fly.max(1e-3)).min(1.0);
                let lift = (k2 * std::f32::consts::PI).sin() * s * 3.0;
                body_at = anchor.lerp(rim, 1.0 - (1.0 - k2).powi(2)) + Vec2::Y * lift;
                // The glowing ball in her mouth until she drops it.
                if *k < fly {
                    g.circle_2d(Isometry2d::from_translation(body_at + Vec2::new(s * 0.9, s * 1.0)), s * 0.25, lin(base, 2.0));
                }
                kit.resume_from = Some(rim - anchor);
                kit.hop = None;
                kit.pos = body_at;
            }
        }
        Some((Event::Catch, k)) => {
            // Two paws up, and a sparkle.
            let up = (k * std::f32::consts::PI).sin();
            for side in [-1.0f32, 1.0] {
                g.line_2d(body_at + Vec2::new(side * s * 0.4, s * 0.3), body_at + Vec2::new(side * s * 0.7, s * (0.8 + up)), c);
            }
            for i in 0..6 {
                let a = i as f32 * std::f32::consts::TAU / 6.0 + k * 2.0;
                let r = s * (1.2 + 1.5 * k);
                let sp = body_at + Vec2::new(0.0, s * 1.6) + Vec2::from_angle(a) * r * 0.4;
                g.line_2d(sp, sp + Vec2::from_angle(a) * s * 0.3 * (1.0 - k), lin(base, 1.8 * (1.0 - k)));
            }
        }
        Some((Event::Rattle, k)) => wiggle = *k * 10.0,
        Some((Event::Flash, k)) => {
            g.circle_2d(Isometry2d::from_translation(anchor), s * (1.5 + 2.0 * k), lin(base, 1.5 * (1.0 - k)));
        }
        None => {}
    }
    draw_body(&mut g, pose, body_at, s, c, wiggle, physics);

    // Yarn beside the anchor: up to yarn_max balls, the oldest tinted by age.
    let waiting = seat.map(|x| x.waiting).unwrap_or(0);
    let oldest = seat.map(|x| x.oldest_waiting_s).unwrap_or(0.0);
    let yarn = kitten::yarn(waiting, oldest, cfg.yarn_max);
    for i in 0..yarn.balls {
        let yp = anchor + Vec2::new(-s * (2.2 + 0.9 * i as f32), -s * 1.6);
        let col = if i == 0 { lin(mix3([0.4, 0.7, 1.0], [1.0, 0.25, 0.2], yarn.age_tint), 1.0) } else { c.with_alpha(0.7) };
        g.circle_2d(Isometry2d::from_translation(yp), s * 0.35, col);
        g.line_2d(yp + Vec2::new(-s * 0.25, -s * 0.1), yp + Vec2::new(s * 0.25, s * 0.15), col);
    }

    // Collar = context; two thin arcs outside it = five-hour and week limits.
    if !matches!(pose, Pose::Boxed) {
        let neck = body_at + Vec2::new(s * 0.15, s * 0.75);
        let top = std::f32::consts::FRAC_PI_2;
        if let Some(ctx) = seat.and_then(|x| x.ctx) {
            let f = (ctx as f32 / 100.0).clamp(0.0, 1.0);
            arc(&mut g, neck, s * 0.5, top + std::f32::consts::PI, top + std::f32::consts::PI + std::f32::consts::PI * f, lin([1.0, 0.8, 0.3], 1.0));
        }
        for (i, lim) in [seat.and_then(|x| x.five_hour), seat.and_then(|x| x.week)].into_iter().enumerate() {
            if let Some(v) = lim {
                let f = (v as f32 / 100.0).clamp(0.0, 1.0);
                let r = s * (2.0 + 0.35 * i as f32);
                let col = lin(mix3([0.3, 0.9, 0.5], [1.0, 0.3, 0.2], f), 0.6);
                arc(&mut g, anchor, r, top - std::f32::consts::PI * f * 0.5, top + std::f32::consts::PI * f * 0.5, col);
            }
        }
    }
}
