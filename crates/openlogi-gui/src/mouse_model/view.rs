//! Centre-of-screen mouse silhouette with clickable hotspots and side
//! labels connected by leader lines.
//!
//! Per UI.md phases 6 (silhouette + hotspots), 7 (labels + leader lines),
//! and 8 (breathing). When a [`ResolvedAsset`] is supplied by the asset
//! cache the synthetic silhouette is replaced by the real device PNG and
//! the hotspot/label positions come from the Logitech-format
//! `core_metadata.json`. Without an asset, we fall back to the original
//! shape-based silhouette plus [`default_hotspots`] / [`default_labels`].

use std::time::Duration;

use gpui::{
    Anchor, Animation, AnimationExt as _, AnyElement, App, Context, ElementId, Entity, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, RenderOnce,
    StatefulInteractiveElement as _, Styled, Window, canvas, div, ease_in_out, hsla, img, px, rgb,
};
use gpui_component::{Selectable, popover::Popover, v_flex};

use crate::asset::ResolvedAsset;
use crate::asset::metadata::Metadata;
use crate::data::mouse_buttons::{ButtonId, Hotspot, MOUSE_MODEL_SIZE, default_hotspots};
use crate::mouse_model::leader_lines::{Label, Side, paint as paint_leader_lines};
use crate::mouse_model::picker::action_picker;
use crate::state::AppState;
use crate::theme::{ACCENT_BLUE, BORDER, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY};

// Side-gutter geometry. Labels sit on the *left* of the mouse so the right
// half of the window is free for the DPI / gesture config column.
const SIDE_W: f32 = 180.;
const SIDE_GAP: f32 = 24.;
const LABEL_W: f32 = 156.;
const LABEL_H: f32 = 44.;

/// Vertical amplitude of the breathing loop. Two pixels reads as a soft
/// rise/fall without feeling unstable.
const BREATH_AMPLITUDE: f32 = 2.0;

/// Approx pixel width of each hotspot hit-target. Logitech only gives us a
/// marker point per button, not a rectangle, so we size by hand.
const ASSET_HOTSPOT: f32 = 56.;

pub struct MouseModelView {
    hotspots: Vec<Hotspot>,
    labels: Vec<Label>,
    hovered: Option<ButtonId>,
    /// Cached device render + dimensions. `None` falls back to the
    /// shape-based silhouette and [`default_hotspots`] / [`default_labels`].
    asset: Option<ResolvedAsset>,
    mouse_w: f32,
    mouse_h: f32,
}

impl MouseModelView {
    pub fn new(asset: Option<ResolvedAsset>, _cx: &mut Context<Self>) -> Self {
        let (mouse_w, mouse_h) = MOUSE_MODEL_SIZE;
        let (mouse_w, mouse_h, hotspots, labels) = match asset.as_ref() {
            Some(a) => {
                let (w, h) = asset_dimensions(&a.metadata, mouse_h);
                let hotspots = asset_hotspots(&a.metadata, w, h);
                let labels = labels_from_hotspots(&hotspots);
                (w, h, hotspots, labels)
            }
            None => (mouse_w, mouse_h, default_hotspots(), default_labels()),
        };
        Self {
            hotspots,
            labels,
            hovered: None,
            asset,
            mouse_w,
            mouse_h,
        }
    }
}

impl Render for MouseModelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (mouse_w, mouse_h) = (self.mouse_w, self.mouse_h);
        let canvas_w = SIDE_W + SIDE_GAP + mouse_w;
        let canvas_h = mouse_h;
        let mouse_left = SIDE_W + SIDE_GAP;

        let active = cx.try_global::<AppState>().and_then(|s| s.active_button);
        let highlight = self.hovered.or(active);
        let bindings = cx
            .try_global::<AppState>()
            .map(|s| s.button_bindings.clone())
            .unwrap_or_default();
        let view = cx.entity();
        let hovered = self.hovered;

        let hotspots = self.hotspots.clone();
        let labels = self.labels.clone();
        let highlight_for_canvas = highlight;
        let leader_canvas = canvas(
            move |_bounds, _, _| (hotspots, labels, highlight_for_canvas),
            move |bounds, payload, window, _app| {
                let (hotspots, labels, highlight) = payload;
                paint_leader_lines(
                    bounds,
                    gpui::point(px(mouse_left), px(0.)),
                    mouse_w,
                    &hotspots,
                    &labels,
                    highlight,
                    window,
                );
            },
        )
        .size_full();

        // Either the real device image, or the shape-based silhouette.
        let device_art: AnyElement = match self.asset.as_ref() {
            Some(a) => img(a.image_path.clone())
                .w(px(mouse_w))
                .h(px(mouse_h))
                .into_any_element(),
            None => silhouette(mouse_w, mouse_h).into_any_element(),
        };

        div()
            .relative()
            .w(px(canvas_w))
            .h(px(canvas_h))
            .child(leader_canvas)
            .children(self.labels.iter().map(|label| {
                let binding = bindings
                    .get(&label.id)
                    .map_or("Unbound".to_string(), |a| a.label().to_string());
                label_card(
                    label,
                    binding,
                    highlight == Some(label.id),
                    mouse_left,
                    mouse_w,
                )
            }))
            .child(
                div()
                    .absolute()
                    .left(px(mouse_left))
                    .top(px(0.))
                    .w(px(mouse_w))
                    .h(px(mouse_h))
                    .child(device_art)
                    .children(self.hotspots.iter().enumerate().map(|(idx, hotspot)| {
                        hotspot_popover(idx, *hotspot, hovered, active, &view)
                    }))
                    .with_animation(
                        "mouse-breath",
                        Animation::new(Duration::from_secs(4))
                            .repeat()
                            .with_easing(ease_in_out),
                        |this, delta| {
                            let dy = (delta * std::f32::consts::TAU).sin() * BREATH_AMPLITUDE;
                            this.top(px(dy))
                        },
                    ),
            )
    }
}

/// Scale the device image to fit a target height while preserving aspect.
#[allow(
    clippy::cast_precision_loss,
    reason = "device images are < 4096 px on either axis — well within f32 mantissa"
)]
fn asset_dimensions(meta: &Metadata, target_h: f32) -> (f32, f32) {
    let Some(origin) = meta.origin() else {
        return MOUSE_MODEL_SIZE;
    };
    let w = target_h * (origin.width as f32) / (origin.height as f32);
    (w, target_h)
}

/// Convert Logitech's percent-based markers into mouse-local pixel rects.
/// Each marker is a point, so we centre a fixed-size hit area on it.
fn asset_hotspots(meta: &Metadata, mouse_w: f32, mouse_h: f32) -> Vec<Hotspot> {
    meta.hotspots()
        .map(|h| {
            let cx = h.marker.x / 100. * mouse_w;
            let cy = h.marker.y / 100. * mouse_h;
            Hotspot {
                id: h.id,
                x: cx - ASSET_HOTSPOT / 2.,
                y: cy - ASSET_HOTSPOT / 2.,
                w: ASSET_HOTSPOT,
                h: ASSET_HOTSPOT,
            }
        })
        .collect()
}

/// Lay labels out on the left side, evenly spaced down the mouse's vertical
/// extent in the same order the hotspots appear in the asset metadata.
/// Logitech's `label.{x,y}` direction codes are ignored for now — the
/// current layout reserves the right gutter for the DPI / gesture column.
#[allow(
    clippy::cast_precision_loss,
    reason = "hotspot count is bounded by ButtonId variants — well under f32 mantissa"
)]
fn labels_from_hotspots(hotspots: &[Hotspot]) -> Vec<Label> {
    if hotspots.is_empty() {
        return Vec::new();
    }
    let mouse_h = MOUSE_MODEL_SIZE.1;
    let step = mouse_h / (hotspots.len() as f32 + 1.);
    hotspots
        .iter()
        .enumerate()
        .map(|(i, h)| Label {
            id: h.id,
            side: Side::Left,
            y: step * (i as f32 + 1.),
        })
        .collect()
}

fn default_labels() -> Vec<Label> {
    vec![
        Label {
            id: ButtonId::LeftClick,
            side: Side::Left,
            y: 60.,
        },
        Label {
            id: ButtonId::RightClick,
            side: Side::Left,
            y: 130.,
        },
        Label {
            id: ButtonId::MiddleClick,
            side: Side::Left,
            y: 200.,
        },
        Label {
            id: ButtonId::Back,
            side: Side::Left,
            y: 290.,
        },
        Label {
            id: ButtonId::Forward,
            side: Side::Left,
            y: 360.,
        },
        Label {
            id: ButtonId::DpiToggle,
            side: Side::Left,
            y: 440.,
        },
    ]
}

fn label_card(
    label: &Label,
    binding: String,
    highlighted: bool,
    mouse_left: f32,
    mouse_w: f32,
) -> impl IntoElement {
    let x = match label.side {
        Side::Left => mouse_left - SIDE_GAP - SIDE_W,
        Side::Right => mouse_left + mouse_w + SIDE_GAP,
    };

    div()
        .absolute()
        .left(px(x))
        .top(px(label.y - LABEL_H / 2.))
        .w(px(LABEL_W))
        .h(px(LABEL_H))
        .px_3()
        .py_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(if highlighted { ACCENT_BLUE } else { BORDER }))
        .bg(rgb(SURFACE_HOVER))
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(TEXT_MUTED))
                        .child(label.id.label()),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if highlighted {
                            ACCENT_BLUE
                        } else {
                            TEXT_PRIMARY
                        }))
                        .child(binding),
                ),
        )
}

/// Shape-based silhouette used when no asset is cached for the device.
fn silhouette(w: f32, h: f32) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .w(px(w))
        .h(px(h))
        .rounded_3xl()
        .border_1()
        .border_color(rgb(TEXT_MUTED))
        .bg(rgb(SURFACE_HOVER))
        .child(
            div()
                .absolute()
                .left(px(w / 2. - 14.))
                .top(px(90.))
                .w(px(28.))
                .h(px(110.))
                .rounded_md()
                .bg(hsla(0., 0., 0.25, 1.0)),
        )
        .child(
            div()
                .absolute()
                .left(px(w / 2.))
                .top(px(20.))
                .w(px(1.))
                .h(px(240.))
                .bg(rgb(BORDER)),
        )
        .child(
            div()
                .absolute()
                .left(px(8.))
                .top(px(210.))
                .w(px(34.))
                .h(px(150.))
                .rounded_md()
                .bg(hsla(0., 0., 0.25, 1.0)),
        )
}

fn hotspot_popover(
    idx: usize,
    hotspot: Hotspot,
    hovered: Option<ButtonId>,
    active: Option<ButtonId>,
    view: &Entity<MouseModelView>,
) -> AnyElement {
    let view = view.clone();
    let trigger = HotspotTrigger {
        id: ("hotspot-trigger", idx).into(),
        hotspot,
        hovered: hovered == Some(hotspot.id) || active == Some(hotspot.id),
        view: view.clone(),
        selected: false,
    };
    Popover::new(("hotspot-popover", idx))
        .anchor(Anchor::TopRight)
        .mouse_button(MouseButton::Left)
        .trigger(trigger)
        .content(move |_state, _window, cx| action_picker(hotspot.id, &view, cx))
        .into_any_element()
}

#[derive(IntoElement)]
struct HotspotTrigger {
    id: ElementId,
    hotspot: Hotspot,
    hovered: bool,
    view: Entity<MouseModelView>,
    selected: bool,
}

impl Selectable for HotspotTrigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl RenderOnce for HotspotTrigger {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let highlighted = self.hovered || self.selected;
        let hotspot = self.hotspot;
        let view = self.view;
        let btn = hotspot.id;

        div()
            .id(self.id)
            .absolute()
            .left(px(hotspot.x))
            .top(px(hotspot.y))
            .w(px(hotspot.w))
            .h(px(hotspot.h))
            .rounded_md()
            .border_2()
            .border_color(if highlighted {
                rgb(ACCENT_BLUE).into()
            } else {
                hsla(0., 0., 0., 0.)
            })
            .bg(if highlighted {
                hsla(0.6, 0.85, 0.6, 0.18)
            } else {
                hsla(0., 0., 0., 0.)
            })
            .on_hover(move |hovered, _window, cx| {
                let is_hovered = *hovered;
                view.update(cx, |this, cx| {
                    if is_hovered {
                        this.hovered = Some(btn);
                    } else if this.hovered == Some(btn) {
                        this.hovered = None;
                    }
                    cx.notify();
                });
            })
    }
}
