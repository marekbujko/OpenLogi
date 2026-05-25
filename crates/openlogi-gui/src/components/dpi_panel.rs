//! DPI slider + speed preview.
//!
//! The user drags the slider; the bound DPI value lives in [`AppState`] so
//! that the carousel and mouse-model annotations can read it later. A small
//! blue dot loops across the preview area at a speed proportional to DPI,
//! giving an immediate visual sense of what the value means.
//!
//! Per UI.md Phase 2.

use std::time::{Duration, Instant};

use gpui::{
    AppContext as _, BorrowAppContext as _, Context, Entity, IntoElement, ParentElement, Render,
    Styled, Subscription, Task, Window, div, px, rgb,
};
use gpui_component::{
    ActiveTheme, h_flex,
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};

use crate::state::AppState;
use crate::theme::{ACCENT_BLUE, BORDER, SURFACE, TEXT_MUTED};

/// Preview strip dimensions. The width is also the scroll distance the dot
/// covers per loop. Sized to fit beside the 420 px mouse model in the
/// 1100 px window.
const PREVIEW_W: f32 = 300.;
const PREVIEW_H: f32 = 64.;
const DOT_DIAMETER: f32 = 14.;

/// px/sec per DPI count. Tuned so DPI 200 reads as obviously sluggish and DPI
/// 6400 reads as obviously fast, without flying past so quickly the dot is
/// invisible.
const DOT_SPEED_PER_DPI: f32 = 0.5;

const TICK: Duration = Duration::from_millis(16);

/// How long the preview dot keeps moving after the most recent DPI
/// change. Outside this window the dot freezes — perpetual motion in a
/// static settings UI reads as distracting. The window is reset on
/// every slider change and primed on launch so the user gets one demo
/// loop without having to touch anything.
const ANIM_WINDOW: Duration = Duration::from_secs(3);

const MIN_DPI: f32 = 200.;
const MAX_DPI: f32 = 6400.;
const STEP_DPI: f32 = 50.;

pub struct DpiPanel {
    slider_state: Entity<SliderState>,
    dot_x: f32,
    /// Wall-clock timestamp of the most recent slider change. The
    /// animation loop only advances `dot_x` within [`ANIM_WINDOW`] of
    /// this value, then leaves the dot frozen until the next change.
    last_change: Instant,
    _slider_sub: Subscription,
    _animation: Task<()>,
}

impl DpiPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let initial_dpi = dpi_to_f32(
            cx.try_global::<AppState>()
                .map_or(crate::state::DEFAULT_DPI, |s| s.dpi),
        );

        // Order matters: SliderState defaults to max=100, and `.min(N)` calls
        // update_thumb_pos which clamps against the current max. Setting
        // max=6400 first keeps the intermediate state coherent.
        let slider_state = cx.new(|_| {
            SliderState::new()
                .max(MAX_DPI)
                .min(MIN_DPI)
                .step(STEP_DPI)
                .default_value(initial_dpi)
        });

        let slider_sub = cx.subscribe(&slider_state, |panel, _slider, event: &SliderEvent, cx| {
            let SliderEvent::Change(value) = event else {
                return;
            };
            let dpi = clamp_dpi(value.start());
            cx.update_global::<AppState, _>(|state, _| state.dpi = dpi);
            panel.last_change = Instant::now();
            cx.notify();
        });

        let animation = cx.spawn(async move |this, cx| {
            let mut last = cx.background_executor().now();
            loop {
                cx.background_executor().timer(TICK).await;
                let now = cx.background_executor().now();
                let dt = now.duration_since(last).as_secs_f32();
                last = now;

                if this
                    .update(cx, |panel, cx| {
                        if panel.last_change.elapsed() > ANIM_WINDOW {
                            return;
                        }
                        let dpi = cx
                            .try_global::<AppState>()
                            .map_or(crate::state::DEFAULT_DPI, |s| s.dpi);
                        let speed = dpi_to_f32(dpi) * DOT_SPEED_PER_DPI;
                        panel.dot_x += dt * speed;
                        let max = PREVIEW_W - DOT_DIAMETER;
                        if panel.dot_x > max {
                            panel.dot_x = 0.;
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            slider_state,
            dot_x: 0.,
            // Prime the window so the user sees one demo loop on launch
            // — `dot_x` advances for ANIM_WINDOW then freezes.
            last_change: Instant::now(),
            _slider_sub: slider_sub,
            _animation: animation,
        }
    }
}

impl Render for DpiPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dpi = cx
            .try_global::<AppState>()
            .map_or(crate::state::DEFAULT_DPI, |s| s.dpi);
        let theme = cx.theme();

        v_flex()
            .gap_4()
            .w(px(PREVIEW_W))
            .child(
                h_flex()
                    .justify_between()
                    .items_baseline()
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("DPI"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(ACCENT_BLUE))
                            .child(format!("{dpi}")),
                    ),
            )
            .child(Slider::new(&self.slider_state).horizontal())
            .child(preview(self.dot_x))
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(TEXT_MUTED))
                    .child("Preview: the dot's speed tracks DPI."),
            )
    }
}

fn preview(dot_x: f32) -> impl IntoElement {
    div()
        .relative()
        .w(px(PREVIEW_W))
        .h(px(PREVIEW_H))
        .rounded_md()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        .child(
            div()
                .absolute()
                .left(px(dot_x))
                .top(px((PREVIEW_H - DOT_DIAMETER) / 2.))
                .size(px(DOT_DIAMETER))
                .rounded_full()
                .bg(rgb(ACCENT_BLUE)),
        )
}

/// Snap a raw slider read to the discrete DPI step and clamp into range.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "value is rounded and clamped into [MIN_DPI, MAX_DPI] above the cast"
)]
fn clamp_dpi(raw: f32) -> u32 {
    raw.clamp(MIN_DPI, MAX_DPI).round() as u32
}

/// Widen a DPI count into f32 for slider math. DPI is bounded by ~6400 so
/// it fits comfortably in f32's 24-bit mantissa with no precision loss.
#[allow(
    clippy::cast_precision_loss,
    reason = "DPI ≤ 6400 — well below f32 mantissa precision"
)]
fn dpi_to_f32(dpi: u32) -> f32 {
    dpi as f32
}
