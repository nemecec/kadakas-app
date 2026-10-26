//! The wheel, the two-finger scroll and the pinch of a 3D canvas, for both programs.
//!
//! egui changes the scroll before the canvas sees it: a vertical scroll with Shift becomes a
//! horizontal one, and the modifier is kept for a whole gesture. The trackpad style needs the
//! scroll as the window system sent it, so this module reads the raw wheel events of the frame
//! and passes them to `kadakas_ui_core::nav3d::scroll_motion`, which keeps the choice of Shift
//! for a gesture. The mouse style still uses the scroll of egui, as before.

use glam::Vec2;
use kadakas_ui_core::nav3d::{
    NavStyle, ScrollLatch, ScrollMotion, WheelEvent, WheelPhase, scroll_motion,
};

/// Points for one line of a wheel that reports lines (egui's own default).
const LINE_POINTS: f32 = 40.0;
/// Points for one page.
const PAGE_POINTS: f32 = 400.0;

/// The raw wheel events of this frame as `nav3d` wants them.
pub fn wheel_events(events: &[egui::Event]) -> Vec<WheelEvent> {
    events
        .iter()
        .filter_map(|e| match e {
            egui::Event::MouseWheel {
                unit,
                delta,
                phase,
                modifiers,
            } => {
                let k = match unit {
                    egui::MouseWheelUnit::Point => 1.0,
                    egui::MouseWheelUnit::Line => LINE_POINTS,
                    egui::MouseWheelUnit::Page => PAGE_POINTS,
                };
                Some(WheelEvent {
                    delta: Vec2::new(delta.x, delta.y) * k,
                    shift: modifiers.shift,
                    phase: match phase {
                        egui::TouchPhase::Start => WheelPhase::Start,
                        egui::TouchPhase::Move => WheelPhase::Move,
                        egui::TouchPhase::End | egui::TouchPhase::Cancel => WheelPhase::End,
                    },
                })
            }
            _ => None,
        })
        .collect()
}

/// What this frame of scroll and pinch does to the view of the canvas `id` in the style `style`.
/// Call it when the pointer is over the canvas.
pub fn frame_motion(ctx: &egui::Context, id: egui::Id, style: NavStyle) -> ScrollMotion {
    let (events, smooth, pinch, now) = ctx.input(|i| {
        (
            i.events.clone(),
            Vec2::new(i.smooth_scroll_delta.x, i.smooth_scroll_delta.y),
            i.zoom_delta(),
            i.time,
        )
    });
    let mut latch: ScrollLatch = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let motion = scroll_motion(
        style,
        &mut latch,
        &wheel_events(&events),
        smooth,
        pinch,
        now,
    );
    ctx.data_mut(|d| d.insert_temp(id, latch));
    motion
}

/// The pointer is not over the canvas: the gesture that was going on is forgotten, so a Shift
/// choice does not wait for the next time the pointer comes back.
pub fn pointer_left(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| {
        if let Some(latch) = d.get_temp::<ScrollLatch>(id) {
            let mut latch = latch;
            latch.forget();
            d.insert_temp(id, latch);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(
        unit: egui::MouseWheelUnit,
        x: f32,
        y: f32,
        shift: bool,
        phase: egui::TouchPhase,
    ) -> egui::Event {
        egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(x, y),
            phase,
            modifiers: egui::Modifiers {
                shift,
                ..Default::default()
            },
        }
    }

    fn motion(ctx: &egui::Context, events: Vec<egui::Event>, style: NavStyle) -> ScrollMotion {
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(100.0, 100.0),
            )),
            ..Default::default()
        };
        let mut out = None;
        let _ = ctx.run_ui(input, |ui| {
            out = Some(frame_motion(ui.ctx(), egui::Id::new("c"), style));
        });
        out.unwrap()
    }

    #[test]
    fn raw_events_keep_the_units_the_phases_and_shift() {
        let events = wheel_events(&[
            wheel(
                egui::MouseWheelUnit::Line,
                0.0,
                1.0,
                false,
                egui::TouchPhase::Move,
            ),
            wheel(
                egui::MouseWheelUnit::Page,
                1.0,
                0.0,
                true,
                egui::TouchPhase::Start,
            ),
            wheel(
                egui::MouseWheelUnit::Point,
                2.0,
                3.0,
                false,
                egui::TouchPhase::Cancel,
            ),
            egui::Event::Zoom(1.2),
        ]);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].delta, Vec2::new(0.0, 40.0));
        assert_eq!(events[1].delta, Vec2::new(400.0, 0.0));
        assert!(events[1].shift && events[1].phase == WheelPhase::Start);
        assert_eq!(events[2].phase, WheelPhase::End);
    }

    #[test]
    fn a_vertical_shift_scroll_tilts_and_a_horizontal_one_turns_with_the_trackpad() {
        let ctx = egui::Context::default();
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                30.0,
                true,
                egui::TouchPhase::Move,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.orbit, Vec2::new(0.0, 30.0), "pitch, not yaw");
        assert_eq!(m.pan, Vec2::ZERO);
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                30.0,
                0.0,
                true,
                egui::TouchPhase::Move,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.orbit, Vec2::new(30.0, 0.0), "yaw");
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                5.0,
                30.0,
                false,
                egui::TouchPhase::Move,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!((m.pan, m.orbit), (Vec2::new(5.0, 30.0), Vec2::ZERO));
    }

    #[test]
    fn the_gesture_is_forgotten_when_the_pointer_leaves_the_canvas() {
        let ctx = egui::Context::default();
        let id = egui::Id::new("c");
        motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                0.0,
                true,
                egui::TouchPhase::Start,
            )],
            NavStyle::Trackpad,
        );
        pointer_left(&ctx, id);
        // Back on the canvas, a Move without Shift is a pan: the Shift of the old gesture is gone.
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                12.0,
                false,
                egui::TouchPhase::Move,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.pan, Vec2::new(0.0, 12.0));
        // Nothing to forget: no panic.
        pointer_left(&egui::Context::default(), id);
    }

    #[test]
    fn the_latch_lives_across_frames() {
        let ctx = egui::Context::default();
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                0.0,
                true,
                egui::TouchPhase::Start,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.orbit, Vec2::ZERO);
        // The next frame: Shift is up, the gesture goes on as an orbit.
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                12.0,
                false,
                egui::TouchPhase::Move,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.orbit, Vec2::new(0.0, 12.0));
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                0.0,
                false,
                egui::TouchPhase::End,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.orbit, Vec2::ZERO);
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Point,
                0.0,
                12.0,
                false,
                egui::TouchPhase::Move,
            )],
            NavStyle::Trackpad,
        );
        assert_eq!(m.pan, Vec2::new(0.0, 12.0));
    }

    #[test]
    fn the_mouse_style_reads_the_scroll_of_egui_as_before() {
        let ctx = egui::Context::default();
        // A pointer over the screen is not needed for the input state.
        let m = motion(
            &ctx,
            vec![wheel(
                egui::MouseWheelUnit::Line,
                0.0,
                1.0,
                false,
                egui::TouchPhase::Move,
            )],
            NavStyle::Mouse,
        );
        assert!(m.zoom > 1.0, "wheel up zooms in: {}", m.zoom);
        assert_eq!((m.pan, m.orbit), (Vec2::ZERO, Vec2::ZERO));
    }
}
