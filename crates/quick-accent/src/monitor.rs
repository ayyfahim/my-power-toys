use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as XiConnectionExt};
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
use x11rb::rust_connection::RustConnection;

use crate::characters;
use crate::config::{ActivationKey, QuickAccentConfig, Trigger};
use crate::usage::UsageCounts;

/// State machine for detecting letter + activation key combos.
#[derive(Debug)]
enum State {
    Idle,
    LetterHeld {
        letter: char,
        keycode: u32,
        chars_typed: u32,
        since: Instant,
    },
    Pending {
        letter: char,
        keycode: u32,
        activation_time: Instant,
        chars_typed: u32,
        trigger: Trigger,
    },
}

pub fn spawn_monitor(config: QuickAccentConfig, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        if let Err(e) = run_monitor(config, stop) {
            warn!("Quick Accent monitor stopped: {e}");
        }
    })
}

fn run_monitor(config: QuickAccentConfig, stop: Arc<AtomicBool>) -> Result<()> {
    let (conn, screen_num) =
        RustConnection::connect(None).context("failed to connect to X11 for Quick Accent")?;
    let root = conn.setup().roots[screen_num].root;

    // Build keycode -> keysym map
    let keymap = build_keymap(&conn)?;

    // Select XI2 raw key events on root window.
    // Raw events bypass grabs, so we see everything.
    // XI_RawKeyPress = bit 13, XI_RawKeyRelease = bit 14
    let mask: u32 = (1 << 13) | (1 << 14);
    let event_mask = xinput::EventMask {
        deviceid: 3, // XIAllMasterDevices
        mask: vec![mask.into()],
    };
    conn.xinput_xi_select_events(root, &[event_mask])?
        .check()
        .context("failed to select XInput2 events")?;
    conn.flush()?;

    info!(
        "Quick Accent monitor active (activation={:?}, delay={}ms, hold={}ms)",
        config.activation_key, config.input_delay_ms, config.hold_duration_ms
    );

    let delay = Duration::from_millis(config.input_delay_ms);
    let hold = Duration::from_millis(config.hold_duration_ms);
    let press_and_hold = config.activation_key == ActivationKey::PressAndHold;
    let mut state = State::Idle;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        // Check pending timeout
        if let State::Pending {
            letter,
            activation_time,
            chars_typed,
            trigger,
            ..
        } = &state
            && activation_time.elapsed() >= delay
        {
            let (letter, ct, trigger) = (*letter, *chars_typed, *trigger);
            state = State::Idle;
            trigger_overlay(&conn, root, letter, ct, trigger, &config);
        }

        // Press-and-hold: the letter alone held long enough.
        if press_and_hold
            && let State::LetterHeld {
                letter,
                chars_typed,
                since,
                ..
            } = &state
            && since.elapsed() >= hold
        {
            let (letter, ct) = (*letter, *chars_typed);
            state = State::Idle;
            trigger_overlay(&conn, root, letter, ct, Trigger::Hold, &config);
        }

        let event = conn.poll_for_event()?;
        let Some(event) = event else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };

        // x11rb with xinput feature parses XI2 events into typed variants
        let (is_press, detail) = match &event {
            x11rb::protocol::Event::XinputRawKeyPress(e) => (true, e.detail),
            x11rb::protocol::Event::XinputRawKeyRelease(e) => (false, e.detail),
            _ => continue,
        };

        let keycode = detail as u8;
        let keysym = keymap.get(&keycode).copied().unwrap_or(0);
        let is_letter = is_alpha_keysym(keysym);
        let is_activation = is_activation_keysym(keysym, config.activation_key);

        state = match state {
            State::Idle => {
                if is_press && is_letter {
                    let letter = keysym_to_char(keysym);
                    if characters::has_accents(letter, &config.languages) {
                        debug!("Quick Accent: letter '{letter}' held (keycode={keycode})");
                        State::LetterHeld {
                            letter,
                            keycode: detail,
                            chars_typed: 1,
                            since: Instant::now(),
                        }
                    } else {
                        State::Idle
                    }
                } else {
                    State::Idle
                }
            }

            State::LetterHeld {
                letter,
                keycode: held_kc,
                chars_typed,
                since,
            } => {
                if !is_press && detail == held_kc {
                    // Letter released -> cancel
                    State::Idle
                } else if is_press && detail == held_kc {
                    // Key repeat
                    State::LetterHeld {
                        letter,
                        keycode: held_kc,
                        chars_typed: chars_typed + 1,
                        since,
                    }
                } else if is_press && is_activation {
                    // Activation key pressed while letter held
                    let extra = if is_space_keysym(keysym) { 1 } else { 0 };
                    let trigger = if keysym == xkeysym::key::Left {
                        Trigger::Left
                    } else if keysym == xkeysym::key::Right {
                        Trigger::Right
                    } else {
                        Trigger::Space
                    };
                    debug!("Quick Accent: activation key pressed, starting delay");
                    State::Pending {
                        letter,
                        keycode: held_kc,
                        activation_time: Instant::now(),
                        chars_typed: chars_typed + extra,
                        trigger,
                    }
                } else if is_press {
                    // Some other key -> cancel
                    State::Idle
                } else {
                    State::LetterHeld {
                        letter,
                        keycode: held_kc,
                        chars_typed,
                        since,
                    }
                }
            }

            State::Pending {
                letter,
                keycode: held_kc,
                activation_time,
                chars_typed,
                trigger,
            } => {
                if !is_press && detail == held_kc {
                    // Letter released before delay -> cancel
                    debug!("Quick Accent: letter released before delay, cancelled");
                    State::Idle
                } else {
                    State::Pending {
                        letter,
                        keycode: held_kc,
                        activation_time,
                        chars_typed,
                        trigger,
                    }
                }
            }
        };
    }

    info!("Quick Accent monitor stopped");
    Ok(())
}

fn trigger_overlay(
    conn: &RustConnection,
    root: u32,
    letter: char,
    backspaces: u32,
    trigger: Trigger,
    config: &QuickAccentConfig,
) {
    let mut accents = characters::accents_for_letter(letter, &config.languages);
    if accents.is_empty() {
        return;
    }

    let active = active_window(conn, root);
    if let Some(win) = active
        && !config.excluded_apps.is_empty()
    {
        let classes = wm_classes(conn, win);
        if config.is_excluded(&classes) {
            info!("Quick Accent: suppressed in excluded app {classes:?}");
            return;
        }
    }
    if config.do_not_activate_on_game_mode {
        if active.is_some_and(|w| is_fullscreen(conn, w)) {
            info!("Quick Accent: suppressed (fullscreen window, game mode)");
            return;
        }
        if feral_gamemode_active() {
            info!("Quick Accent: suppressed (GameMode active)");
            return;
        }
    }

    if config.sort_by_usage_frequency {
        UsageCounts::load().sort(&mut accents);
    }

    let chars_str: String = accents.iter().collect();
    let position = config.toolbar_position.as_arg();

    let bin = crate::find_gui_binary();
    info!("Quick Accent: triggering overlay for '{letter}' (accents={chars_str}, bs={backspaces})");

    let mut cmd = std::process::Command::new(&bin);
    cmd.args([
        "--letter",
        &letter.to_string(),
        "--chars",
        &chars_str,
        "--backspaces",
        &backspaces.to_string(),
        "--position",
        position,
        "--trigger",
        trigger.as_arg(),
    ]);
    if config.start_selection_from_the_left {
        cmd.arg("--start-left");
    }
    if config.show_unicode_description {
        cmd.arg("--describe");
    }
    if config.sort_by_usage_frequency {
        cmd.arg("--record-usage");
    }
    let _ = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn intern(conn: &RustConnection, name: &[u8]) -> Option<u32> {
    conn.intern_atom(false, name)
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

fn active_window(conn: &RustConnection, root: u32) -> Option<u32> {
    let atom = intern(conn, b"_NET_ACTIVE_WINDOW")?;
    let reply = conn
        .get_property(false, root, atom, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next().filter(|w| *w != 0)
}

fn wm_classes(conn: &RustConnection, window: u32) -> Vec<String> {
    let Some(reply) = conn
        .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        .ok()
        .and_then(|c| c.reply().ok())
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&reply.value)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_fullscreen(conn: &RustConnection, window: u32) -> bool {
    let (Some(state), Some(fullscreen)) = (
        intern(conn, b"_NET_WM_STATE"),
        intern(conn, b"_NET_WM_STATE_FULLSCREEN"),
    ) else {
        return false;
    };
    conn.get_property(false, window, state, AtomEnum::ATOM, 0, 64)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|mut v| v.any(|a| a == fullscreen)))
        .unwrap_or(false)
}

/// Parse `gamemoded --status` output.
pub fn parse_gamemode_status(out: &str) -> bool {
    out.contains("is active")
}

/// Whether Feral GameMode reports an active game (`gamemoded --status`).
fn feral_gamemode_active() -> bool {
    std::process::Command::new("gamemoded")
        .arg("--status")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| parse_gamemode_status(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or(false)
}

/// Build a keycode -> primary keysym map from the X11 keyboard mapping.
fn build_keymap(conn: &RustConnection) -> Result<HashMap<u8, u32>> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max.saturating_sub(min).saturating_add(1);

    let reply = conn
        .get_keyboard_mapping(min, count)?
        .reply()
        .context("get_keyboard_mapping failed")?;

    if reply.keysyms_per_keycode == 0 {
        return Ok(HashMap::new());
    }

    let per_kc = reply.keysyms_per_keycode as usize;
    let mut map = HashMap::new();

    for (offset, keysyms) in reply.keysyms.chunks(per_kc).enumerate() {
        let keycode = min.saturating_add(offset as u8);
        if let Some(&ks) = keysyms.first()
            && ks != 0
        {
            map.insert(keycode, ks);
        }
    }

    Ok(map)
}

fn is_alpha_keysym(keysym: u32) -> bool {
    (0x41..=0x5a).contains(&keysym) || (0x61..=0x7a).contains(&keysym)
}

fn keysym_to_char(keysym: u32) -> char {
    let lower = if (0x41..=0x5a).contains(&keysym) {
        keysym + 0x20
    } else {
        keysym
    };
    char::from(lower as u8)
}

fn is_space_keysym(keysym: u32) -> bool {
    keysym == xkeysym::key::space || keysym == xkeysym::key::KP_Space
}

fn is_activation_keysym(keysym: u32, activation: ActivationKey) -> bool {
    match activation {
        ActivationKey::Space => is_space_keysym(keysym),
        ActivationKey::LeftRight => keysym == xkeysym::key::Left || keysym == xkeysym::key::Right,
        ActivationKey::Any => {
            is_space_keysym(keysym) || keysym == xkeysym::key::Left || keysym == xkeysym::key::Right
        }
        // Space/arrows don't activate in press-and-hold; they cancel the hold.
        ActivationKey::PressAndHold => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamemode_status() {
        assert!(parse_gamemode_status("gamemode is active\n"));
        assert!(!parse_gamemode_status("gamemode is inactive\n"));
    }

    #[test]
    fn press_and_hold_has_no_activation_key() {
        assert!(!is_activation_keysym(
            xkeysym::key::space,
            ActivationKey::PressAndHold
        ));
        assert!(is_activation_keysym(xkeysym::key::Left, ActivationKey::Any));
    }
}
