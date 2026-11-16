//! Desktop theme switching for GNOME, KDE Plasma, XFCE, Cinnamon and MATE.
//!
//! The freedesktop portal `color-scheme` setting is read-only, so each
//! desktop is driven through its own settings tool.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{debug, info, warn};

use crate::config::{DesktopKind, LightSwitchConfig, ModeSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    Gnome,
    Kde,
    Xfce,
    Cinnamon,
    Mate,
}

/// Map `XDG_CURRENT_DESKTOP` / `DESKTOP_SESSION` to a backend. GNOME-like
/// desktops (Unity, Budgie, Pantheon, ...) and unknown ones use GNOME
/// gsettings keys.
pub fn detect_desktop(xdg_current_desktop: Option<&str>, desktop_session: Option<&str>) -> Desktop {
    let joined = format!(
        "{}:{}",
        xdg_current_desktop.unwrap_or_default(),
        desktop_session.unwrap_or_default()
    )
    .to_lowercase();
    for part in joined.split([':', ';']) {
        let part = part.trim();
        if part.contains("kde") || part.contains("plasma") {
            return Desktop::Kde;
        }
        if part.contains("xfce") {
            return Desktop::Xfce;
        }
        if part.contains("cinnamon") {
            return Desktop::Cinnamon;
        }
        if part == "mate" || part.starts_with("mate") {
            return Desktop::Mate;
        }
    }
    Desktop::Gnome
}

pub fn resolve_desktop(kind: DesktopKind) -> Desktop {
    match kind {
        DesktopKind::Gnome => Desktop::Gnome,
        DesktopKind::Kde => Desktop::Kde,
        DesktopKind::Xfce => Desktop::Xfce,
        DesktopKind::Cinnamon => Desktop::Cinnamon,
        DesktopKind::Mate => Desktop::Mate,
        DesktopKind::Auto => detect_desktop(
            std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
            std::env::var("DESKTOP_SESSION").ok().as_deref(),
        ),
    }
}

// ── command helpers ──────────────────────────────────────────────────────

fn run(program: &str, args: &[&str]) -> bool {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(s) if s.success() => {
            debug!("Light Switch: {program} {args:?} ok");
            true
        }
        Ok(s) => {
            warn!("Light Switch: {program} {args:?} exited with {s}");
            false
        }
        Err(e) => {
            warn!("Light Switch: failed to run {program}: {e}");
            false
        }
    }
}

/// Like [`run`] but silent on failure (for optional keys).
fn run_quiet(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    output("gsettings", &["get", schema, key])
}

fn gsettings_set(schema: &str, key: &str, value: &str) -> bool {
    run("gsettings", &["set", schema, key, value])
}

fn looks_dark(value: &str) -> bool {
    value.to_lowercase().contains("dark")
}

// ── reading ──────────────────────────────────────────────────────────────

/// Read the current theme of the desktop.
pub fn get_current_theme(desktop: Desktop) -> SystemTheme {
    let dark = match desktop {
        Desktop::Gnome => gsettings_get("org.gnome.desktop.interface", "color-scheme")
            .is_some_and(|v| looks_dark(&v)),
        Desktop::Kde => ["kreadconfig6", "kreadconfig5"]
            .iter()
            .find_map(|tool| output(tool, &["--group", "General", "--key", "ColorScheme"]))
            .or_else(|| gsettings_get("org.gnome.desktop.interface", "color-scheme"))
            .is_some_and(|v| looks_dark(&v)),
        Desktop::Xfce => output("xfconf-query", &["-c", "xsettings", "-p", "/Net/ThemeName"])
            .is_some_and(|v| looks_dark(&v)),
        Desktop::Cinnamon => gsettings_get("org.cinnamon.desktop.interface", "gtk-theme")
            .is_some_and(|v| looks_dark(&v)),
        Desktop::Mate => {
            gsettings_get("org.mate.interface", "gtk-theme").is_some_and(|v| looks_dark(&v))
        }
    };
    if dark {
        SystemTheme::Dark
    } else {
        SystemTheme::Light
    }
}

/// Parse `gdbus call ... Properties.Get` output such as `(<true>,)`.
pub fn parse_gdbus_bool(out: &str) -> Option<bool> {
    if out.contains("true") {
        Some(true)
    } else if out.contains("false") {
        Some(false)
    } else {
        None
    }
}

/// Whether the desktop's night light is currently active (GNOME
/// `org.gnome.SettingsDaemon.Color.NightLightActive`, KDE Plasma 6
/// `org.kde.KWin.NightLight.running`). `None` if unavailable.
pub fn night_light_active() -> Option<bool> {
    let probes: [(&str, &str, &str, &str); 2] = [
        (
            "org.gnome.SettingsDaemon.Color",
            "/org/gnome/SettingsDaemon/Color",
            "org.gnome.SettingsDaemon.Color",
            "NightLightActive",
        ),
        (
            "org.kde.KWin.NightLight",
            "/org/kde/KWin/NightLight",
            "org.kde.KWin.NightLight",
            "running",
        ),
    ];
    probes.iter().find_map(|(dest, path, iface, prop)| {
        output(
            "gdbus",
            &[
                "call",
                "--session",
                "--dest",
                dest,
                "--object-path",
                path,
                "--method",
                "org.freedesktop.DBus.Properties.Get",
                iface,
                prop,
            ],
        )
        .and_then(|o| parse_gdbus_bool(&o))
    })
}

// ── writing ──────────────────────────────────────────────────────────────

/// Apply light or dark mode according to `config`.
pub fn set_theme(dark: bool, config: &LightSwitchConfig) {
    let desktop = resolve_desktop(config.desktop);
    let mode = config.mode(dark);
    info!(
        "Light Switch: switching to {} mode ({desktop:?})",
        if dark { "dark" } else { "light" }
    );

    match desktop {
        Desktop::Gnome => apply_gnome(&mode, config),
        Desktop::Kde => apply_kde(&mode, config),
        Desktop::Xfce => apply_xfce(&mode, config),
        Desktop::Cinnamon => apply_cinnamon(&mode, config),
        Desktop::Mate => apply_mate(&mode, config),
    }

    if let Some(path) = mode.wallpaper {
        set_wallpaper(desktop, path);
    }
    if let Some(profile) = mode.power_display_profile {
        apply_power_display_profile(profile);
    }
}

fn color_scheme(dark: bool) -> &'static str {
    if dark { "prefer-dark" } else { "prefer-light" }
}

fn apply_gnome(mode: &ModeSettings<'_>, config: &LightSwitchConfig) {
    if config.apply_system || config.apply_apps {
        gsettings_set(
            "org.gnome.desktop.interface",
            "color-scheme",
            color_scheme(mode.dark),
        );
    }
    if config.apply_system && config.switch_gtk_theme && !mode.gtk_theme.is_empty() {
        gsettings_set("org.gnome.desktop.interface", "gtk-theme", mode.gtk_theme);
    }
}

fn apply_kde(mode: &ModeSettings<'_>, config: &LightSwitchConfig) {
    if config.apply_system || config.apply_apps {
        if let Some(lnf) = mode.kde_look_and_feel {
            if !run("lookandfeeltool", &["-a", lnf]) {
                warn!("Light Switch: lookandfeeltool failed; is plasma-workspace installed?");
            }
        } else if !mode.kde_color_scheme.is_empty()
            && !run("plasma-apply-colorscheme", &[mode.kde_color_scheme])
        {
            warn!("Light Switch: plasma-apply-colorscheme failed; is plasma-workspace installed?");
        }
        // GTK apps under Plasma read this through xdg-desktop-portal-gtk.
        run_quiet(
            "gsettings",
            &[
                "set",
                "org.gnome.desktop.interface",
                "color-scheme",
                color_scheme(mode.dark),
            ],
        );
    }
    if config.apply_system && config.switch_gtk_theme && !mode.gtk_theme.is_empty() {
        run_quiet(
            "gsettings",
            &[
                "set",
                "org.gnome.desktop.interface",
                "gtk-theme",
                mode.gtk_theme,
            ],
        );
    }
}

fn apply_xfce(mode: &ModeSettings<'_>, config: &LightSwitchConfig) {
    if (config.apply_system || config.apply_apps) && !mode.gtk_theme.is_empty() {
        run(
            "xfconf-query",
            &[
                "-c",
                "xsettings",
                "-p",
                "/Net/ThemeName",
                "-s",
                mode.gtk_theme,
            ],
        );
    }
    if config.apply_apps {
        run_quiet(
            "gsettings",
            &[
                "set",
                "org.gnome.desktop.interface",
                "color-scheme",
                color_scheme(mode.dark),
            ],
        );
    }
}

fn apply_cinnamon(mode: &ModeSettings<'_>, config: &LightSwitchConfig) {
    if config.apply_apps || config.apply_system {
        // Cinnamon 6+ portal preference; older versions lack the schema.
        run_quiet(
            "gsettings",
            &[
                "set",
                "org.x.apps.portal",
                "color-scheme",
                color_scheme(mode.dark),
            ],
        );
        run_quiet(
            "gsettings",
            &[
                "set",
                "org.gnome.desktop.interface",
                "color-scheme",
                color_scheme(mode.dark),
            ],
        );
    }
    if config.apply_system && config.switch_gtk_theme && !mode.gtk_theme.is_empty() {
        gsettings_set(
            "org.cinnamon.desktop.interface",
            "gtk-theme",
            mode.gtk_theme,
        );
    }
}

fn apply_mate(mode: &ModeSettings<'_>, config: &LightSwitchConfig) {
    if config.apply_apps {
        run_quiet(
            "gsettings",
            &[
                "set",
                "org.gnome.desktop.interface",
                "color-scheme",
                color_scheme(mode.dark),
            ],
        );
    }
    if (config.apply_system || config.apply_apps)
        && config.switch_gtk_theme
        && !mode.gtk_theme.is_empty()
    {
        gsettings_set("org.mate.interface", "gtk-theme", mode.gtk_theme);
    }
}

/// `file://` URI for a path (percent-encoding the characters that matter
/// for gsettings URIs).
pub fn file_uri(path: &str) -> String {
    let mut out = String::from("file://");
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return Path::new(&home).join(rest);
    }
    PathBuf::from(path)
}

fn set_wallpaper(desktop: Desktop, path: &str) {
    let path_buf = expand_home(path);
    if !path_buf.exists() {
        warn!("Light Switch: wallpaper {path_buf:?} not found, skipping");
        return;
    }
    let path = path_buf.to_string_lossy().to_string();
    let uri = file_uri(&path);
    match desktop {
        Desktop::Gnome => {
            gsettings_set("org.gnome.desktop.background", "picture-uri", &uri);
            // GNOME 42+ uses a separate key in dark mode.
            run_quiet(
                "gsettings",
                &[
                    "set",
                    "org.gnome.desktop.background",
                    "picture-uri-dark",
                    &uri,
                ],
            );
        }
        Desktop::Kde => {
            run("plasma-apply-wallpaperimage", &[&path]);
        }
        Desktop::Xfce => {
            let props = output("xfconf-query", &["-c", "xfce4-desktop", "-l"]).unwrap_or_default();
            let mut any = false;
            for prop in props.lines().filter(|p| p.ends_with("/last-image")) {
                any |= run(
                    "xfconf-query",
                    &["-c", "xfce4-desktop", "-p", prop, "-s", &path],
                );
            }
            if !any {
                warn!("Light Switch: no XFCE backdrop last-image property found");
            }
        }
        Desktop::Cinnamon => {
            gsettings_set("org.cinnamon.desktop.background", "picture-uri", &uri);
        }
        Desktop::Mate => {
            gsettings_set("org.mate.background", "picture-filename", &path);
        }
    }
}

fn find_power_display_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-power-display");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-power-display".into()
}

fn apply_power_display_profile(profile: &str) {
    let bin = find_power_display_binary();
    info!("Light Switch: applying Power Display profile {profile:?}");
    match Command::new(&bin)
        .args(["--apply-profile", profile])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // Monitor DDC/CI writes can take seconds; reap in the background.
        Ok(mut child) => {
            std::thread::spawn(move || {
                if let Ok(status) = child.wait()
                    && !status.success()
                {
                    warn!("Light Switch: mpt-power-display --apply-profile exited with {status}");
                }
            });
        }
        Err(e) => warn!("Light Switch: failed to run {bin:?}: {e}"),
    }
}

/// Toggle the current theme and return the new state.
pub fn toggle_theme(config: &LightSwitchConfig) -> SystemTheme {
    let current = get_current_theme(resolve_desktop(config.desktop));
    let go_dark = current == SystemTheme::Light;
    set_theme(go_dark, config);
    if go_dark {
        SystemTheme::Dark
    } else {
        SystemTheme::Light
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_detection() {
        assert_eq!(detect_desktop(Some("KDE"), None), Desktop::Kde);
        assert_eq!(detect_desktop(Some("XFCE"), None), Desktop::Xfce);
        assert_eq!(detect_desktop(Some("X-Cinnamon"), None), Desktop::Cinnamon);
        assert_eq!(detect_desktop(Some("MATE"), None), Desktop::Mate);
        assert_eq!(detect_desktop(Some("ubuntu:GNOME"), None), Desktop::Gnome);
        assert_eq!(detect_desktop(None, Some("plasma")), Desktop::Kde);
        assert_eq!(detect_desktop(None, None), Desktop::Gnome);
        // "mate" must not match inside other words.
        assert_eq!(detect_desktop(Some("Ultimate"), None), Desktop::Gnome);
    }

    #[test]
    fn gdbus_bool_parsing() {
        assert_eq!(parse_gdbus_bool("(<true>,)"), Some(true));
        assert_eq!(parse_gdbus_bool("(<false>,)"), Some(false));
        assert_eq!(parse_gdbus_bool(""), None);
    }

    #[test]
    fn file_uri_encodes_spaces() {
        assert_eq!(
            file_uri("/home/u/My Pics/a.png"),
            "file:///home/u/My%20Pics/a.png"
        );
    }
}
