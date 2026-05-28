//! Popover content for binding a [`ButtonId`] to an [`Action`].
//!
//! Generic over the entity that should be notified after the binding changes
//! — that lets both the Phase 4 row and the Phase 6 mouse model reuse the
//! same picker.

use std::rc::Rc;

use gpui::{
    AnyElement, BorrowAppContext as _, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement as _, Styled, div, px, rgb,
};
use gpui_component::{popover::PopoverState, v_flex};

const POPOVER_W: f32 = 200.;
/// Cap the scrollable action list at ~11 rows tall. The catalog has 29+
/// entries plus section headers, so the full list would otherwise blow
/// past the window height. Header + footer-less list stays sticky.
const POPOVER_LIST_MAX_H: f32 = 360.;

use crate::data::mouse_buttons::{Action, ButtonId, Category};
use crate::state::AppState;
use crate::theme::{ACCENT_BLUE, SURFACE, SURFACE_HOVER, TEXT_MUTED, TEXT_PRIMARY};

/// Build the popover body that lets the user re-bind `btn`.
///
/// `observer` is whatever entity wraps the trigger — it'll be notified after
/// the global is updated so the trigger re-renders with the new label.
///
/// Actions are grouped by [`Category`] with a small muted section header
/// above each group.
pub fn action_picker<T: 'static>(
    btn: ButtonId,
    observer: &Entity<T>,
    cx: &mut Context<PopoverState>,
) -> AnyElement {
    let popover = cx.entity().downgrade();
    let current = cx
        .try_global::<AppState>()
        .and_then(|s| s.button_bindings.get(&btn).cloned());

    // Group the catalog by category while preserving catalog order within
    // each group.  We collect (category, items) in first-seen category order
    // so the sections appear in the same order the catalog defines them.
    let catalog = Action::catalog();
    let mut sections: Vec<(Category, Vec<Action>)> = Vec::new();
    for action in catalog {
        let cat = action.category();
        if let Some(sec) = sections.iter_mut().find(|(c, _)| *c == cat) {
            sec.1.push(action);
        } else {
            sections.push((cat, vec![action]));
        }
    }

    // Global item index for stable GPUI element IDs across sections.
    let mut item_idx: usize = 0;
    let mut children: Vec<AnyElement> = Vec::new();

    for (category, actions) in sections {
        // Section header — small muted all-caps label.
        children.push(
            div()
                .w_full()
                .px_2()
                .pt_2()
                .pb_1()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .child(category.label())
                .into_any_element(),
        );

        for action in actions {
            let is_selected = current.as_ref() == Some(&action);
            let label = action.label();
            let observer = observer.clone();
            let popover = popover.clone();
            let action = Rc::new(action);
            let idx = item_idx;
            item_idx += 1;

            children.push(
                div()
                    .id(("action-item", idx))
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .rounded_md()
                    .text_sm()
                    .text_color(rgb(if is_selected {
                        ACCENT_BLUE
                    } else {
                        TEXT_PRIMARY
                    }))
                    .bg(rgb(if is_selected { SURFACE_HOVER } else { SURFACE }))
                    .hover(|s| s.bg(rgb(SURFACE_HOVER)))
                    .child(label)
                    .on_click(move |_event, window, cx| {
                        let action = (*action).clone();
                        cx.update_global::<AppState, _>(|state, _| {
                            state.commit_binding(btn, action);
                        });
                        observer.update(cx, |_, cx| cx.notify());
                        if let Some(p) = popover.upgrade() {
                            p.update(cx, |s, cx| s.dismiss(window, cx));
                        }
                    })
                    .into_any_element(),
            );
        }
    }

    v_flex()
        .min_w(px(POPOVER_W))
        .gap_1()
        .p_2()
        .child(
            div()
                .text_xs()
                .text_color(rgb(TEXT_MUTED))
                .px_2()
                .pb_1()
                .child(format!("Bind {}", btn.label())),
        )
        // Action list scrolls. The catalog is long enough (29+ actions
        // across half a dozen categories) that an unconstrained popover
        // overflows the window; capping height + scroll keeps the
        // sticky-header pattern that's already familiar to the user.
        .child(
            div()
                .id("picker-scroll")
                .max_h(px(POPOVER_LIST_MAX_H))
                .overflow_y_scroll()
                .children(children),
        )
        .into_any_element()
}
