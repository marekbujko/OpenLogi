//! Centre-of-screen mouse silhouette with clickable hotspots.
//!
//! Per UI.md Phase 6. The base art is drawn from positioned divs rather than
//! shipping placeholder SVGs — keeps the asset pipeline empty until a real
//! illustrator is in the loop, and the silhouette is simple enough that
//! shapes are fine. Each hotspot is a `Popover` whose trigger is a custom
//! `HotspotTrigger` element that highlights on hover *and* while the popover
//! is open.

use gpui::{
    Anchor, AnyElement, App, Context, ElementId, Entity, InteractiveElement, IntoElement,
    MouseButton, ParentElement, Render, RenderOnce, StatefulInteractiveElement as _, Styled,
    Window, div, hsla, px, rgb,
};
use gpui_component::{Selectable, popover::Popover};

use crate::data::mouse_buttons::{ButtonId, Hotspot, MOUSE_MODEL_SIZE, default_hotspots};
use crate::mouse_model::picker::action_picker;
use crate::state::AppState;
use crate::theme::{ACCENT_BLUE, BORDER, SURFACE, SURFACE_HOVER};

pub struct MouseModelView {
    hotspots: Vec<Hotspot>,
    hovered: Option<ButtonId>,
}

impl MouseModelView {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            hotspots: default_hotspots(),
            hovered: None,
        }
    }
}

impl Render for MouseModelView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (w, h) = MOUSE_MODEL_SIZE;
        let active = cx.try_global::<AppState>().and_then(|s| s.active_button);
        let view = cx.entity();
        let hovered = self.hovered;

        div()
            .relative()
            .w(px(w))
            .h(px(h))
            .child(silhouette(w, h))
            .children(
                self.hotspots
                    .iter()
                    .enumerate()
                    .map(|(idx, hotspot)| hotspot_popover(idx, *hotspot, hovered, active, &view)),
            )
    }
}

/// The static "mouse body" art. Just a rounded slab with a wheel hint and a
/// thumb-cluster cutout — placeholder until a real illustrator hands over
/// a proper SVG.
fn silhouette(w: f32, h: f32) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .w(px(w))
        .h(px(h))
        .rounded_3xl()
        .border_1()
        .border_color(rgb(BORDER))
        .bg(rgb(SURFACE))
        // Scroll-wheel stripe.
        .child(
            div()
                .absolute()
                .left(px(w / 2. - 14.))
                .top(px(90.))
                .w(px(28.))
                .h(px(110.))
                .rounded_md()
                .bg(rgb(SURFACE_HOVER)),
        )
        // Subtle divider between left-click and right-click halves.
        .child(
            div()
                .absolute()
                .left(px(w / 2.))
                .top(px(20.))
                .w(px(1.))
                .h(px(240.))
                .bg(rgb(BORDER)),
        )
        // Thumb-cluster pocket on the left side.
        .child(
            div()
                .absolute()
                .left(px(8.))
                .top(px(210.))
                .w(px(34.))
                .h(px(150.))
                .rounded_md()
                .bg(rgb(SURFACE_HOVER)),
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

/// Transparent click target + glow. Implements [`Selectable`] so the
/// surrounding [`Popover`] can colour it while open.
#[derive(IntoElement)]
struct HotspotTrigger {
    id: ElementId,
    hotspot: Hotspot,
    /// True while the user is hovering or this hotspot is the active binding
    /// target. Drives the visible highlight independently of popover state.
    hovered: bool,
    view: Entity<MouseModelView>,
    /// Set by [`Popover`] when its content is open.
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
