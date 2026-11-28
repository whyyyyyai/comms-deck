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
    let show = layout.right > 0.0;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_brackets_the_side_tab() {
        assert_eq!(header_text(Tab::Notify), " F2 ACTIVITY  [F3 NOTIFY]");
    }

    #[test]
    fn off_when_hidden_or_chat_off() {
        let cfg = mods::ChatCfg::default();
        assert!(right_width(2240.0, &cfg) > 0.0);
        assert_eq!(right_width(2240.0, &mods::ChatCfg { side: false, ..cfg.clone() }), 0.0);
        assert_eq!(right_width(2240.0, &mods::ChatCfg { show: false, ..cfg }), 0.0);
    }
}
