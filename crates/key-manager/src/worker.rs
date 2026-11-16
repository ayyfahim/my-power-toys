//! evdev/uinput input pipeline.
//!
//! The worker grabs every physical keyboard (`EVIOCGRAB`), so the desktop no
//! longer sees its events, and re-emits the [`Engine`] output on a uinput
//! virtual keyboard. This works on X11 and Wayland alike because it sits
//! below the display server.
//!
//! Lifecycle guarantees:
//! - A keyboard is only grabbed when none of its keys are down, so no key
//!   press seen by the desktop loses its release.
//! - When a device disappears, releases for its held keys are fed to the
//!   engine.
//! - On stop (or thread exit/panic) every key still down on the virtual
//!   device is released, devices are ungrabbed and the virtual device is
//!   destroyed.
//! - Hotplug: `/dev/input` is rescanned every few seconds.

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{AttributeSet, Device, EventType, InputEvent, Key, RelativeAxisType};
use tracing::{debug, info, warn};

use crate::actions;
use crate::engine::{Engine, Output, RELEASE, Rules};
use crate::focus::{FocusState, FocusTracker};

pub const VIRTUAL_NAME: &str = "mpt-key-manager virtual keyboard";
const RESCAN: Duration = Duration::from_secs(3);

pub const PERMISSION_HELP: &str = "Key Manager needs read/write access to /dev/input/event* and /dev/uinput. \
Fix: add your user to the 'input' group (`sudo usermod -aG input $USER`), install the udev rule \
crates/key-manager/assets/99-mpt-uinput.rules to /etc/udev/rules.d/ \
(it contains KERNEL==\"uinput\", GROUP=\"input\", MODE=\"0660\"), run \
`sudo udevadm control --reload-rules && sudo udevadm trigger`, then log out and back in.";

pub struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn is_alive(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take()
            && h.join().is_err()
        {
            warn!("Key Manager: input thread panicked");
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Keyboard {
    path: PathBuf,
    dev: Device,
    down: BTreeSet<u16>,
    grabbed: bool,
}

fn is_permission(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::PermissionDenied
}

/// Codes advertised by the virtual keyboard: all keyboard keys and mouse
/// buttons, but no joystick/gamepad/tablet buttons (those would make
/// libinput classify the device as a tablet or joystick).
fn virtual_keys() -> AttributeSet<Key> {
    let mut keys = AttributeSet::<Key>::new();
    let ranges = [(1u16, 0xffu16), (0x110, 0x117), (0x160, 0x2bf)];
    for (lo, hi) in ranges {
        for c in lo..=hi {
            keys.insert(Key::new(c));
        }
    }
    keys
}

fn create_virtual() -> Result<VirtualDevice> {
    let builder = VirtualDeviceBuilder::new().map_err(|e| {
        if is_permission(&e) {
            anyhow::anyhow!("cannot open /dev/uinput: {e}. {PERMISSION_HELP}")
        } else if e.kind() == io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "/dev/uinput not found: load the module with `sudo modprobe uinput` \
                 (add `uinput` to /etc/modules-load.d/ to persist). {PERMISSION_HELP}"
            )
        } else {
            anyhow::anyhow!("cannot open /dev/uinput: {e}")
        }
    })?;
    let mut rel = AttributeSet::<RelativeAxisType>::new();
    for r in [
        RelativeAxisType::REL_X,
        RelativeAxisType::REL_Y,
        RelativeAxisType::REL_WHEEL,
        RelativeAxisType::REL_HWHEEL,
        RelativeAxisType::REL_WHEEL_HI_RES,
        RelativeAxisType::REL_HWHEEL_HI_RES,
    ] {
        rel.insert(r);
    }
    let dev = builder
        .name(VIRTUAL_NAME)
        .with_keys(&virtual_keys())
        .context("uinput: set keys")?
        .with_relative_axes(&rel)
        .context("uinput: set relative axes")?
        .build()
        .context("uinput: create virtual keyboard")?;
    Ok(dev)
}

/// Open `path` if it is a physical keyboard we should grab.
fn open_keyboard(path: &Path) -> io::Result<Option<Device>> {
    let dev = Device::open(path)?;
    if dev.name() == Some(VIRTUAL_NAME) {
        return Ok(None);
    }
    // Letter keycodes are not contiguous, so check each of the 26 letters.
    let is_kbd = dev.supported_keys().is_some_and(|keys| {
        ('a'..='z')
            .filter_map(crate::keymap::char_to_key)
            .all(|(code, _)| keys.contains(Key::new(code)))
    });
    if !is_kbd {
        return Ok(None);
    }
    if dev
        .supported_absolute_axes()
        .is_some_and(|a| a.iter().next().is_some())
    {
        warn!(
            "Key Manager: {} ({}) has absolute axes; not grabbed (its keys are not remapped)",
            path.display(),
            dev.name().unwrap_or("?")
        );
        return Ok(None);
    }
    Ok(Some(dev))
}

fn event_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir("/dev/input")
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("event"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn set_nonblocking(dev: &Device) -> io::Result<()> {
    let fd = dev.as_raw_fd();
    // SAFETY: fcntl on a valid, owned file descriptor.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Start the worker. Fails with a descriptive error when the devices cannot
/// be accessed.
pub fn start(rules: Rules) -> Result<Worker> {
    let vdev = create_virtual()?;

    let mut keyboards = Vec::new();
    let mut ignored = HashSet::new();
    let mut denied = 0usize;
    for path in event_paths() {
        match open_keyboard(&path) {
            Ok(Some(dev)) => keyboards.push(Keyboard {
                path,
                dev,
                down: BTreeSet::new(),
                grabbed: false,
            }),
            Ok(None) => {
                ignored.insert(path);
            }
            Err(e) if is_permission(&e) => denied += 1,
            Err(e) => debug!("Key Manager: skip {}: {e}", path.display()),
        }
    }
    if keyboards.is_empty() {
        if denied > 0 {
            bail!("cannot open input devices in /dev/input (permission denied). {PERMISSION_HELP}");
        }
        bail!("no keyboard devices found in /dev/input");
    }
    if denied > 0 {
        warn!("Key Manager: {denied} input device(s) could not be opened (permission denied)");
    }

    let focus = if rules.has_app_filters() {
        if crate::focus::is_wayland() {
            warn!(
                "Key Manager: app-specific mappings are not supported on Wayland and are skipped"
            );
            None
        } else {
            match FocusTracker::start() {
                Ok(t) => Some(t),
                Err(e) => {
                    warn!(
                        "Key Manager: cannot track focused app ({e:#}); app-specific mappings are skipped"
                    );
                    None
                }
            }
        }
    } else {
        None
    };

    let stop = Arc::new(AtomicBool::new(false));
    let sp = stop.clone();
    let handle = std::thread::Builder::new()
        .name("key-manager-input".into())
        .spawn(move || {
            let mut l = Loop {
                engine: Engine::new(rules),
                vdev,
                keyboards,
                ignored,
                focus,
                focus_gen: 0,
            };
            // Give the display server a moment to pick up the new device.
            std::thread::sleep(Duration::from_millis(200));
            l.run(&sp);
            l.shutdown();
        })
        .context("spawn input thread")?;

    Ok(Worker {
        stop,
        handle: Some(handle),
    })
}

struct Loop {
    engine: Engine,
    vdev: VirtualDevice,
    keyboards: Vec<Keyboard>,
    ignored: HashSet<PathBuf>,
    focus: Option<FocusTracker>,
    focus_gen: u64,
}

impl Loop {
    fn run(&mut self, stop: &AtomicBool) {
        let mut last_scan = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            self.try_grab_pending();
            self.sync_focus();

            let mut fds: Vec<libc::pollfd> = self
                .keyboards
                .iter()
                .filter(|k| k.grabbed)
                .map(|k| libc::pollfd {
                    fd: k.dev.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let pending = self.keyboards.iter().any(|k| !k.grabbed);
            let timeout = if pending { 50 } else { 100 };
            // SAFETY: `fds` is a valid array of pollfd for the duration of the call.
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::Interrupted {
                    warn!("Key Manager: poll failed: {e}");
                    std::thread::sleep(Duration::from_millis(100));
                }
            } else if n > 0 {
                let ready: Vec<(i32, i16)> = fds.iter().map(|p| (p.fd, p.revents)).collect();
                for (fd, rev) in ready {
                    if rev == 0 {
                        continue;
                    }
                    if let Some(i) = self.keyboards.iter().position(|k| k.dev.as_raw_fd() == fd) {
                        self.read_device(i, rev);
                    }
                }
            }

            if last_scan.elapsed() >= RESCAN {
                last_scan = Instant::now();
                self.rescan();
            }
        }
    }

    fn sync_focus(&mut self) {
        let Some(f) = &self.focus else { return };
        let Ok(state) = f.state.lock() else { return };
        let FocusState { ids, generation } = &*state;
        if *generation != self.focus_gen {
            self.focus_gen = *generation;
            let ids = ids.clone();
            drop(state);
            self.engine.set_focused_app(&ids);
        }
    }

    fn try_grab_pending(&mut self) {
        let mut failed = Vec::new();
        for (i, k) in self.keyboards.iter_mut().enumerate() {
            if k.grabbed {
                continue;
            }
            // Wait until all keys are released, otherwise the desktop would
            // never see the release of a key it saw pressed.
            match k.dev.get_key_state() {
                Ok(state) if state.iter().next().is_none() => {}
                Ok(_) => continue,
                Err(e) => {
                    warn!("Key Manager: {}: {e}", k.path.display());
                    failed.push(i);
                    continue;
                }
            }
            let res = set_nonblocking(&k.dev).and_then(|_| k.dev.grab());
            match res {
                Ok(()) => {
                    k.grabbed = true;
                    // Drain anything queued before the grab.
                    while k
                        .dev
                        .fetch_events()
                        .map(|it| it.count())
                        .is_ok_and(|n| n > 0)
                    {}
                    info!(
                        "Key Manager: grabbed {} ({})",
                        k.path.display(),
                        k.dev.name().unwrap_or("?")
                    );
                }
                Err(e) => {
                    warn!("Key Manager: cannot grab {}: {e}", k.path.display());
                    failed.push(i);
                }
            }
        }
        for i in failed.into_iter().rev() {
            let k = self.keyboards.remove(i);
            self.ignored.insert(k.path);
        }
    }

    fn read_device(&mut self, idx: usize, revents: i16) {
        let mut events: Vec<InputEvent> = Vec::new();
        let mut gone = revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0;
        if !gone {
            match self.keyboards[idx].dev.fetch_events() {
                Ok(it) => events.extend(it),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    debug!("Key Manager: read error: {e}");
                    gone = true;
                }
            }
        }
        for ev in events {
            match ev.event_type() {
                EventType::KEY => {
                    let (code, value) = (ev.code(), ev.value());
                    let k = &mut self.keyboards[idx];
                    match value {
                        1 => {
                            k.down.insert(code);
                        }
                        0 => {
                            k.down.remove(&code);
                        }
                        _ => {}
                    }
                    let out = self.engine.process(code, value);
                    self.apply(out);
                }
                EventType::RELATIVE => self.write(&[ev]),
                _ => {}
            }
        }
        if gone {
            let k = self.keyboards.remove(idx);
            info!("Key Manager: {} removed", k.path.display());
            for code in k.down {
                let out = self.engine.process(code, RELEASE);
                self.apply(out);
            }
        }
    }

    fn write(&mut self, evs: &[InputEvent]) {
        if let Err(e) = self.vdev.emit(evs) {
            warn!("Key Manager: uinput write failed: {e}");
        }
    }

    fn apply(&mut self, out: Vec<Output>) {
        for o in out {
            match o {
                Output::Key { code, value } => {
                    self.write(&[InputEvent::new(EventType::KEY, code, value)]);
                }
                Output::Action(a) => actions::run(&a),
            }
        }
    }

    fn rescan(&mut self) {
        let paths = event_paths();
        let present: HashSet<&PathBuf> = paths.iter().collect();
        self.ignored.retain(|p| present.contains(p));
        for path in &paths {
            if self.ignored.contains(path) || self.keyboards.iter().any(|k| &k.path == path) {
                continue;
            }
            match open_keyboard(path) {
                Ok(Some(dev)) => {
                    info!("Key Manager: new keyboard {}", path.display());
                    self.keyboards.push(Keyboard {
                        path: path.clone(),
                        dev,
                        down: BTreeSet::new(),
                        grabbed: false,
                    });
                }
                Ok(None) => {
                    self.ignored.insert(path.clone());
                }
                Err(e) => {
                    debug!("Key Manager: skip {}: {e}", path.display());
                    self.ignored.insert(path.clone());
                }
            }
        }
    }

    fn shutdown(&mut self) {
        let out = self.engine.release_all();
        self.apply(out);
        for k in &mut self.keyboards {
            if k.grabbed {
                let _ = k.dev.ungrab();
            }
        }
        self.keyboards.clear();
        self.focus = None;
        info!("Key Manager: input pipeline stopped");
    }
}

impl Drop for Loop {
    fn drop(&mut self) {
        // Runs on panic too: ungrab before the fds close (closing also
        // releases the grab; this is belt and braces).
        for k in &mut self.keyboards {
            if k.grabbed {
                let _ = k.dev.ungrab();
            }
        }
    }
}
