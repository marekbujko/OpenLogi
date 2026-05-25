//! OpenLogi GPUI desktop window.
//!
//! Initial HID++ inventory is collected synchronously on startup (GPUI owns
//! the main thread, so we can't move it onto a tokio runtime). Live polling
//! lands when there's something to react to.

mod app;
mod asset;
mod components;
mod data;
mod mouse_model;
mod state;
mod theme;

use anyhow::{Context as _, Result};
use gpui::{
    AppContext, Bounds, SharedString, Size, Styled, TitlebarOptions, WindowBounds, WindowOptions,
    px,
};
use gpui_component::{ActiveTheme, Root};
use openlogi_core::device::DeviceInventory;
use tracing_subscriber::EnvFilter;

use crate::app::AppView;

fn main() -> Result<()> {
    init_tracing();

    let inventories = enumerate_blocking().context("HID enumeration failed")?;

    gpui_platform::application().run(move |cx| {
        gpui_component::init(cx);
        cx.spawn(async move |cx| {
            let bounds = cx.update(|cx| Bounds::centered(None, Size::new(px(1100.), px(750.)), cx));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(Size::new(px(720.), px(520.))),
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from("OpenLogi")),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                ..WindowOptions::default()
            };

            #[allow(
                clippy::expect_used,
                reason = "failure to open the main window is fatal; nothing useful to recover to"
            )]
            cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| AppView::new(&inventories, cx));
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("opening the main window should not fail");
        })
        .detach();
    });

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_env("OPENLOGI_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}

fn enumerate_blocking() -> Result<Vec<DeviceInventory>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime init")?;
    rt.block_on(openlogi_hid::enumerate())
        .context("openlogi_hid::enumerate")
}
