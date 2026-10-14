//! Links the window's `wl_surface` to the exported menu with KDE's `org_kde_kwin_appmenu`
//! protocol (plasma-wayland-protocols `appmenu.xml`, LGPL-2.1+; only the generated client
//! bindings from `wayland-protocols-plasma` are used).
//!
//! winit owns the Wayland connection and the surface, so this joins that connection from
//! its raw `wl_display` pointer and wraps the raw `wl_surface` pointer. Those two calls are
//! the only `unsafe` code in paintnotnet; everything else goes through wayland-client's
//! safe API on a private event queue.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::NonNull;

use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_plasma::appmenu::client::{
    org_kde_kwin_appmenu::OrgKdeKwinAppmenu, org_kde_kwin_appmenu_manager::OrgKdeKwinAppmenuManager,
};

pub struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinAppmenuManager, ()> for State {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinAppmenuManager,
        _: <OrgKdeKwinAppmenuManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinAppmenu, ()> for State {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinAppmenu,
        _: <OrgKdeKwinAppmenu as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// The appmenu objects for one window. Must not be dropped after winit's display is gone
/// (dropping destroys our event queue on that display); the owner keeps it in a
/// `ManuallyDrop`.
pub struct AppMenu {
    conn: Connection,
    _queue: EventQueue<State>,
    qh: QueueHandle<State>,
    manager: OrgKdeKwinAppmenuManager,
    surface: WlSurface,
    menu: Option<OrgKdeKwinAppmenu>,
}

impl AppMenu {
    /// Join winit's connection and bind the appmenu manager. `None` when the compositor
    /// doesn't offer it (anything but KWin).
    pub fn connect(display: NonNull<c_void>, surface: NonNull<c_void>) -> Option<Self> {
        // SAFETY: `display` is winit's live `wl_display`; it stays connected for as long as
        // the event loop runs, and this backend is never dropped (see `AppMenu`). The backend
        // is a guest: it doesn't close the connection.
        let backend = unsafe { Backend::from_foreign_display(display.as_ptr().cast()) };
        let conn = Connection::from_backend(backend);
        // SAFETY: `surface` is winit's live `wl_surface` proxy for our window, which outlives
        // every request made with it here (requests stop when the app shuts down).
        let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface.as_ptr().cast()) }
            .map_err(|e| log::warn!("global menu: surface: {e}"))
            .ok()?;
        let surface = WlSurface::from_id(&conn, id).ok()?;
        let (globals, queue) = registry_queue_init::<State>(&conn)
            .map_err(|e| log::warn!("global menu: registry: {e}"))
            .ok()?;
        let qh = queue.handle();
        let manager: OrgKdeKwinAppmenuManager = globals
            .bind(&qh, 1..=2, ())
            .map_err(|e| log::info!("global menu: compositor has no appmenu protocol ({e})"))
            .ok()?;
        Some(Self {
            conn,
            _queue: queue,
            qh,
            manager,
            surface,
            menu: None,
        })
    }

    /// Point the compositor at the menu's D-Bus address.
    pub fn set_address(&mut self, service: &str, path: &str) {
        let menu = self
            .menu
            .get_or_insert_with(|| self.manager.create(&self.surface, &self.qh, ()));
        menu.set_address(service.to_owned(), path.to_owned());
        if let Err(e) = self.conn.flush() {
            log::warn!("global menu: {e}");
        }
    }

    /// Unlink the menu (the desktop stops showing it for this window). KWin keeps the last
    /// address after the appmenu object is released, so clear the address instead; the
    /// object stays for when the menu is linked again.
    pub fn clear(&mut self) {
        if let Some(menu) = &self.menu {
            menu.set_address(String::new(), String::new());
            if let Err(e) = self.conn.flush() {
                log::warn!("global menu: {e}");
            }
        }
    }
}
