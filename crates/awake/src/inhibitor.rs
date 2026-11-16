use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use zbus::blocking::Connection;

/// logind inhibit "what" when the display must also stay on.
const WHAT_DISPLAY: &str = "idle:sleep";
/// logind inhibit "what" when only system sleep is blocked.
const WHAT_SLEEP: &str = "sleep";

/// Holds D-Bus screensaver/suspend inhibit cookies.
/// Dropping or calling `release()` un-inhibits.
pub struct ScreenSaverInhibitor {
    conn: Option<Connection>,
    screensaver_cookie: Option<u32>,
    login_fd: Option<zbus::zvariant::OwnedFd>,
    login_what: &'static str,
    display_inhibited: bool,
}

impl ScreenSaverInhibitor {
    /// Inhibit screensaver and/or suspend via D-Bus.
    ///
    /// Uses two interfaces:
    /// - `org.freedesktop.ScreenSaver.Inhibit` — prevents screen blanking (only when `keep_screen_on`)
    /// - `org.freedesktop.login1.Manager.Inhibit` — prevents system suspend
    ///   (`idle:sleep` with `keep_screen_on`, otherwise `sleep` only)
    pub fn inhibit(keep_screen_on: bool) -> Result<Self> {
        let conn = match Connection::session() {
            Ok(c) => Some(c),
            Err(e) => {
                warn!("Awake: no session D-Bus, screensaver inhibit unavailable: {e}");
                None
            }
        };

        let mut this = Self {
            conn,
            screensaver_cookie: None,
            login_fd: None,
            login_what: WHAT_SLEEP,
            display_inhibited: false,
        };

        this.set_display_inhibited(keep_screen_on);
        if this.login_fd.is_none() {
            this.swap_login_inhibit(WHAT_SLEEP);
        }

        if this.screensaver_cookie.is_none() && this.login_fd.is_none() {
            anyhow::bail!("failed to inhibit both screensaver and suspend");
        }
        Ok(this)
    }

    /// Whether the display is currently being kept on.
    pub fn display_inhibited(&self) -> bool {
        self.display_inhibited
    }

    /// Start or stop keeping the display on, while always keeping system
    /// sleep blocked.
    pub fn set_display_inhibited(&mut self, on: bool) {
        if on {
            if self.screensaver_cookie.is_none()
                && let Some(conn) = &self.conn
            {
                match inhibit_screensaver(conn) {
                    Ok(c) => self.screensaver_cookie = Some(c),
                    Err(e) => warn!("Awake: screensaver inhibit failed: {e:#}"),
                }
            }
            if self.login_what != WHAT_DISPLAY || self.login_fd.is_none() {
                self.swap_login_inhibit(WHAT_DISPLAY);
            }
        } else {
            if let Some(cookie) = self.screensaver_cookie.take()
                && let Some(conn) = &self.conn
            {
                match uninhibit_screensaver(conn, cookie) {
                    Ok(()) => info!("Released screensaver inhibitor"),
                    Err(e) => warn!("Failed to release screensaver inhibitor: {e}"),
                }
            }
            if self.login_what != WHAT_SLEEP || self.login_fd.is_none() {
                self.swap_login_inhibit(WHAT_SLEEP);
            }
        }
        self.display_inhibited = on;
    }

    /// Acquire a new logind inhibitor before dropping the old one so sleep
    /// is never briefly allowed.
    fn swap_login_inhibit(&mut self, what: &'static str) {
        match inhibit_suspend(what) {
            Ok(fd) => {
                self.login_fd = Some(fd);
                self.login_what = what;
            }
            Err(e) => warn!("Awake: logind inhibit ({what}) failed: {e:#}"),
        }
    }

    pub fn release(mut self) -> Result<()> {
        // Release screensaver inhibitor
        if let Some(cookie) = self.screensaver_cookie.take()
            && let Some(conn) = &self.conn
        {
            if let Err(e) = uninhibit_screensaver(conn, cookie) {
                tracing::warn!("Failed to release screensaver inhibitor: {e}");
            } else {
                info!("Released screensaver inhibitor");
            }
        }

        // Suspend inhibitor is released by dropping the fd
        if self.login_fd.take().is_some() {
            info!("Released suspend inhibitor (fd dropped)");
        }

        Ok(())
    }
}

/// Call org.freedesktop.ScreenSaver.Inhibit(app_name, reason) -> cookie
fn inhibit_screensaver(conn: &Connection) -> Result<u32> {
    let reply = conn.call_method(
        Some("org.freedesktop.ScreenSaver"),
        "/org/freedesktop/ScreenSaver",
        Some("org.freedesktop.ScreenSaver"),
        "Inhibit",
        &("MyPowerToys", "User requested Awake mode"),
    )?;

    let cookie: u32 = reply.body().deserialize()?;
    info!("Screensaver inhibited (cookie={cookie})");
    Ok(cookie)
}

/// Call org.freedesktop.ScreenSaver.UnInhibit(cookie)
fn uninhibit_screensaver(conn: &Connection, cookie: u32) -> Result<()> {
    conn.call_method(
        Some("org.freedesktop.ScreenSaver"),
        "/org/freedesktop/ScreenSaver",
        Some("org.freedesktop.ScreenSaver"),
        "UnInhibit",
        &(cookie,),
    )?;
    Ok(())
}

/// Inhibit suspend via systemd-logind.
/// Returns an fd that must be kept open to maintain the inhibition.
fn inhibit_suspend(what: &str) -> Result<zbus::zvariant::OwnedFd> {
    let conn = Connection::system().context("failed to connect to system D-Bus for logind")?;

    let reply = conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        Some("org.freedesktop.login1.Manager"),
        "Inhibit",
        &(
            what,                        // what
            "MyPowerToys",               // who
            "User requested Awake mode", // why
            "block",                     // mode
        ),
    )?;

    let fd: zbus::zvariant::OwnedFd = reply.body().deserialize()?;
    info!("Suspend inhibited (logind fd acquired, what={what})");
    Ok(fd)
}

/// Detects whether the user's session is locked. Uses logind's
/// `LockedHint` on the caller's session and falls back to the screensaver
/// `GetActive` calls (GNOME, then freedesktop/KDE).
pub struct LockWatcher {
    system: Option<Connection>,
    session: Option<Connection>,
}

impl LockWatcher {
    pub fn new() -> Self {
        Self {
            system: Connection::system().ok(),
            session: Connection::session().ok(),
        }
    }

    /// `None` when no source could be queried.
    pub fn is_locked(&self) -> Option<bool> {
        let mut answered = false;

        if let Some(sys) = &self.system {
            match locked_hint(sys) {
                Ok(true) => return Some(true),
                Ok(false) => answered = true,
                Err(e) => debug!("Awake: LockedHint unavailable: {e:#}"),
            }
        }

        if let Some(sess) = &self.session {
            for (dest, path, iface) in [
                (
                    "org.gnome.ScreenSaver",
                    "/org/gnome/ScreenSaver",
                    "org.gnome.ScreenSaver",
                ),
                (
                    "org.freedesktop.ScreenSaver",
                    "/org/freedesktop/ScreenSaver",
                    "org.freedesktop.ScreenSaver",
                ),
            ] {
                if let Ok(reply) = sess.call_method(Some(dest), path, Some(iface), "GetActive", &())
                    && let Ok(active) = reply.body().deserialize::<bool>()
                {
                    if active {
                        return Some(true);
                    }
                    answered = true;
                    break;
                }
            }
        }

        answered.then_some(false)
    }
}

fn locked_hint(conn: &Connection) -> Result<bool> {
    match locked_hint_at(conn, "/org/freedesktop/login1/session/auto") {
        Ok(v) => Ok(v),
        // A systemd --user service is not part of a session; use the
        // user's graphical session instead.
        Err(_) => {
            let path = display_session_path(conn)?;
            locked_hint_at(conn, &path)
        }
    }
}

fn locked_hint_at(conn: &Connection, path: &str) -> Result<bool> {
    let reply = conn.call_method(
        Some("org.freedesktop.login1"),
        path,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.login1.Session", "LockedHint"),
    )?;
    let value: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
    Ok(bool::try_from(value)?)
}

fn display_session_path(conn: &Connection) -> Result<String> {
    let reply = conn.call_method(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1/user/self",
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.login1.User", "Display"),
    )?;
    let value: zbus::zvariant::OwnedValue = reply.body().deserialize()?;
    let (_id, path): (String, zbus::zvariant::OwnedObjectPath) = value.try_into()?;
    let path = path.as_str().to_string();
    if path == "/" {
        anyhow::bail!("user has no display session");
    }
    Ok(path)
}
