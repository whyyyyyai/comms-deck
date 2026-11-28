# Quantum Kitten: Lumo's seat as a helper, drawn in gizmo lines

**Author:** vibe · **Date:** 28 Nov 2026 · **Status:** rev 1, t78 round 5 (spec locked in round 4)
**Covers:** `warp-core/docs/quantum-kitten.md` only — art direction for `claude/quantum-kitten`. No code in this file.

## Frame and primitives

All recipes are relative to `size` (`[kitten].size`, default 14.0 world units). Write
`k = size`. Origin is the kitten's centre, +y up, +x right. Every primitive is one of:

- `gizmos.line_2d(a, b, colour)` — straight segment
- `gizmos.linestrip_gradient_2d(pts.map(|p| (p, colour)))` — polyline, per-point colour
- `gizmos.circle_2d(Isometry2d::from_translation(p), r, colour)` — filled disc
- `gizmos.rect_2d(Isometry2d::new(p, Rot2::radians(a)), Vec2::splat(w), colour)` — square
- text via the existing `Text2d` + `TextFont { font_size: FontSize::Px(..) }` pipeline used for comet labels
- `lin([r,g,b], alpha)` for colours; `seat_colour("lumo", bright)` for her tint

An arc from angle `a0` to `a1` on radius `r` centred at `c` is a linestrip of 24 points:
`c + r * Vec2::new((a0 + (a1-a0)*i/24).cos(), (a0 + (a1-a0)*i/24).sin())`.

**Anchor marker** (always drawn, dim, alpha 0.25, seat colour): dashed circle at the
ring slot, radius `0.9 k`. The label, hover card and note routing hang off this point;
the kitten may leave it, the anchor never moves.

## Poses (persistent, from `shown`)

Shared body parts — body disc, head disc, two ear polylines — vary per pose:

| Pose | `shown` | Recipe |
| --- | --- | --- |
| Ready | `ready` | body circle `(0,-0.15k)` r `0.45k`; head `(0,0.42k)` r `0.32k`; ears: polylines `(−0.22k,0.62k)→(−0.12k,0.85k)→(−0.02k,0.62k)` and mirrored; tail: linestrip `(0.38k,−0.35k) → (0.55k,0.1k) → (0.5k,0.75k)` — up |
| Stuck | `stuck` | as Ready, but the tail tip oscillates: `x += 0.08k * sin(t*6Hz)` |
| Busy | `busy` | body lower `(0,−0.28k)` r `0.5k` (loaf); head `(0.25k,0.18k)` r `0.3k`; paw: line `(0.05k,−0.05k) → (0.45k,0.25k)` alternating lift `y += 0.12k*|sin(t*3Hz)|`; plus orbit preview if physics task (below) |
| Away | `away` | curled: one circle `(0,0)` r `0.55k`; tail wrapped: arc r `0.55k` from 200° to 340°; no ears |
| Resting | `idle until …` | as Away, plus clock: circle `(0,0.85k)` r `0.22k` outline, hands `line((0,0.85k),(0.12k,0.95k))` and `line((0,0.85k),(0,0.73k))`; until-time on the hover card only |
| Spent | `spent` | as Away, plus three small arcs rising right of the body (`(0.7k,0.3k)`, r `0.1k`, alpha decaying 0.6→0), and a static dim cooldown arc: arc r `0.8k` at `(−0.6k,0)` from 180°..270°, alpha 0.3 (no progress — `spent_until` is not in the feed) |
| Boxed | `not set up` | `rect_2d` centred `(0,0)` w `0.9k` (so height `0.9k`), seat-colour outline alpha 0.8; lid line `line((−0.45k,0.28k),(0.45k,0.28k))`; "?" as text at `(0,−0.05k)`, 16 px; latch: small rect at `(0.45k,0.28k)` w `0.12k` |

Rattle (Boxed + `waiting_normal` rises): whole box offset `x += 0.05k * sin(t * 40Hz)` for
`rattle_s` = 0.4 s, then back.

Seat colour tints the outline of body/head in every pose; `bright` dims to 0.25 when
another seat is focused, same as planets.

## Hops and ghosts

Hop path: `linestrip_gradient_2d` from `from` to `to` with alpha fading 0.5→0, plus the
landing pose drawn at `to`. **Ghosts**: in the `ghost_s` = 0.5 s before a hop, draw the
body+head discs only (no ears/tail) at the other two candidate offsets, alpha
`ghost_alpha` = 0.25. Off entirely when `!ghosts_allowed(fps, cfg)` (fps <
`ghost_min_fps` = 30). No hops while Boxed, Spent or Away.

## Yarn (waiting notes)

Up to `yarn_max` = 3 balls at `(-0.7k,-0.55k)`, `(0,-0.68k)`, `(0.7k,-0.55k)`, r `0.16k`
each; if waiting > 3, text `"+N"` at `(0,-0.95k)`, 13 px. The ball in slot 0 is tinted by
age: `mix3(fresh, old, clamp(oldest_waiting_s / 3600, 0, 1))` — same gradient the moons use.
Shown in every pose including Boxed (balls sit in front of the box).

## Collar and limit arcs

- **Collar (ctx)**: arc r `0.55k` centred at head `(0,0.42k)` from 150° to 390° (= 150°..30°
  through the front), background full arc alpha 0.15, fill arc to `ctx` fraction in seat
  colour. Omit the fill if ctx unknown.
- **Limits**: two thin arcs outside the collar, centred at kitten origin: inner r `0.66k`
  = five_hour, outer r `0.76k` = week; fill fraction = meter, colour amber when above
  0.8, red at 1.0. Arcs sweep from -90° clockwise.

## Events

- **Catch** (note to lumo lands): two paw lines converging on the landing point +
  sparkle: 4 lines radiating 0.12k from the point, alpha 1→0 over `catch_s` = 0.6 s.
  If Boxed: rattle instead (above).
- **Relay** (note from lumo): the real packet flies at once on its normal path; the kitten
  leaps after it carrying a glowing ball (circle r `0.15k` at mouth `(0.38k,0.25k)`, bright),
  **perches** outside the recipient's rim for `perch_s` = 0.8 s drawn at 0.7× scale, then
  rejoins the hop schedule (no snap home). Chase target = first non-lumo recipient, sorted.
  Boxed/Spent: no leap, one flash disc at the anchor r `0.5k`, alpha 0.6→0 over 0.3 s.

## Orbit preview (physics role)

While Busy and the held task matches `physics_words` (`physic|orbit|gravity|warp|solver`,
configurable; untitled/private never match): ellipse centred on the kitten, rx `1.6k`,
ry `0.9k`, alpha 0.2, plus one dot r `0.1k` travelling the ellipse at 0.5 Hz. Purely
visual. The paw-print flash on a warp-core physics commit is v2 (no commit data in the
feed yet).

## Task tag

Text `tNN` (lowest held id) via Text2d at `(0,-1.15k)`, 12 px, seat colour, always in every
pose where she can hold a task.

---

Written by: vibe (2026-11-28), t78 round 5.
