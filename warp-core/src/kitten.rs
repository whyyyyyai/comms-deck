//! Movement and message cues for Lumo's quantum kitten.
//! Written by: codex

use crate::feed::TaskInfo;
use bevy::math::Vec2;
use std::collections::VecDeque;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pose {
    Ready,
    Stuck,
    Busy,
    Away,
    Resting,
    Spent,
    Boxed,
}

pub fn pose(shown: &str) -> Pose {
    match shown {
        "ready" | "idle" => Pose::Ready,
        "stuck" => Pose::Stuck,
        "busy" => Pose::Busy,
        "spent" => Pose::Spent,
        "not set up" => Pose::Boxed,
        label if label.starts_with("idle until ") => Pose::Resting,
        _ => Pose::Away,
    }
}

pub fn hops(pose: Pose) -> bool {
    matches!(pose, Pose::Ready | Pose::Stuck | Pose::Busy)
}

pub fn stable_key(shown: &str, waiting: u64, task: Option<&str>) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in shown
        .bytes()
        .chain([0])
        .chain(waiting.to_le_bytes())
        .chain([u8::from(task.is_some())])
        .chain(task.unwrap_or("").bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

pub struct Tempo {
    pub base_s: f32,
    pub floor: f32,
    pub min_s: f32,
}

pub fn dwell_s(cfg: &Tempo, oldest_s: f64) -> f32 {
    let age_fraction = (oldest_s.max(0.0) / 3600.0).min(1.0) as f32;
    let factor = (1.0 - 0.4 * age_fraction.sqrt()).max(cfg.floor.clamp(0.0, 1.0));
    (cfg.base_s * factor).max(cfg.min_s.max(0.0))
}

fn mix_seed(value: u64) -> u64 {
    let value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    let value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

pub fn candidates(
    anchor: Vec2,
    reach: f32,
    seed: u64,
    index: u64,
    key: u64,
    blocked: &[(Vec2, f32)],
    body_r: f32,
) -> Vec<Vec2> {
    if !reach.is_finite() || reach <= 0.0 {
        return Vec::new();
    }
    let bits = mix_seed(seed ^ mix_seed(index) ^ key);
    let angle = (bits >> 40) as f32 / (1 << 24) as f32 * TAU;
    (0..3)
        .map(|slot| {
            let phase = angle + slot as f32 * TAU / 3.0;
            anchor + Vec2::new(phase.cos(), phase.sin()) * reach
        })
        .filter(|&position| {
            blocked.iter().all(|&(center, radius)| {
                position.distance_squared(center) >= (body_r.max(0.0) + radius.max(0.0)).powi(2)
            })
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hop {
    pub index: u64,
    pub from: Vec2,
    pub to: Vec2,
    pub ghosts: Vec<Vec2>,
    /// Time at which the kitten leaves its current position.
    pub start: f32,
    pub dwell: f32,
}

/// Schedule the next hop after the previous one has landed. Keep this plan
/// through takeoff and landing so incoming data cannot change its destination.
#[allow(clippy::too_many_arguments)]
pub fn next_hop(
    prev: Option<&Hop>,
    now: f32,
    anchor: Vec2,
    reach: f32,
    seed: u64,
    key: u64,
    blocked: &[(Vec2, f32)],
    body_r: f32,
    tempo: &Tempo,
    oldest_s: f64,
) -> Hop {
    let index = prev.map_or(0, |hop| hop.index.wrapping_add(1));
    let from = prev.map_or(anchor, |hop| hop.to);
    let mut ghosts = candidates(anchor, reach, seed, index, key, blocked, body_r);
    let to = if ghosts.is_empty() {
        anchor
    } else {
        ghosts.remove(0)
    };
    let dwell = dwell_s(tempo, oldest_s);
    Hop {
        index,
        from,
        to,
        ghosts,
        start: now + dwell,
        dwell,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Yarn {
    pub balls: u8,
    pub extra: u64,
    pub age_tint: f32,
}

pub fn yarn(waiting: u64, oldest_s: f64, max: u8) -> Yarn {
    let balls = waiting.min(u64::from(max)) as u8;
    Yarn {
        balls,
        extra: waiting - u64::from(balls),
        age_tint: if waiting == 0 {
            0.0
        } else {
            (oldest_s.max(0.0) / 3600.0).min(1.0) as f32
        },
    }
}

pub fn held_task<'a>(tasks: &'a [TaskInfo], seat: &str) -> Option<&'a TaskInfo> {
    tasks
        .iter()
        .filter(|task| !task.open && task.holder.as_deref() == Some(seat))
        .min_by_key(|task| {
            let number = task
                .id
                .strip_prefix('t')
                .and_then(|id| id.parse::<u64>().ok());
            (number.is_none(), number, task.id.as_str())
        })
}

pub fn is_physics(title: Option<&str>, words: &[String]) -> bool {
    title.is_some_and(|title| {
        let title = title.to_lowercase();
        words.iter().any(|word| {
            let word = word.trim();
            !word.is_empty() && title.contains(&word.to_lowercase())
        })
    })
}

pub fn relay_target(to: &[String], me: &str) -> Option<String> {
    to.iter()
        .filter(|seat| !seat.is_empty() && seat.as_str() != me)
        .min()
        .cloned()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Event {
    Catch,
    Relay { to: String },
    Rattle,
    Flash,
}

#[derive(Clone, Copy)]
pub struct Durations {
    pub catch_s: f32,
    pub relay_s: f32,
    pub perch_s: f32,
    pub rattle_s: f32,
}

impl Default for Durations {
    fn default() -> Self {
        Self {
            catch_s: 0.6,
            relay_s: 1.2,
            perch_s: 0.8,
            rattle_s: 0.4,
        }
    }
}

impl Durations {
    fn duration(self, event: &Event) -> f32 {
        match event {
            Event::Catch => self.catch_s,
            Event::Relay { .. } => self.relay_s.max(0.0) + self.perch_s.max(0.0),
            Event::Rattle | Event::Flash => self.rattle_s,
        }
        .max(0.01)
    }
}

const MAX_QUEUED: usize = 8;
const RECENT_IDS: usize = 256;

struct Playing {
    event: Event,
    start: f32,
    duration: f32,
}

pub struct Queue {
    durations: Durations,
    pending: VecDeque<Event>,
    active: Option<Playing>,
    // Startup replay is filtered by the feed. Remember recent live IDs,
    // including overflow drops, so retransmissions cannot create new cues.
    recent: VecDeque<String>,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new(Durations::default())
    }
}

impl Queue {
    pub fn new(durations: Durations) -> Self {
        Self {
            durations,
            pending: VecDeque::new(),
            active: None,
            recent: VecDeque::new(),
        }
    }

    pub fn push(&mut self, id: &str, event: Event, now: f32) -> bool {
        if !id.is_empty() {
            if self.recent.iter().any(|seen| seen == id) {
                return false;
            }
            if self.recent.len() == RECENT_IDS {
                self.recent.pop_front();
            }
            self.recent.push_back(id.to_owned());
        }
        self.advance(now);
        if self.pending.len() == MAX_QUEUED {
            return false;
        }
        self.pending.push_back(event);
        self.advance(now);
        true
    }

    pub fn current(&mut self, now: f32) -> Option<(&Event, f32)> {
        self.advance(now);
        let active = self.active.as_ref()?;
        let progress = ((now - active.start) / active.duration).clamp(0.0, 1.0);
        Some((&active.event, progress))
    }

    /// Inspect the current event without changing the queue or its clock.
    pub fn peek(&self) -> Option<&Event> {
        self.active.as_ref().map(|active| &active.event)
    }

    fn advance(&mut self, now: f32) {
        let mut start = now;
        if let Some(active) = &self.active {
            start = active.start + active.duration;
            if now < start {
                return;
            }
        }
        self.active = None;
        while let Some(event) = self.pending.pop_front() {
            let duration = self.durations.duration(&event);
            if now < start + duration {
                self.active = Some(Playing {
                    event,
                    start,
                    duration,
                });
                break;
            }
            start += duration;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_active_poses_hop() {
        for (shown, expected, moves) in [
            ("ready", Pose::Ready, true),
            ("idle", Pose::Ready, true),
            ("stuck", Pose::Stuck, true),
            ("busy", Pose::Busy, true),
            ("away", Pose::Away, false),
            ("idle until 1h", Pose::Resting, false),
            ("spent", Pose::Spent, false),
            ("not set up", Pose::Boxed, false),
            ("unknown", Pose::Away, false),
        ] {
            assert_eq!(pose(shown), expected);
            assert_eq!(hops(expected), moves);
        }
    }

    #[test]
    fn dwell_is_smooth_and_bounded() {
        let mut tempo = Tempo {
            base_s: 4.0,
            floor: 0.6,
            min_s: 1.5,
        };
        assert_eq!(dwell_s(&tempo, 0.0), 4.0);
        assert!(dwell_s(&tempo, 900.0) < 4.0);
        assert_eq!(dwell_s(&tempo, 7200.0), dwell_s(&tempo, 3600.0));
        tempo.base_s = 2.0;
        assert_eq!(dwell_s(&tempo, 3600.0), 1.5);
    }

    #[test]
    fn hop_targets_are_stable_and_clear_of_other_seats() {
        let anchor = Vec2::new(50.0, 80.0);
        let key = stable_key("ready", 31, Some("t10"));
        assert_eq!(key, stable_key("ready", 31, Some("t10")));
        assert_ne!(key, stable_key("ready", 30, Some("t10")));
        assert_ne!(key, stable_key("busy", 31, Some("t10")));
        assert_ne!(key, stable_key("ready", 31, Some("t11")));
        let spots = candidates(anchor, 20.0, 7, 0, key, &[], 2.0);
        assert_eq!(spots.len(), 3);
        assert_eq!(spots, candidates(anchor, 20.0, 7, 0, key, &[], 2.0));
        assert_ne!(spots, candidates(anchor, 20.0, 7, 1, key, &[], 2.0));
        let blocked = [(spots[0], 5.0)];
        assert_eq!(
            candidates(anchor, 20.0, 7, 0, key, &blocked, 2.0),
            spots[1..]
        );

        let tempo = Tempo {
            base_s: 4.0,
            floor: 0.6,
            min_s: 1.5,
        };
        let hop = next_hop(None, 10.0, anchor, 20.0, 7, key, &blocked, 2.0, &tempo, 0.0);
        assert_eq!(
            (hop.from, hop.to, hop.start, hop.dwell),
            (anchor, spots[1], 14.0, 4.0)
        );
        assert_eq!(hop.ghosts, spots[2..]);
        let next = next_hop(
            Some(&hop),
            15.0,
            anchor,
            20.0,
            7,
            key,
            &[(anchor, 25.0)],
            2.0,
            &tempo,
            0.0,
        );
        assert_eq!((next.index, next.from, next.to), (1, hop.to, anchor));
        assert!(next.ghosts.is_empty());
    }

    #[test]
    fn yarn_counts_overflow_and_clamps_age() {
        assert_eq!(
            yarn(31, 7200.0, 3),
            Yarn {
                balls: 3,
                extra: 28,
                age_tint: 1.0
            }
        );
        assert_eq!(
            yarn(2, 1800.0, 3),
            Yarn {
                balls: 2,
                extra: 0,
                age_tint: 0.5
            }
        );
        assert_eq!(
            yarn(0, 900.0, 3),
            Yarn {
                balls: 0,
                extra: 0,
                age_tint: 0.0
            }
        );
    }

    #[test]
    fn selects_lowest_held_task_and_matches_only_known_titles() {
        let task = |id: &str, holder: &str, open| TaskInfo {
            id: id.to_owned(),
            holder: Some(holder.to_owned()),
            open,
            ..Default::default()
        };
        let tasks = [
            task("t10", "lumo", false),
            task("t1", "grok", false),
            task("t3", "lumo", false),
            task("t2", "lumo", true),
        ];
        assert_eq!(held_task(&tasks, "lumo").unwrap().id, "t3");
        assert!(held_task(&tasks, "codex").is_none());
        let words = ["", "Orbit", "gravity"].map(str::to_owned);
        assert!(is_physics(Some("Improve ORBIT stability"), &words));
        assert!(!is_physics(Some("Review mail layout"), &words));
        assert!(!is_physics(None, &words));
    }

    #[test]
    fn relay_selects_one_other_seat_without_changing_recipient_order() {
        let to = ["lumo", "vibe", "claude"].map(str::to_owned);
        assert_eq!(relay_target(&to, "lumo").as_deref(), Some("claude"));
        assert_eq!(relay_target(&to[..1], "lumo"), None);
    }

    #[test]
    fn queue_deduplicates_and_keeps_fifo_timing() {
        let mut queue = Queue::new(Durations {
            catch_s: 1.0,
            relay_s: 1.0,
            perch_s: 1.0,
            rattle_s: 1.0,
        });
        let relay = Event::Relay {
            to: "claude".into(),
        };
        assert!(queue.push("first", Event::Catch, 10.0));
        assert!(!queue.push("first", Event::Flash, 10.0));
        assert!(queue.push("second", relay.clone(), 10.1));
        assert_eq!(queue.current(10.5), Some((&Event::Catch, 0.5)));
        assert_eq!(queue.peek(), Some(&Event::Catch));
        assert_eq!(queue.current(12.0), Some((&relay, 0.5)));
        assert_eq!(queue.peek(), Some(&relay));
        assert_eq!(queue.current(13.0), None);
        assert_eq!(queue.peek(), None);
        assert!(!queue.push("first", Event::Rattle, 13.0));
    }

    #[test]
    fn queue_caps_waiting_events_and_drops_expired_cues() {
        let mut queue = Queue::default();
        assert!(queue.push("active", Event::Flash, 0.0));
        for i in 0..MAX_QUEUED {
            assert!(queue.push(&i.to_string(), Event::Rattle, 0.0));
        }
        assert!(!queue.push("overflow", Event::Rattle, 0.0));
        assert_eq!(queue.current(20.0), None);
        assert!(!queue.push("overflow", Event::Rattle, 20.0));
        assert!(queue.push("new", Event::Catch, 20.0));
        assert_eq!(queue.current(20.0), Some((&Event::Catch, 0.0)));
    }
}
