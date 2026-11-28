//! The right panel: F2 ACTIVITY and F3 NOTIFY (Y, 02:50: "you should still
//! put F2 and F3 on the right"). F1 COMMS and the line you type stay in the
//! bottom strip of the central column (Devin's t91). Mirrors the left panel:
//! same background, a header with the two tabs, the newest rows that fit.
//! `[chat] side = false` folds both tabs back into the strip.
//!
//! Written by: claude

use super::*;

/// The panel's width for a window, or 0 when it's off.
pub fn right_width(window_w: f32, cfg: &mods::ChatCfg) -> f32 {
    if !cfg.show || !cfg.side {
        return 0.0;
    }
    // Same bounds as the left panel (t99): both panels share one size.
    (window_w * cfg.width).clamp(260.0, 520.0).min(window_w * 0.4)
}

#[derive(Component)]
pub struct RightBg;
#[derive(Component)]
pub struct RightHeader;
#[derive(Component)]
pub struct RightLog;

pub fn header_text(side: Tab) -> String {
    [Tab::Activity, Tab::Notify]
        .iter()
        .map(|t| {
            let k = if *t == Tab::Activity { 2 } else { 3 };
            if *t == side { format!("[F{k} {}]", t.name()) } else { format!(" F{k} {} ", t.name()) }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn right_panel(
    mut commands: Commands,
    mut chat: ResMut<Chat>,
    layout: Res<Layout>,
    mods: Res<Mods>,
    assets: Res<Assets2d>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    real: Res<Time<Real>>,
    mut bg: Query<(&mut Transform, &mut Visibility), (With<RightBg>, Without<RightHeader>, Without<RightLog>)>,
    mut header: Query<(&mut Text2d, &mut Transform, &mut Visibility), (With<RightHeader>, Without<RightBg>, Without<RightLog>)>,
    mut log: Query<(Entity, &mut Transform, &mut TextBounds, &mut Visibility), (With<RightLog>, Without<RightBg>, Without<RightHeader>)>,
    mut state: Local<(bool, f32, usize, usize)>,
) {
    let font = mods.cfg.chat.font;
    if !state.0 {
        let panel_bg = materials.add(Color::srgb(0.028, 0.032, 0.058));
        commands.spawn((RightBg, Mesh2d(assets.quad.clone()), MeshMaterial2d(panel_bg), Transform::from_xyz(0.0, 0.0, 7.0), Visibility::Hidden));
        commands.spawn((
            RightHeader,
            Text2d::new(""),
            TextFont { font_size: FontSize::Px(font), ..default() },
            TextColor(Color::srgb(0.55, 0.65, 0.8)),
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 10.0),
            Visibility::Hidden,
        ));
        commands.spawn((
            RightLog,
            Text2d::new(""),
            TextFont { font_size: FontSize::Px(font), ..default() },
            TextColor(Color::srgb(0.85, 0.88, 0.95)),
            TextBounds::new_horizontal(300.0),
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, 10.0),
            Visibility::Hidden,
        ));
        state.0 = true;
    }
    // Only the bottom dock has a side panel; dock = "right" puts every tab
    // in the chat column instead.
    let show = layout.right > 0.0 && mods.cfg.chat.dock == mods::ChatDock::Bottom;
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    let h = layout.half();
    // The panel's left edge: the scene's right edge. It runs the window's full
    // height, beside the scene and the strip.
    let x0 = h.x;
    let full_h = layout.scene.y + layout.panel;
    let top = h.y;
    let mid_y = top - full_h * 0.5;
    for (mut tf, mut vis) in &mut bg {
        *vis = want;
        tf.translation = Vec3::new(x0 + layout.right * 0.5, mid_y, 7.0);
        tf.scale = Vec3::new(layout.right, full_h, 1.0);
    }
    for (mut t, mut tf, mut vis) in &mut header {
        *vis = want;
        let text = header_text(chat.side);
        if t.0 != text {
            t.0 = text;
        }
        tf.translation = Vec3::new(x0 + 14.0, top - 14.0, 10.0);
    }
    // Rebuilt when its rows change, the tab changes, or each 20 s for the ages.
    let now = real.elapsed_secs();
    let rows = ((full_h - 60.0) / (font * 1.25)).max(1.0) as usize;
    let count = match chat.side {
        Tab::Notify => chat.notices.len(),
        _ => chat.activity.len(),
    };
    let stale = chat.side_dirty || count != state.2 || rows != state.3 || now - state.1 > 20.0;
    for (e, mut tf, mut bounds, mut vis) in &mut log {
        *vis = want;
        tf.translation = Vec3::new(x0 + 14.0, top - 40.0, 10.0);
        let inner = (layout.right - 28.0).max(50.0);
        if bounds.width != Some(inner) {
            bounds.width = Some(inner);
        }
        if !show || !stale {
            continue;
        }
        let now_epoch = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let spans = match chat.side {
            Tab::Notify => notice_spans(&chat.notices, 0, rows / 2, now_epoch),
            _ => activity_spans(&chat.activity, 0, rows / 2, now_epoch),
        };
        commands.entity(e).despawn_related::<Children>();
        commands.entity(e).with_children(|p| {
            for (text, colour) in spans {
                p.spawn((TextSpan::new(text), TextColor(colour), TextFont { font_size: FontSize::Px(font), ..default() }));
            }
        });
    }
    if stale {
        chat.side_dirty = false;
        *state = (true, now, count, rows);
    }
}

// ---------------------------------------------------------------- the price chart
//
// Y, t145: "a live candlestick chart of sol/xrp in the bottom right panel",
// "using oracles". The oracle is the pricefeed's Coinbase spot samples
// (~/.local/state/warp-core/prices.json); here they're bucketed into
// candles and drawn with gizmos in the bottom of this panel, SOL above
// XRP, each with its last price beside the symbol.

#[derive(Component)]
pub struct CandleTag(usize);

/// Bucket [ts, price] samples into candles of `bucket` seconds:
/// (bucket start, open, high, low, close), oldest first.
pub fn candles(samples: &[(f64, f64)], bucket: f64) -> Vec<(f64, f32, f32, f32, f32)> {
    let mut out: Vec<(f64, f32, f32, f32, f32)> = Vec::new();
    for &(ts, price) in samples {
        let b = (ts / bucket).floor() * bucket;
        let p = price as f32;
        match out.last_mut() {
            Some(c) if c.0 == b => {
                c.2 = c.2.max(p);
                c.3 = c.3.min(p);
                c.4 = p;
            }
            _ => out.push((b, p, p, p, p)),
        }
    }
    out
}

const UP: [f32; 3] = [0.25, 0.9, 0.45];
const DOWN: [f32; 3] = [1.0, 0.18, 0.12];

#[allow(clippy::too_many_arguments)]
pub fn draw_candles(
    mut commands: Commands,
    mods: Res<Mods>,
    layout: Res<Layout>,
    real: Res<Time<Real>>,
    mut prices: Local<(f32, HashMap<String, Vec<(f64, f64)>>, String)>,
    mut tags: Query<(Entity, &CandleTag, &mut Text2d, &mut Transform, &mut Visibility)>,
    mut gizmos: Gizmos,
) {
    let font = mods.cfg.chat.font;
    let symbols = &mods.cfg.chart.symbols;
    let sig = symbols.join(",");
    // The symbol rows are entities; when [chart] symbols changes (futures
    // keys joining, t146) the old labels are replaced wholesale.
    if sig != prices.2 || tags.is_empty() {
        for (e, ..) in tags.iter() {
            commands.entity(e).despawn();
        }
        for (i, _) in symbols.iter().enumerate() {
            commands.spawn((
                CandleTag(i),
                Text2d::new(""),
                TextFont { font_size: FontSize::Px(font * 0.8), ..default() },
                TextColor(Color::srgb(0.6, 0.7, 0.85)),
                Anchor::TOP_LEFT,
                Transform::from_xyz(0.0, 0.0, 10.0),
                Visibility::Hidden,
            ));
        }
        prices.2 = sig;
        return;
    }
    let show = layout.right > 0.0
        && mods.cfg.chat.dock == mods::ChatDock::Bottom
        && mods.cfg.chart.show
        && !symbols.is_empty();
    if !show {
        for (_, _, _, _, mut vis) in tags.iter_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    }

    let now = real.elapsed_secs();
    if prices.0 == 0.0 || now - prices.0 > 2.0 {
        prices.1 = read_prices();
        prices.0 = now.max(1e-3);
    }

    // The bottom ~30% of the panel, one row per symbol, top to bottom.
    let h = layout.half();
    let x0 = h.x;
    let full_h = layout.scene.y + layout.panel;
    let y_bot = h.y - full_h;
    let chart_h = (full_h * 0.3).clamp(140.0, 380.0);
    let y_top = y_bot + chart_h;
    let rows = symbols.len() as f32;
    let row_h = chart_h / rows;
    let inner_x = x0 + 16.0;
    let inner_w = (layout.right - 32.0).max(40.0);
    let bucket = mods.cfg.chart.bucket_s.max(30.0) as f64;

    for (_e, tag, mut text, mut tf, mut vis) in tags.iter_mut() {
        let sym = symbols[tag.0].as_str();
        // Row 0 is on top: it spans [y_top - row_h, y_top]. A 16 px strip at
        // the row's top holds the symbol, last price and window change.
        let row_top = y_top - row_h * tag.0 as f32;
        let row_bot = row_top - row_h;
        let (plot_lo, plot_hi, label_y) = (row_bot + 4.0, row_top - 16.0, row_top - 12.0);
        *vis = Visibility::Inherited;
        tf.translation = Vec3::new(inner_x, label_y, 10.0);

        let Some(samples) = prices.1.get(sym) else {
            if text.0 != format!("{sym} --") {
                text.0 = format!("{sym} --");
            }
            continue;
        };
        let all = candles(samples, bucket);
        if all.is_empty() {
            if text.0 != format!("{sym} --") {
                text.0 = format!("{sym} --");
            }
            continue;
        }
        // ~16 px a candle, at least 6, at most 48 - two hours of 5-minute
        // candles is 24, which is what the feed keeps.
        let count = ((inner_w / 16.0) as usize).clamp(6, 48);
        let shown: Vec<_> = all.iter().rev().take(count).rev().collect();
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for c in &shown {
            lo = lo.min(c.3);
            hi = hi.max(c.2);
        }
        if !(hi > lo) {
            hi = lo + 1e-6;
        }
        let pad = (hi - lo) * 0.08;
        lo -= pad;
        hi += pad;
        let y_of = |p: f32| plot_lo + (p - lo) / (hi - lo) * (plot_hi - plot_lo);
        let pitch = inner_w / shown.len() as f32;
        let body_w = (pitch * 0.62).max(1.5);

        for (k, c) in shown.iter().enumerate() {
            let x = inner_x + pitch * (k as f32 + 0.5);
            let (_bucket, open, high, low, close) = **c;
            let up = close >= open;
            let col = Color::linear_rgb(
                if up { UP[0] } else { DOWN[0] },
                if up { UP[1] } else { DOWN[1] },
                if up { UP[2] } else { DOWN[2] },
            );
            // Wick: the full high-low line; body: open-close rect.
            gizmos.line_2d(Vec2::new(x, y_of(high)), Vec2::new(x, y_of(low)), col);
            let (y1, y2) = (y_of(open).min(y_of(close)), y_of(open).max(y_of(close)));
            gizmos.rect_2d(
                Isometry2d::from_translation(Vec2::new(x, (y1 + y2) * 0.5)),
                Vec2::new(body_w, (y2 - y1).max(1.0)),
                col,
            );
        }
        // A faint line at the last close, so the live edge reads at a glance.
        let last = shown.last().unwrap().4;
        gizmos.line_2d(
            Vec2::new(inner_x, y_of(last)),
            Vec2::new(inner_x + inner_w, y_of(last)),
            Color::linear_rgb(0.5, 0.6, 0.8),
        );
        let change = shown.last().unwrap().4 - shown.first().unwrap().1;
        let sign = if change >= 0.0 { "+" } else { "" };
        let label = format!("{sym} {:.2} {sign}{:.2}%", last, change / shown.first().unwrap().1 * 100.0);
        if text.0 != label {
            text.0 = label;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_brackets_the_side_tab() {
        assert_eq!(header_text(Tab::Notify), " F2 ACTIVITY  [F3 NOTIFY]");
    }

    #[test]
    fn candles_bucket_samples_into_ohlc() {
        // Two buckets of 100 s: prices 10, 12, 11 then 13, 9.
        let s = [(0.0, 10.0), (30.0, 12.0), (90.0, 11.0), (110.0, 13.0), (150.0, 9.0)];
        let c = candles(&s, 100.0);
        assert_eq!(c.len(), 2);
        // (bucket, open, high, low, close)
        assert_eq!(c[0], (0.0, 10.0, 12.0, 10.0, 11.0));
        assert_eq!(c[1], (100.0, 13.0, 13.0, 9.0, 9.0));
        assert!(candles(&[], 100.0).is_empty());
    }

    #[test]
    fn off_when_hidden_or_chat_off() {
        let cfg = mods::ChatCfg::default();
        assert!(right_width(2240.0, &cfg) > 0.0);
        assert_eq!(right_width(2240.0, &mods::ChatCfg { side: false, ..cfg.clone() }), 0.0);
        assert_eq!(right_width(2240.0, &mods::ChatCfg { show: false, ..cfg }), 0.0);
    }
}
