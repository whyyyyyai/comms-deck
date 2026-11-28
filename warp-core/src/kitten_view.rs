//! Lumo's seat as a quantum electron (t78). Y's brief: "Lumo's job will be
//! to improve the physics engine of the warp core ... a quantum-kitten
//! helper ... that jumps around the warp core in a semi random semi
//! deterministic way". Then, after seeing the cat (00:55): "it should be an
//! electron, and the jumps should be between orbiting different agents, and
//! it should orbit agents that are close together in both of their orbits
//! like a figure 8 or even a trinary orbit."
//!
//! So Lumo has no planet. Her electron orbits the agents' planets: one
//! planet, or a figure 8 round two that are close together, or three lobes
//! round three. Every few seconds she quantum-jumps to another planet or
//! group, chosen by a seeded hash of a stable state, so the same state
//! gives the same jumps. She is never in the gravity sim: the physics she is
//! there to improve stays clean. The logic for states, tempo, waiting and
//! the event queue is `kitten` (Codex); the knobs are `[kitten]` (Devin).
//!
//! Written by: claude

use super::*;
use crate::kitten::{self, Event, Pose};

pub const SEAT: &str = "lumo";
/// Planets closer than this (world px) can be orbited together.
const CLOSE: f32 = 260.0;
/// One lobe of an orbit takes this long, seconds (busy goes faster).
const LOBE_S: f32 = 2.6;
/// Share of a lobe spent looping round its planet; the rest crosses over.
const LOOP: f32 = 0.8;
/// Trail points kept behind the electron.
const TRAIL: usize = 28;

#[derive(Resource)]
pub struct KittenView {
    /// Lumo's empty ring slot this frame (world), if she's on the roster.
    pub anchor: Option<Vec2>,
    /// Where the electron is this frame (world): comets to Lumo land here.
    pub pos: Vec2,
    pub pose: Pose,
    /// The planets she is orbiting now (empty = her own slot, ground state).
    hosts: Vec<String>,
    /// The group she will jump to, frozen once the ghost window opens.
    next: Option<Vec<String>>,
    /// The other groups she might have jumped to, shown as ghosts.
    maybe: Vec<Vec<String>>,
    phase: f32,
    jump_at: f32,
    jump_index: u64,
    /// The last jump: when, and where she left from (for the flashes).
    jumped: Option<(f32, Vec2)>,
    trail: VecDeque<Vec2>,
    queue: kitten::Queue,
    seed: u64,
    last_waiting: Option<u64>,
    fps: f32,
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
            hosts: Vec::new(),
            next: None,
            maybe: Vec::new(),
            phase: 0.0,
            jump_at: 0.0,
            jump_index: 0,
            jumped: None,
            trail: VecDeque::new(),
            queue: kitten::Queue::default(),
            seed,
            last_waiting: None,
            fps: 60.0,
            seed_logged: false,
        }
    }
}

impl KittenView {
    /// A note from Lumo has launched: she jumps to orbit its recipient.
    pub fn sent(&mut self, note_id: &str, to: &[String], now: f32) {
        if let Some(target) = kitten::relay_target(to, SEAT) {
            let ev = if kitten::hops(self.pose) { Event::Relay { to: target } } else { Event::Flash };
            self.queue.push(&format!("r:{note_id}"), ev, now);
        }
    }
    /// A note to Lumo has landed on her: she absorbs it.
    pub fn landed(&mut self, note_id: &str, now: f32) {
        let ev = if self.pose == Pose::Boxed { Event::Rattle } else { Event::Catch };
        self.queue.push(&format!("c:{note_id}"), ev, now);
    }
}

pub fn on(mods: &Mods) -> bool {
    mods.cfg.kitten.enable
}

/// Her name, tag and "+N" at her slot.
#[derive(Component)]
pub struct KittenText;

/// The groups of planets she can orbit: every planet alone, every pair
/// closer than `close`, every triple whose members are all that close.
/// Sorted, so the choice from them is stable.
pub fn groups(planets: &[(String, Vec2)], close: f32) -> Vec<Vec<String>> {
    let mut ps: Vec<&(String, Vec2)> = planets.iter().collect();
    ps.sort_by(|a, b| a.0.cmp(&b.0));
    let near = |a: &Vec2, b: &Vec2| a.distance(*b) < close;
    let mut out = Vec::new();
    for i in 0..ps.len() {
        out.push(vec![ps[i].0.clone()]);
        for j in (i + 1)..ps.len() {
            if !near(&ps[i].1, &ps[j].1) {
                continue;
            }
            out.push(vec![ps[i].0.clone(), ps[j].0.clone()]);
            for k in (j + 1)..ps.len() {
                if near(&ps[i].1, &ps[k].1) && near(&ps[j].1, &ps[k].1) {
                    out.push(vec![ps[i].0.clone(), ps[j].0.clone(), ps[k].0.clone()]);
                }
            }
        }
    }
    out
}

fn mix(mut v: u64) -> u64 {
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    v ^ (v >> 31)
}

/// Pick the next group: weighted to the shared orbits (a triple counts 3,
/// a pair 2, one planet 1), never the group she is on if there's another,
/// by a hash of the seed, the jump number and the stable state.
/// Returns the pick and up to two runners-up for the ghosts.
pub fn pick(all: &[Vec<String>], current: &[String], seed: u64, index: u64, key: u64) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let pool: Vec<&Vec<String>> = {
        let other: Vec<&Vec<String>> = all.iter().filter(|g| g.as_slice() != current).collect();
        if other.is_empty() { all.iter().collect() } else { other }
    };
    if pool.is_empty() {
        return None;
    }
    let weight = |g: &Vec<String>| g.len() as u64;
    let total: u64 = pool.iter().map(|g| weight(g)).sum();
    let mut roll = mix(seed ^ mix(index) ^ key.rotate_left(17)) % total.max(1);
    let mut chosen = 0;
    for (i, g) in pool.iter().enumerate() {
        if roll < weight(g) {
            chosen = i;
            break;
        }
        roll -= weight(g);
    }
    let others: Vec<Vec<String>> = (1..pool.len()).map(|d| pool[(chosen + d * 7) % pool.len()].clone()).filter(|g| g != pool[chosen]).take(2).collect();
    Some((pool[chosen].clone(), others))
}

/// Where the electron is at `phase` on an orbit round `hosts` (positions),
/// with loop radius `r`. One planet: a circle. Two: a figure 8, one loop
/// each way round each planet, crossing between them. Three: three lobes,
/// all the same way round, crossing through the middle. Each lobe starts
/// and ends on the side of its planet facing the group's centre, so close
/// planets join up smoothly.
pub fn orbit_at(hosts: &[Vec2], phase: f32, r: f32) -> Vec2 {
    let n = hosts.len();
    if n == 0 {
        return Vec2::ZERO;
    }
    if n == 1 {
        return hosts[0] + Vec2::from_angle(phase * std::f32::consts::TAU) * r;
    }
    let centre = hosts.iter().copied().sum::<Vec2>() / n as f32;
    let lobe = (phase.rem_euclid(n as f32)).floor() as usize % n;
    let f = phase.rem_euclid(1.0);
    let face = |i: usize| (centre - hosts[i]).normalize_or(Vec2::X);
    let sign = if n == 2 && lobe == 1 { -1.0 } else { 1.0 };
    if f < LOOP {
        let a = face(lobe).to_angle() + sign * std::f32::consts::TAU * (f / LOOP);
        hosts[lobe] + Vec2::from_angle(a) * r
    } else {
        let j = (lobe + 1) % n;
        let a = hosts[lobe] + face(lobe) * r;
        let b = hosts[j] + face(j) * r;
        bezier(a, centre, b, (f - LOOP) / (1.0 - LOOP))
    }
}

/// The loop radius for a group: half the gap between close planets, so a
/// figure 8 crosses in the middle, within sensible bounds.
fn loop_radius(hosts: &[Vec2]) -> f32 {
    if hosts.len() < 2 {
        return 44.0;
    }
    let c = hosts.iter().copied().sum::<Vec2>() / hosts.len() as f32;
    let d = hosts.iter().map(|h| h.distance(c)).fold(0.0f32, f32::max);
    (d * 0.9).clamp(38.0, 90.0)
}

fn positions_of(names: &[String], planets: &HashMap<String, Vec2>) -> Option<Vec<Vec2>> {
    names.iter().map(|n| planets.get(n).copied()).collect()
}

/// Each frame: read Lumo's seat, keep the jump schedule, move the electron.
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
            TextFont { font_size: FontSize::Px(15.0), ..default() },
            TextColor(seat_colour(SEAT, 0.85)),
            Anchor::TOP_CENTER,
            Transform::from_xyz(0.0, 0.0, 4.0),
            Visibility::Hidden,
        ));
        *spawned = true;
    }
    kit.queue.set_durations(kitten::Durations { catch_s: cfg.catch_s, relay_s: cfg.relay_s, perch_s: cfg.perch_s, rattle_s: cfg.rattle_s });
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
        println!("warp-core electron: session seed {:016x}", kit.seed);
        kit.seed_logged = true;
    }
    let seat = deck.seat(SEAT);
    let shown = seat.map(|s| s.shown.as_str()).unwrap_or("not set up");
    kit.pose = kitten::pose(shown);
    let waiting = seat.map(|s| s.waiting).unwrap_or(0);
    let oldest = seat.map(|s| s.oldest_waiting_s).unwrap_or(0.0);
    if let Some(before) = kit.last_waiting {
        if waiting > before && kit.pose == Pose::Boxed {
            let key = format!("w:{waiting}:{}", (now / 2.0) as u64);
            kit.queue.push(&key, Event::Rattle, now);
        }
    }
    kit.last_waiting = Some(waiting);

    let planets: HashMap<String, Vec2> = nodes.iter().map(|(n, tf)| (n.name.clone(), tf.translation.truncate())).collect();
    let task = kitten::held_task(&deck.tasks, SEAT).map(|t| t.id.clone());
    let key = kitten::stable_key(shown, waiting, task.as_deref());
    let tempo = kitten::Tempo { base_s: cfg.base_dwell_s, floor: cfg.dwell_floor, min_s: cfg.dwell_min_s };
    let relay_to = match kit.queue.peek() {
        Some(Event::Relay { to }) if planets.contains_key(to) => Some(to.clone()),
        _ => None,
    };

    let excited = kit.pose == Pose::Busy;
    let speed = if excited { 1.6 } else { 1.0 };
    kit.phase += dt * speed / LOBE_S;
    let jumping = kitten::hops(kit.pose) && !planets.is_empty();
    let before = kit.pos;
    if let Some(to) = relay_to {
        // Relaying: she jumps straight onto the recipient's orbit.
        if kit.hosts != [to.clone()] {
            kit.jumped = Some((now, before));
            kit.hosts = vec![to];
            kit.next = None;
            kit.jump_at = now + cfg.relay_s + cfg.perch_s;
        }
    } else if jumping {
        let list: Vec<(String, Vec2)> = planets.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let all = groups(&list, CLOSE);
        let lost = positions_of(&kit.hosts, &planets).is_none() || kit.hosts.is_empty();
        // Freeze the next group when the ghost window opens.
        if kit.next.is_none() && (lost || now >= kit.jump_at - cfg.ghost_s) {
            let index = kit.jump_index;
            let hosts = kit.hosts.clone();
            if let Some((g, others)) = pick(&all, &hosts, kit.seed, index, key) {
                kit.next = Some(g);
                kit.maybe = others;
            }
        }
        if lost || now >= kit.jump_at {
            if let Some(g) = kit.next.take() {
                kit.jumped = Some((now, before));
                kit.hosts = g;
                kit.jump_index += 1;
                kit.jump_at = now + kitten::dwell_s(&tempo, oldest);
                kit.maybe.clear();
            }
        }
    } else if !kit.hosts.is_empty() {
        kit.jumped = Some((now, before));
        kit.hosts.clear();
        kit.next = None;
    }
    let pos = match positions_of(&kit.hosts, &planets) {
        Some(hp) if !hp.is_empty() => {
            let mut r = loop_radius(&hp) * if excited { 1.25 } else { 1.0 };
            if let Some((Event::Catch, k)) = kit.queue.current(now).map(|(e, k)| (e.clone(), k)) {
                r *= 1.0 + 0.6 * (1.0 - k); // absorbed: a wider orbit, decaying back
            }
            orbit_at(&hp, kit.phase, r)
        }
        // Ground state: a small slow orbit at her own empty slot.
        _ => anchor + Vec2::from_angle(kit.phase * 0.5 * std::f32::consts::TAU) * 18.0,
    };
    kit.pos = pos;
    kit.trail.push_front(pos);
    kit.trail.truncate(TRAIL);
    if kit.jumped.is_some_and(|(t, _)| t == now) {
        kit.trail.truncate(1); // a quantum jump leaves no trail across the gap
    }

    let yarn = kitten::yarn(waiting, oldest, cfg.yarn_max);
    let mut words = Vec::new();
    if let Some(t) = &task {
        words.push(t.clone());
    }
    if waiting > 0 {
        words.push(format!("+{}", yarn.extra + yarn.balls as u64));
    }
    for (mut t, mut tf, mut vis) in &mut text {
        let line = if words.is_empty() { label_for(SEAT) } else { format!("{}\n{}", label_for(SEAT), words.join("  ")) };
        if t.0 != line {
            t.0 = line;
        }
        tf.translation.x = anchor.x;
        tf.translation.y = anchor.y - 22.0;
        *vis = Visibility::Inherited;
    }
}

fn arc(g: &mut Gizmos, centre: Vec2, r: f32, a0: f32, a1: f32, c: Color) {
    let steps = (((a1 - a0).abs() / 0.2).ceil() as usize).clamp(3, 40);
    let pts: Vec<Vec2> = (0..=steps).map(|i| centre + Vec2::from_angle(a0 + (a1 - a0) * i as f32 / steps as f32) * r).collect();
    for w in pts.windows(2) {
        g.line_2d(w[0], w[1], c);
    }
}

/// The electron itself: a bright core and a soft glow, its size from context.
fn electron(g: &mut Gizmos, p: Vec2, glow: f32, c: [f32; 3], bright: f32, hollow: bool) {
    if hollow {
        g.circle_2d(Isometry2d::from_translation(p), 6.0, lin(c, 0.7 * bright));
        return;
    }
    for i in 0..4 {
        g.circle_2d(Isometry2d::from_translation(p), 1.5 + i as f32 * 1.4, lin(c, 2.4 * bright));
    }
    g.circle_2d(Isometry2d::from_translation(p), glow, lin(c, 0.9 * bright).with_alpha(0.5));
    g.circle_2d(Isometry2d::from_translation(p), glow * 1.6, lin(c, 0.5 * bright).with_alpha(0.25));
}

/// Draw her: trail, charges, ghosts, the jump flashes, absorption, limits.
pub fn draw_kitten(mut kit: ResMut<KittenView>, deck: Res<Deck>, mods: Res<Mods>, time: Res<Time>, nodes: Query<(&SeatNode, &Transform)>, mut g: Gizmos) {
    let cfg = &mods.cfg.kitten;
    let Some(anchor) = kit.anchor else { return };
    let now = time.elapsed_secs();
    let base = seat_rgb(SEAT);
    let seat = deck.seat(SEAT);
    let pose = kit.pose;
    let planets: HashMap<String, Vec2> = nodes.iter().map(|(n, tf)| (n.name.clone(), tf.translation.truncate())).collect();

    // Her empty slot: a dim dashed circle, where her name hangs.
    for i in 0..12 {
        let a0 = i as f32 * std::f32::consts::TAU / 12.0;
        arc(&mut g, anchor, 12.0, a0, a0 + std::f32::consts::TAU / 24.0, lin(base, 0.5).with_alpha(0.3));
    }

    let (colour, bright, hollow) = match pose {
        Pose::Boxed => (base, 0.5, true),
        Pose::Spent => ([0.55, 0.55, 0.6], 0.5, false),
        Pose::Away | Pose::Resting => (base, 0.45, false),
        Pose::Stuck => (base, if (now * 9.0).sin() > 0.2 { 1.0 } else { 0.35 }, false),
        Pose::Busy => (base, 1.3, false),
        Pose::Ready => (base, 1.0, false),
    };
    let ctx = seat.and_then(|x| x.ctx).map(|c| (c as f32 / 100.0).clamp(0.0, 1.0)).unwrap_or(0.3);
    let glow = 6.0 + 8.0 * ctx;

    // On a physics task, her whole orbit is drawn faintly: her trajectory work.
    let physics = kitten::held_task(&deck.tasks, SEAT).is_some_and(|t| kitten::is_physics(t.title.as_deref(), &cfg.physics_words));
    if physics {
        if let Some(hp) = positions_of(&kit.hosts, &planets).filter(|h| !h.is_empty()) {
            let r = loop_radius(&hp);
            let n = hp.len() as f32;
            let pts: Vec<Vec2> = (0..=90).map(|i| orbit_at(&hp, n * i as f32 / 90.0, r)).collect();
            for w in pts.windows(2) {
                g.line_2d(w[0], w[1], lin(colour, 0.6).with_alpha(0.35));
            }
        }
    }

    // The trail, fading.
    let n = kit.trail.len().max(1) as f32;
    for (i, w) in kit.trail.iter().zip(kit.trail.iter().skip(1)).enumerate() {
        let fade = 1.0 - i as f32 / n;
        g.line_2d(*w.0, *w.1, lin(colour, 1.4 * bright * fade).with_alpha(fade));
    }

    // Ghosts: in the window before a jump, faint electrons on the orbits she
    // might take, at the phase she'd be at. Then she collapses into one.
    let until = kit.jump_at - now;
    if until > 0.0 && until <= cfg.ghost_s && mods::ghosts_allowed(kit.fps, cfg) {
        let a = (cfg.ghost_alpha * (1.0 - until / cfg.ghost_s.max(1e-3)) * 1.6).min(1.0);
        let mut options: Vec<Vec<String>> = kit.maybe.clone();
        if let Some(nx) = &kit.next {
            options.push(nx.clone());
        }
        for o in &options {
            if let Some(hp) = positions_of(o, &planets).filter(|h| !h.is_empty()) {
                let p = orbit_at(&hp, kit.phase, loop_radius(&hp));
                g.circle_2d(Isometry2d::from_translation(p), glow, lin(colour, 1.5).with_alpha(a));
                g.circle_2d(Isometry2d::from_translation(p), 3.0, lin(colour, 2.0).with_alpha(a));
            }
        }
    }

    // A quantum jump: a spark where she left, a ring where she arrived.
    if let Some((t, from)) = kit.jumped {
        let k = (now - t) / 0.45;
        if k < 1.0 {
            for i in 0..6 {
                let ang = i as f32 * std::f32::consts::TAU / 6.0;
                g.line_2d(from, from + Vec2::from_angle(ang) * (6.0 + 18.0 * k), lin(colour, 1.6 * (1.0 - k)));
            }
            g.circle_2d(Isometry2d::from_translation(kit.pos), 6.0 + 22.0 * k, lin(colour, 1.8 * (1.0 - k)));
        }
    }

    // The queue's beat (read by kitten_step this frame): absorbing a note,
    // the rattle of a "not set up" seat, or a flash at the slot.
    let beat = kit.queue.current(now).map(|(e, k)| (e.clone(), k));
    match beat {
        Some((Event::Catch, k)) => {
            // Absorbed: a burst of light on her, then a photon ring given off
            // as she decays back to her orbit.
            if k < 0.35 {
                let f = 1.0 - k / 0.35;
                g.circle_2d(Isometry2d::from_translation(kit.pos), glow * (1.0 + 2.5 * (1.0 - f)), lin([1.0, 1.0, 1.0], 2.5 * f));
                for i in 0..10 {
                    let ang = i as f32 * std::f32::consts::TAU / 10.0;
                    let p0 = kit.pos + Vec2::from_angle(ang) * (glow + 4.0);
                    g.line_2d(p0, p0 + Vec2::from_angle(ang) * 16.0 * f, lin(colour, 2.2 * f));
                }
            } else {
                let f = (k - 0.35) / 0.65;
                g.circle_2d(Isometry2d::from_translation(kit.pos), glow + 70.0 * f, lin(colour, 1.6 * (1.0 - f)).with_alpha(1.0 - f));
            }
        }
        Some((Event::Rattle, k)) => {
            g.circle_2d(Isometry2d::from_translation(anchor), 12.0 + 10.0 * (k * 30.0).sin().abs(), lin(base, 1.2 * (1.0 - k)));
        }
        Some((Event::Flash, k)) => {
            g.circle_2d(Isometry2d::from_translation(anchor), 14.0 + 30.0 * k, lin(base, 1.5 * (1.0 - k)));
        }
        _ => {}
    }

    // The electron.
    electron(&mut g, kit.pos, glow, colour, bright, hollow);

    // Waiting notes: up to `yarn_max` charge dots trailing behind her, the
    // first one tinted by the oldest note's age (blue fresh, red old).
    let waiting = seat.map(|x| x.waiting).unwrap_or(0);
    let oldest = seat.map(|x| x.oldest_waiting_s).unwrap_or(0.0);
    let yarn = kitten::yarn(waiting, oldest, cfg.yarn_max);
    for i in 0..yarn.balls as usize {
        let at = (i + 1) * 5;
        if let Some(p) = kit.trail.get(at.min(kit.trail.len().saturating_sub(1))) {
            let col = if i == 0 { mix3([0.4, 0.7, 1.0], [1.0, 0.25, 0.2], yarn.age_tint) } else { colour };
            g.circle_2d(Isometry2d::from_translation(*p), 2.5, lin(col, 1.6));
        }
    }

    // Two thin arcs over her slot: the five-hour and week limits.
    let top = std::f32::consts::FRAC_PI_2;
    for (i, lim) in [seat.and_then(|x| x.five_hour), seat.and_then(|x| x.week)].into_iter().enumerate() {
        if let Some(v) = lim {
            let f = (v as f32 / 100.0).clamp(0.0, 1.0);
            let col = lin(mix3([0.3, 0.9, 0.5], [1.0, 0.3, 0.2], f), 0.7);
            arc(&mut g, anchor, 18.0 + 5.0 * i as f32, top - std::f32::consts::PI * f * 0.5, top + std::f32::consts::PI * f * 0.5, col);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(v: &[(&str, f32, f32)]) -> Vec<(String, Vec2)> {
        v.iter().map(|(n, x, y)| (n.to_string(), Vec2::new(*x, *y))).collect()
    }

    #[test]
    fn groups_are_singles_close_pairs_and_close_triples() {
        let g = groups(&named(&[("a", 0.0, 0.0), ("b", 100.0, 0.0), ("c", 50.0, 80.0), ("d", 900.0, 0.0)]), 260.0);
        assert!(g.contains(&vec!["a".into()]) && g.contains(&vec!["d".into()]));
        assert!(g.contains(&vec!["a".into(), "b".into()]));
        assert!(g.contains(&vec!["a".into(), "b".into(), "c".into()]));
        assert!(!g.iter().any(|x| x.len() > 1 && x.contains(&"d".to_string())), "far planets are never shared");
    }

    #[test]
    fn pick_is_deterministic_and_leaves_the_current_group() {
        let all = vec![vec!["a".to_string()], vec!["b".to_string()], vec!["a".to_string(), "b".to_string()]];
        let one = pick(&all, &["a".to_string()], 7, 3, 11).unwrap();
        assert_eq!(one, pick(&all, &["a".to_string()], 7, 3, 11).unwrap(), "same state, same jump");
        assert_ne!(one.0, vec!["a".to_string()]);
        assert!(pick(&[], &[], 1, 1, 1).is_none());
        // With only the current group there is nowhere else: she stays.
        assert_eq!(pick(&[vec!["a".to_string()]], &["a".to_string()], 1, 1, 1).unwrap().0, vec!["a".to_string()]);
    }

    #[test]
    fn a_figure_eight_loops_each_planet_and_crosses_in_the_middle() {
        let h = [Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0)];
        let r = 54.0;
        // Mid-loop she is round the first planet, then round the second.
        assert!(orbit_at(&h, 0.4, r).distance(h[0]) < r + 0.1);
        assert!(orbit_at(&h, 1.4, r).distance(h[1]) < r + 0.1);
        // Each loop starts on the side facing the other planet.
        assert!(orbit_at(&h, 0.0, r).distance(Vec2::new(-6.0, 0.0)) < 0.5);
        // The two loops turn opposite ways (one anticlockwise, one
        // clockwise): a smooth figure 8, not two circles touching at a cusp.
        let turn = |i: usize, f: f32| {
            let p0 = orbit_at(&h, f, r) - h[i];
            let p1 = orbit_at(&h, f + 0.01, r) - h[i];
            p0.perp_dot(p1)
        };
        assert!(turn(0, 0.3) * turn(1, 1.3) < 0.0);
        // And she leaves the crossing heading the same way she arrived.
        let arrive = orbit_at(&h, 0.999, r) - orbit_at(&h, 0.99, r);
        let leave = orbit_at(&h, 1.01, r) - orbit_at(&h, 1.0, r);
        assert!(arrive.dot(leave) > 0.0);
        // Continuous across the crossing.
        assert!(orbit_at(&h, 0.999, r).distance(orbit_at(&h, 1.0, r)) < 3.0);
    }

    #[test]
    fn a_trinary_orbit_visits_all_three() {
        let h = [Vec2::new(0.0, 80.0), Vec2::new(-70.0, -40.0), Vec2::new(70.0, -40.0)];
        for (i, hp) in h.iter().enumerate() {
            assert!(orbit_at(&h, i as f32 + 0.4, 50.0).distance(*hp) < 50.1);
        }
        assert!(orbit_at(&h, 2.999, 50.0).distance(orbit_at(&h, 3.0, 50.0)) < 3.0, "wraps smoothly");
    }
}
