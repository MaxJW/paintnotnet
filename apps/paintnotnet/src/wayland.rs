//! The window's own Wayland objects, on winit's connection: KDE's `org_kde_kwin_appmenu`
//! (plasma-wayland-protocols `appmenu.xml`, LGPL-2.1+; only the generated client bindings
//! from `wayland-protocols-plasma` are used) to link the global menu, and `xdg_activation_v1`
//! to raise the window when another launch hands it files (`single_instance`).
//!
//! winit owns the Wayland connection and the surface, so this joins that connection from
//! its raw `wl_display` pointer and wraps the raw `wl_surface` pointer. Those two calls are
//! the app's only `unsafe` code (the other is `pnn-ai`'s cuDNN preload); everything else goes
//! through wayland-client's safe API on a private event queue.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
use wayland_protocols_plasma::appmenu::client::{
    org_kde_kwin_appmenu::OrgKdeKwinAppmenu, org_kde_kwin_appmenu_manager::OrgKdeKwinAppmenuManager,
};

pub struct State;

/// Requests only: none of these objects sends events we use.
macro_rules! ignore_events {
    ($($t:ty, $data:ty);*) => {$(
        impl Dispatch<$t, $data> for State {
            fn event(
                _: &mut Self,
                _: &$t,
                _: <$t as Proxy>::Event,
                _: &$data,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore_events!(
    wl_registry::WlRegistry, GlobalListContents;
    OrgKdeKwinAppmenuManager, ();
    OrgKdeKwinAppmenu, ();
    XdgActivationV1, ()
);

/// The window's Wayland objects. Must not be dropped after winit's display is gone
/// (dropping destroys our event queue on that display); the app keeps it in a
/// `ManuallyDrop`.
pub struct WindowLink {
    conn: Connection,
    _queue: EventQueue<State>,
    qh: QueueHandle<State>,
    surface: WlSurface,
    appmenu: Option<OrgKdeKwinAppmenuManager>,
    menu: Option<OrgKdeKwinAppmenu>,
    activation: Option<XdgActivationV1>,
}

impl WindowLink {
    /// Join winit's connection for the app's window. `None` off Wayland.
    pub fn new(cc: &eframe::CreationContext) -> Option<Self> {
        let (Ok(display), Ok(window)) = (cc.display_handle(), cc.window_handle()) else {
            return None;
        };
        match (display.as_raw(), window.as_raw()) {
            (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) => {
                Self::connect(d.display, w.surface)
            }
            _ => None,
        }
    }

    fn connect(display: NonNull<c_void>, surface: NonNull<c_void>) -> Option<Self> {
        // SAFETY: `display` is winit's live `wl_display`; it stays connected for as long as
        // the event loop runs, and this backend is never dropped (see `WindowLink`). The
        // backend is a guest: it doesn't close the connection.
        let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        // SAFETY: `surface` is winit's live `wl_surface` proxy for our window, which outlives
        // every request made with it here (requests stop when the app shuts down).
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }
            .map_err(|e| log::warn!("wayland: surface: {e}"))
            .ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;
        let (globals, queue) = registry_queue_init::<State>(&conn)
            .map_err(|e| log::warn!("wayland: registry: {e}"))
            .ok()?;
        let qh = queue.handle();
        let appmenu = globals
            .bind(&qh, 1..=2, ())
            .map_err(|e| log::info!("wayland: no appmenu protocol ({e})"))
            .ok();
        let activation = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| log::info!("wayland: no xdg-activation ({e})"))
            .ok();
        Some(Self {
            conn,
            _queue: queue,
            qh,
            surface,
            appmenu,
            menu: None,
            activation,
        })
    }

    /// Whether the compositor can show a global menu (KWin's appmenu protocol).
    pub fn has_appmenu(&self) -> bool {
        self.appmenu.is_some()
    }

    fn flush(&self) {
        if let Err(e) = self.conn.flush() {
            log::warn!("wayland: {e}");
        }
    }

    /// Point the compositor at the global menu's D-Bus address.
    pub fn set_menu_address(&mut self, service: &str, path: &str) {
        let Some(manager) = &self.appmenu else {
            return;
        };
        let menu = self
            .menu
            .get_or_insert_with(|| manager.create(&self.surface, &self.qh, ()));
        menu.set_address(service.to_owned(), path.to_owned());
        self.flush();
    }

    /// Unlink the global menu (the desktop stops showing it for this window). KWin keeps
    /// the last address after the appmenu object is released, so clear the address
    /// instead; the object stays for when the menu is linked again.
    pub fn clear_menu(&mut self) {
        if let Some(menu) = &self.menu {
            menu.set_address(String::new(), String::new());
            self.flush();
        }
    }

    /// Ask the compositor to raise and focus the window with an activation token from the
    /// launch that asked for it (its `XDG_ACTIVATION_TOKEN`). Returns false without
    /// xdg-activation.
    pub fn activate(&self, token: &str) -> bool {
        let Some(activation) = &self.activation else {
            return false;
        };
        activation.activate(token.to_owned(), &self.surface);
        self.flush();
        true
    }
}
