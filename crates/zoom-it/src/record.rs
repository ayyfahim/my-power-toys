//! Screen recording (upstream `RecordToggleKey`) by driving `wf-recorder`
//! (Wayland) or `ffmpeg -f x11grab` (X11) as a child process.
//!
//! The recorder PID and output path are kept in a runtime file so a toggle
//! still works after the daemon restarts.

use crate::config::ZoomItConfig;
use crate::screen::{self, MonitorGeom, SessionType};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderTool {
    WfRecorder,
    Ffmpeg,
}

#[derive(Debug)]
pub enum ToggleResult {
    Started(PathBuf),
    Stopped(PathBuf),
}

/// Recorder state owned by the module.
#[derive(Default)]
pub struct Recorder {
    child: Option<Child>,
}

impl Recorder {
    pub fn is_recording(&mut self) -> bool {
        if let Some(child) = self.child.as_mut()
            && matches!(child.try_wait(), Ok(None))
        {
            return true;
        }
        self.child = None;
        read_state().is_some_and(|(pid, _)| pid_alive(pid))
    }

    pub fn toggle(&mut self, config: &ZoomItConfig) -> Result<ToggleResult> {
        if self.is_recording() {
            self.stop()
        } else {
            self.start(config)
        }
    }

    pub fn start(&mut self, config: &ZoomItConfig) -> Result<ToggleResult> {
        let session = screen::detect_session();
        let tool = pick_tool(session).context(
            "no screen recorder found (install wf-recorder on Wayland or ffmpeg on X11)",
        )?;
        let (monitor, _) = screen::monitor_under_cursor()?;
        let dir = config.recording_dir();
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let out = dir.join(format!("ZoomIt-{ts}.mp4"));
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        let (cmd, args) = build_command(tool, &monitor, config, &display, &out);
        info!("Starting recording: {cmd} {}", args.join(" "));
        let child = Command::new(&cmd)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {cmd}"))?;
        write_state(child.id(), &out)?;
        self.child = Some(child);
        Ok(ToggleResult::Started(out))
    }

    pub fn stop(&mut self) -> Result<ToggleResult> {
        let state = read_state();
        let pid = self
            .child
            .as_ref()
            .map(|c| c.id())
            .or(state.as_ref().map(|(p, _)| *p))
            .context("not recording")?;
        let out = state.map(|(_, p)| p).unwrap_or_default();
        // SIGINT lets both ffmpeg and wf-recorder finalize the MP4 container.
        let _ = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(5);
        if let Some(mut child) = self.child.take() {
            loop {
                if !matches!(child.try_wait(), Ok(None)) {
                    break;
                }
                if Instant::now() > deadline {
                    warn!("Recorder did not exit after SIGINT, killing it");
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        } else {
            while pid_alive(pid) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        let _ = std::fs::remove_file(state_path());
        info!("Recording stopped: {}", out.display());
        Ok(ToggleResult::Stopped(out))
    }
}

fn pick_tool(session: SessionType) -> Option<RecorderTool> {
    match session {
        SessionType::Wayland => in_path("wf-recorder").then_some(RecorderTool::WfRecorder),
        _ => in_path("ffmpeg").then_some(RecorderTool::Ffmpeg),
    }
}

/// Build the recorder command line for `monitor`.
pub fn build_command(
    tool: RecorderTool,
    monitor: &MonitorGeom,
    config: &ZoomItConfig,
    display: &str,
    out: &Path,
) -> (String, Vec<String>) {
    let fps = config.record_frame_rate.clamp(1, 120).to_string();
    let scale = config.record_scaling.clamp(10, 100);
    let scale_filter = (scale != 100).then(|| {
        let f = scale as f32 / 100.0;
        format!("scale=trunc(iw*{f}/2)*2:trunc(ih*{f}/2)*2")
    });
    let out = out.to_string_lossy().to_string();
    match tool {
        RecorderTool::WfRecorder => {
            let mut a = vec![
                "-g".into(),
                format!(
                    "{},{} {}x{}",
                    monitor.x, monitor.y, monitor.width, monitor.height
                ),
                "-r".into(),
                fps,
            ];
            if config.capture_audio {
                a.push("--audio".into());
            }
            if let Some(f) = scale_filter {
                a.push("-F".into());
                a.push(f);
            }
            a.push("-f".into());
            a.push(out);
            ("wf-recorder".into(), a)
        }
        RecorderTool::Ffmpeg => {
            let mut a: Vec<String> = [
                "-y",
                "-loglevel",
                "error",
                "-f",
                "x11grab",
                "-framerate",
                &fps,
                "-video_size",
                &format!("{}x{}", monitor.width, monitor.height),
                "-i",
                &format!("{display}+{},{}", monitor.x, monitor.y),
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            if config.capture_audio {
                a.extend(["-f", "pulse", "-i", "default"].map(String::from));
            }
            if let Some(f) = scale_filter {
                a.push("-vf".into());
                a.push(f);
            }
            a.extend(
                [
                    "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p",
                ]
                .map(String::from),
            );
            if config.capture_audio {
                a.extend(["-c:a", "aac"].map(String::from));
            }
            a.push(out);
            ("ffmpeg".into(), a)
        }
    }
}

fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn state_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("mpt-zoom-it-record.pid")
}

fn write_state(pid: u32, out: &Path) -> Result<()> {
    std::fs::write(state_path(), format!("{pid}\n{}\n", out.display()))
        .context("write recorder state")
}

fn read_state() -> Option<(u32, PathBuf)> {
    let s = std::fs::read_to_string(state_path()).ok()?;
    parse_state(&s)
}

fn parse_state(s: &str) -> Option<(u32, PathBuf)> {
    let mut lines = s.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let path = lines.next().map(PathBuf::from).unwrap_or_default();
    Some((pid, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon() -> MonitorGeom {
        MonitorGeom {
            name: "DP-1".into(),
            x: 1920,
            y: 0,
            width: 2560,
            height: 1440,
        }
    }

    #[test]
    fn ffmpeg_command() {
        let cfg = ZoomItConfig::default();
        let (cmd, args) = build_command(
            RecorderTool::Ffmpeg,
            &mon(),
            &cfg,
            ":1",
            Path::new("/tmp/a.mp4"),
        );
        assert_eq!(cmd, "ffmpeg");
        let joined = args.join(" ");
        assert!(joined.contains("-video_size 2560x1440"));
        assert!(joined.contains("-i :1+1920,0"));
        assert!(joined.contains("-framerate 30"));
        assert!(!joined.contains("pulse"));
        assert!(!joined.contains("-vf"));
        assert_eq!(args.last().unwrap(), "/tmp/a.mp4");
    }

    #[test]
    fn wf_recorder_command_with_audio_and_scale() {
        let cfg = ZoomItConfig {
            capture_audio: true,
            record_scaling: 50,
            ..Default::default()
        };
        let (cmd, args) = build_command(
            RecorderTool::WfRecorder,
            &mon(),
            &cfg,
            ":0",
            Path::new("/tmp/b.mp4"),
        );
        assert_eq!(cmd, "wf-recorder");
        assert_eq!(args[1], "1920,0 2560x1440");
        assert!(args.contains(&"--audio".to_string()));
        assert!(args.iter().any(|a| a.starts_with("scale=")));
        assert_eq!(args[args.len() - 2..], ["-f", "/tmp/b.mp4"]);
    }

    #[test]
    fn state_parsing() {
        assert_eq!(
            parse_state("123\n/home/u/Videos/x.mp4\n"),
            Some((123, PathBuf::from("/home/u/Videos/x.mp4")))
        );
        assert_eq!(parse_state("abc"), None);
    }
}
