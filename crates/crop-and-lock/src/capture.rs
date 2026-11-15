//! Capture the contents of an X11 window (or a region of it).
//!
//! When the Composite extension is available the window is redirected
//! (automatic mode) and its off-screen pixmap is read with NameWindowPixmap,
//! so occluded windows still render. Otherwise GetImage reads the window
//! directly, which only returns what is visible on screen.

use crate::geometry::{CropRect, zpixmap_to_rgba};
use anyhow::{Context, Result};
use image::RgbaImage;
use tracing::{debug, info, warn};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::composite::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt as _, Drawable, ImageFormat, ImageOrder, Window};
use x11rb::rust_connection::RustConnection;

pub struct WindowCapturer {
    conn: RustConnection,
    window: Window,
    redirected: bool,
}

impl WindowCapturer {
    pub fn new(window: Window) -> Result<Self> {
        let (conn, _) = crate::x11::connect()?;
        conn.get_geometry(window)?
            .reply()
            .with_context(|| format!("window 0x{window:x} does not exist"))?;
        let redirected = redirect(&conn, window);
        if redirected {
            info!("Capturing window 0x{window:x} via Composite (works while occluded)");
        } else {
            warn!(
                "Composite extension unavailable; capturing window 0x{window:x} from screen \
                 (occluded parts will not be visible)"
            );
        }
        Ok(Self {
            conn,
            window,
            redirected,
        })
    }

    pub fn window(&self) -> Window {
        self.window
    }

    pub fn connection(&self) -> &RustConnection {
        &self.conn
    }

    /// Current window size, or an error if the window was destroyed.
    pub fn size(&self) -> Result<(u32, u32)> {
        let (w, h, _) = self.geometry()?;
        Ok((w, h))
    }

    /// (width, height, border_width)
    fn geometry(&self) -> Result<(u32, u32, u32)> {
        let geom = self
            .conn
            .get_geometry(self.window)?
            .reply()
            .context("source window was destroyed")?;
        Ok((
            u32::from(geom.width),
            u32::from(geom.height),
            u32::from(geom.border_width),
        ))
    }

    pub fn is_alive(&self) -> bool {
        self.size().is_ok()
    }

    /// Capture the whole window, or `rect` clamped to the current window size.
    pub fn capture(&self, rect: Option<CropRect>) -> Result<RgbaImage> {
        let (w, h, border) = self.geometry()?;
        let full = CropRect::new(0, 0, w, h);
        let region = match rect {
            Some(r) => r
                .clamp_to(w, h)
                .context("crop region is outside the window")?,
            None => full,
        };

        if self.redirected {
            // The named pixmap includes the window border.
            let in_pixmap = CropRect::new(
                region.x + border,
                region.y + border,
                region.width,
                region.height,
            );
            match self.capture_pixmap(in_pixmap) {
                Ok(img) => return Ok(img),
                Err(e) => debug!("Composite capture failed, using GetImage on window: {e:#}"),
            }
        }
        self.get_image(self.window, region)
    }

    fn capture_pixmap(&self, region: CropRect) -> Result<RgbaImage> {
        let pixmap = self.conn.generate_id()?;
        self.conn
            .composite_name_window_pixmap(self.window, pixmap)?
            .check()
            .context("NameWindowPixmap failed")?;
        let result = self.get_image(pixmap, region);
        let _ = self.conn.free_pixmap(pixmap);
        result
    }

    fn get_image(&self, drawable: Drawable, region: CropRect) -> Result<RgbaImage> {
        let reply = self
            .conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                drawable,
                region.x as i16,
                region.y as i16,
                region.width as u16,
                region.height as u16,
                !0,
            )?
            .reply()
            .context("GetImage failed")?;

        let setup = self.conn.setup();
        let format = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == reply.depth)
            .with_context(|| format!("no pixmap format for depth {}", reply.depth))?;
        let lsb_first = setup.image_byte_order == ImageOrder::LSB_FIRST;
        let rgba = zpixmap_to_rgba(
            &reply.data,
            region.width,
            region.height,
            format.bits_per_pixel,
            format.scanline_pad,
            lsb_first,
        )?;
        RgbaImage::from_raw(region.width, region.height, rgba).context("image buffer size mismatch")
    }
}

impl Drop for WindowCapturer {
    fn drop(&mut self) {
        if self.redirected {
            let _ = self
                .conn
                .composite_unredirect_window(self.window, composite::Redirect::AUTOMATIC);
            let _ = self.conn.flush();
        }
    }
}

/// Redirect the window off-screen (automatic mode: the server still paints it
/// on screen as usual). Returns false if Composite is missing or it failed.
fn redirect(conn: &RustConnection, window: Window) -> bool {
    let has_ext = conn
        .extension_information(composite::X11_EXTENSION_NAME)
        .ok()
        .flatten()
        .is_some();
    if !has_ext {
        return false;
    }
    // NameWindowPixmap needs Composite >= 0.2
    let version_ok = conn
        .composite_query_version(0, 4)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|v| (v.major_version, v.minor_version) >= (0, 2));
    if !version_ok {
        return false;
    }
    conn.composite_redirect_window(window, composite::Redirect::AUTOMATIC)
        .ok()
        .and_then(|c| c.check().ok())
        .is_some()
}
