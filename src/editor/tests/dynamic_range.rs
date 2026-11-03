use super::*;

/// Plot geometry, matching the hand-computed values the other spectrum tests
/// use: x runs 57..843 over 20 Hz..20 kHz, y is 172.5 at 0 dB with 155 px over
/// 48 dB.
fn plot_x(hz: f32) -> f32 {
    57.0 + (hz / 20.0).ln() / 1000.0_f32.ln() * 786.0
}

fn plot_y(db: f32) -> f32 {
    172.5 - db / 48.0 * 155.0
}

/// Where the grip should land for a point: at the bottom of its range.
fn grip(f: &Fixture, index: usize) -> Pos2 {
    let v = f.editor.telemetry.values();
    let band = f.editor.params.band(index);
    let f0 = if v[2] > 0.0 { v[2] } else { PREVIEW_F0 };

    Pos2::new(
        plot_x(f0 * band.ratio.value()),
        plot_y(band.gain.value() + band.dynamics.range.value()),
    )
}

/// The two arrow heads of the grip, as drawn.
fn grips(f: &Fixture) -> Vec<Rect> {
    f.shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Path(path)
                if path.closed
                    && path.points.len() == 3
                    && path
                        .points
                        .iter()
                        .all(|point| point.y > 90.0 && point.y < 255.0) =>
            {
                Some(shape.shape.visual_bounding_rect())
            }
            _ => None,
        })
        .collect()
}

fn grip_center(f: &Fixture) -> Option<Pos2> {
    let rects = grips(f);
    Some(
        rects
            .iter()
            .fold(Rect::NOTHING, |acc, rect| acc.union(*rect))
            .center(),
    )
}

fn shades(f: &Fixture) -> usize {
    f.shapes
        .iter()
        .filter(
            |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.vertices.len() > 300),
        )
        .count()
}

#[test]
fn dynamic_range_grip_appears_only_with_dynamics_and_sets_the_range() {
    let mut f = Fixture::new();
    f.frame(vec![]);
    assert_eq!(grips(&f).len(), 0, "a static point must not show a grip");

    f.click_text("Dynamics");
    f.frame(vec![]);
    assert_eq!(grips(&f).len(), 2, "the grip is one arrow per direction");

    let from = grip(&f, 0);
    let drawn = grip_center(&f).expect("the grip is missing");
    assert!(
        (drawn.x - from.x).abs() < 1.0 && (drawn.y - from.y).abs() < 1.0,
        "grip drawn at {drawn:?} instead of {from:?}"
    );

    let gain = f.editor.params.band(0).gain.value();
    f.host.events.lock().unwrap().clear();
    f.drag(from, from + Vec2::new(0.0, 24.0));

    let range = f.editor.params.band(0).dynamics.range.value();
    let expected = -6.0 - 24.0 / 155.0 * 48.0;
    assert!(
        (range - expected).abs() < 0.5,
        "range is {range} dB instead of {expected} dB"
    );
    assert_eq!(
        f.editor.params.band(0).gain.value(),
        gain,
        "the grip must not touch the static gain"
    );

    let events = f.host.events.lock().unwrap().clone();
    assert_eq!(events.first(), Some(&"begin"), "events: {events:?}");
    assert_eq!(events.last(), Some(&"end"), "events: {events:?}");
    assert!(events.contains(&"set"), "events: {events:?}");
}

#[test]
fn the_grip_stops_at_zero_and_never_covers_the_point() {
    let mut f = Fixture::new();
    f.click_text("Dynamics");
    f.frame(vec![]);

    let from = grip(&f, 0);
    f.drag(from, from - Vec2::new(0.0, 400.0));
    f.frame(vec![]);
    assert_eq!(f.editor.params.band(0).dynamics.range.value(), 0.0);

    // With no range the grip would sit on the marker, which would make one of
    // the two undraggable, so it stops short of it.
    let marker = f.handle(0);
    let center = grip_center(&f).expect("the grip is missing");
    assert!(
        center.y >= marker.y + 16.0,
        "grip at {center:?} covers the marker at {marker:?}"
    );

    // And it can be grabbed again from where it is actually drawn.
    f.drag(center, center + Vec2::new(0.0, 24.0));
    assert!(
        f.editor.params.band(0).dynamics.range.value() < -7.0,
        "range is {}",
        f.editor.params.band(0).dynamics.range.value()
    );
}

#[test]
fn dynamic_points_shade_their_range_and_static_ones_do_not() {
    let mut f = Fixture::new();
    f.frame(vec![]);
    let plain = shades(&f);

    f.click_text("Dynamics");
    f.frame(vec![]);
    assert_eq!(shades(&f), plain + 1, "the range should be shaded");

    f.click_text("Dynamics");
    f.frame(vec![]);
    assert_eq!(shades(&f), plain, "the shading goes away with dynamics");
}

#[test]
fn the_grip_follows_the_selection() {
    let mut f = Fixture::new();
    f.frame(vec![]);
    let plain = shades(&f);

    f.click_text("Dynamics");
    f.frame(vec![]);
    assert_eq!(grips(&f).len(), 2);

    // A new point takes the selection, so the grip goes with it even though the
    // first point is still dynamic.
    f.add();
    f.frame(vec![]);
    assert_eq!(
        grips(&f).len(),
        0,
        "an unselected dynamic point must not show a grip"
    );

    f.click_text("Dynamics");
    f.frame(vec![]);
    let center = grip_center(&f).expect("the grip is missing");
    assert!(
        (center.x - grip(&f, 1).x).abs() < 1.0,
        "grip at {center:?} instead of the selected point at {:?}",
        grip(&f, 1)
    );
    assert_eq!(
        shades(&f),
        plain + 2,
        "both dynamic points keep their shading"
    );
}

#[test]
fn the_wheel_while_holding_the_grip_sets_q_and_leaves_the_range_alone() {
    let mut f = Fixture::new();
    f.click_text("Dynamics");
    f.frame(vec![]);

    let from = grip(&f, 0);
    let q = f.editor.params.band(0).q.value();
    let range = f.editor.params.band(0).dynamics.range.value();
    let moved = from + Vec2::new(1.0, 0.0);

    f.frame(vec![egui::Event::PointerMoved(from)]);
    f.frame(vec![
        egui::Event::PointerMoved(from),
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        },
    ]);
    f.frame(vec![egui::Event::PointerMoved(moved)]);
    f.frame(vec![egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: Vec2::new(0.0, 1.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Wheel,
        modifiers: Default::default(),
    }]);
    f.frame(vec![egui::Event::PointerButton {
        pos: moved,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    }]);

    let expected = q * 1.15;
    assert!(
        (f.editor.params.band(0).q.value() - expected).abs() < 0.01,
        "q is {} instead of {expected}",
        f.editor.params.band(0).q.value()
    );
    assert!(
        (f.editor.params.band(0).dynamics.range.value() - range).abs() < 0.5,
        "the wheel must not move the range"
    );
}
