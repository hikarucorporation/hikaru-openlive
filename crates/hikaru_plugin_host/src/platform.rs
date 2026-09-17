// Copyright (C) Hikaru Corporation - 2026
// Platform helpers for X11 window creation (used on Wayland via XWayland)
// crates/hikaru_plugin_host/src/platform.rs

#[cfg(target_os = "linux")]
pub mod linux {
    use raw_window_handle::RawWindowHandle;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    /// Wraps an X11 connection so it stays alive while the GUI is open.
    /// When dropped, the connection closes and all windows created through it are destroyed.
    pub struct X11Connection {
        conn: x11rb::rust_connection::RustConnection,
        #[allow(dead_code)]
        screen_num: usize,
    }

    impl X11Connection {
        pub fn connect() -> Result<Self, String> {
            let (conn, screen_num) = x11rb::rust_connection::RustConnection::connect(None)
                .map_err(|e| format!("[Platform] Failed to connect to X11 (XWayland): {}", e))?;
            Ok(Self { conn, screen_num })
        }

        pub fn root_window(&self) -> Window {
            self.conn.setup().roots[self.screen_num].root
        }

        /// Create a top-level X11 window on XWayland for plugin GUI embedding.
        /// Returns the X11 window ID. The window stays alive as long as this
        /// connection is alive.
        pub fn create_window(&self, width: u16, height: u16) -> Result<u32, String> {
            let root = self.root_window();
            let window = self.conn.generate_id().map_err(|e| format!("[Platform] generate_id: {}", e))?;

            let window_aux = CreateWindowAux::new()
                .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY)
                .override_redirect(Some(1));

            self.conn.create_window(
                0, // depth: default
                window,
                root,
                0, 0, // x, y
                width,
                height,
                0, // border width
                WindowClass::INPUT_OUTPUT,
                0, // visual: default
                &window_aux,
            )
            .map_err(|e| format!("[Platform] create_window: {}", e))?;

            self.conn.map_window(window)
                .map_err(|e| format!("[Platform] map_window: {}", e))?;

            self.conn.flush()
                .map_err(|e| format!("[Platform] flush: {}", e))?;

            let window_id = window as u32;
            println!(
                "[Platform] X11 companion window created: {} ({}x{})",
                window_id, width, height
            );

            Ok(window_id)
        }
    }

    // Safety: RustConnection is Send+Sync safe for X11 operations
    unsafe impl Send for X11Connection {}

    /// Check if the given window handle is Wayland.
    pub fn is_wayland(handle: RawWindowHandle) -> bool {
        matches!(handle, RawWindowHandle::Wayland(_))
    }

    /// Get X11 window ID from a RawWindowHandle, or create a companion window on Wayland.
    /// Returns (window_id, connection) - the connection MUST be kept alive for the GUI lifetime.
    /// On X11/Xcb, the connection is None (no companion needed).
    pub fn get_or_create_x11_window(
        handle: RawWindowHandle,
        width: u16,
        height: u16,
    ) -> Result<(u32, Option<X11Connection>), String> {
        match handle {
            RawWindowHandle::Xlib(h) => Ok((h.window as u32, None)),
            RawWindowHandle::Xcb(h) => Ok((h.window.get().into(), None)),
            RawWindowHandle::Wayland(_) => {
                println!("[Platform] Wayland detected — creating X11 companion window via XWayland");
                let conn = X11Connection::connect()?;
                let window_id = conn.create_window(width, height)?;
                Ok((window_id, Some(conn)))
            }
            _ => Err(format!("[Platform] Unsupported window handle: {:?}", handle)),
        }
    }
}
