//! Pure-Rust headless Chromium (CDP) storyboard recorder for ARUS Studio.
//!
//! Zero Python, zero Playwright, zero `ffmpeg`. Drives a local Chromium binary via Chrome
//! DevTools Protocol over a lightweight RFC 6455 WebSocket client and encodes captured frames
//! directly into a faststart H.264 `.mp4` via `vidr` + `containr`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::video;

/// CLI arguments for `arus-worker record`.
pub struct RecordArgs {
    pub spec: PathBuf,
    pub out: PathBuf,
    pub url: Option<String>,
    pub port: u16,
    pub resolution: Option<String>,
    pub fps: u32,
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Serialize)]
struct StepLog {
    step: usize,
    action: String,
    elapsed_s: f64,
    frames_captured: usize,
    state: Value,
}

#[derive(Serialize)]
struct RecordSummary {
    name: String,
    spec: String,
    resolution: String,
    fps: u32,
    video_mp4: Option<String>,
    frames_encoded: u64,
    screenshots: Vec<String>,
    steps: Vec<StepLog>,
}

/// Executes a declarative JSON storyboard in headless Chromium and encodes directly to `.mp4`.
pub fn run_record(args: RecordArgs) -> Result<(), String> {
    let raw = fs::read_to_string(&args.spec)
        .map_err(|e| format!("read storyboard {}: {e}", args.spec.display()))?;
    let spec: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("parse storyboard {}: {e}", args.spec.display()))?;

    fs::create_dir_all(&args.out)
        .map_err(|e| format!("create output dir {}: {e}", args.out.display()))?;

    let (width, height, res_label) = resolve_dimensions(&spec, args.resolution.as_deref());
    let fps = args.fps.max(1);

    let _api_guard;
    let _archive_tmp;
    let mut base_url = if let Some(u) = args.url {
        _api_guard = None;
        _archive_tmp = None;
        u
    } else {
        let tmp = tempfile::tempdir().map_err(|e| format!("tempdir failed: {e}"))?;
        let api_bin = find_arus_api_binary()?;
        let child = Command::new(&api_bin)
            .args([
                "--archive",
                &tmp.path().to_string_lossy(),
                "--bind",
                &format!("127.0.0.1:{}", args.port),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn {} failed: {e}", api_bin.display()))?;
        _api_guard = Some(ChildGuard(child));
        _archive_tmp = Some(tmp);
        wait_for_tcp("127.0.0.1", args.port, Duration::from_secs(10))?;
        format!("http://127.0.0.1:{}/", args.port)
    };

    if let Some(init_obj) = spec.get("init").and_then(Value::as_object) {
        let qs = encode_init_query(init_obj);
        if !qs.is_empty() {
            let sep = if base_url.contains('?') { '&' } else { '?' };
            base_url.push(sep);
            base_url.push_str(&qs);
        }
    }

    let chrome_bin = find_chromium_binary()?;
    let user_data = tempfile::tempdir().map_err(|e| format!("chrome tempdir failed: {e}"))?;
    eprintln!("[record] launching chromium: {}", chrome_bin.display());
    let (mut chrome_child, cdp_port) =
        spawn_headless_chromium(&chrome_bin, user_data.path(), width, height)?;
    let _chrome_guard = ChildGuard(
        chrome_child
            .take()
            .ok_or_else(|| "missing chrome handle".to_owned())?,
    );

    eprintln!("[record] chromium CDP ready on 127.0.0.1:{cdp_port}");
    let ws_path = open_cdp_page_target(cdp_port)?;
    let mut cdp = CdpClient::connect("127.0.0.1", cdp_port, &ws_path)?;

    cdp.call("Page.enable", json!({}))?;
    cdp.call("Runtime.enable", json!({}))?;
    cdp.call(
        "Emulation.setDeviceMetricsOverride",
        json!({
            "width": width,
            "height": height,
            "deviceScaleFactor": 1,
            "mobile": false
        }),
    )?;
    eprintln!("[record] navigating to {base_url}");
    cdp.call("Page.navigate", json!({ "url": base_url }))?;

    wait_for_js_truthy(
        &mut cdp,
        "Boolean(window.arusDirector && window.arusDirector.isReady())",
        Duration::from_secs(35),
    )?;
    eprintln!("[record] studio ready; running setup_steps");

    if spec
        .get("presentation")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        cdp.eval("window.arusDirector.setPresentationMode(true)")?;
    }

    if let Some(setup_steps) = spec.get("setup_steps").and_then(Value::as_array) {
        for step in setup_steps {
            apply_instant_action(&mut cdp, step)?;
        }
    }

    let _ = wait_for_js_truthy(
        &mut cdp,
        "Boolean(window.arusDirector && window.arusDirector.isSettled())",
        Duration::from_secs(12),
    );
    thread::sleep(Duration::from_millis(900));

    let mut frames_png: Vec<Vec<u8>> = Vec::new();
    let mut saved_shots: Vec<String> = Vec::new();
    let mut step_logs: Vec<StepLog> = Vec::new();

    let steps = spec
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    for (idx, step) in steps.iter().enumerate() {
        let step_num = idx + 1;
        let action = step
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing `action` at step {step_num}"))?
            .to_owned();
        let t0 = Instant::now();
        let before_frames = frames_png.len();

        match action.as_str() {
            "screenshot" => {
                let fname = step
                    .get("file")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("shot-{step_num:02}.png"));
                let png = capture_png(&mut cdp)?;
                fs::write(args.out.join(&fname), &png)
                    .map_err(|e| format!("write screenshot {fname} failed: {e}"))?;
                saved_shots.push(fname);
            }
            "wait" => {
                let secs = step.get("seconds").and_then(Value::as_f64).unwrap_or(1.0);
                let count = ((secs * f64::from(fps)).round() as usize).max(1);
                let png = capture_png(&mut cdp)?;
                for _ in 0..count {
                    frames_png.push(png.clone());
                }
            }
            "play" => {
                let secs = step.get("seconds").and_then(Value::as_f64).unwrap_or(3.0);
                let total_out_frames = ((secs * f64::from(fps)).round() as usize).max(2);
                let state = cdp.eval_json("window.arusDirector.getState()")?;
                let frame_count = state
                    .get("frameCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(48) as usize;
                let start_idx = state.get("frameIdx").and_then(Value::as_u64).unwrap_or(0) as usize;
                let end_idx = frame_count.saturating_sub(1);
                let span = end_idx.saturating_sub(start_idx).max(1);

                let mut last_sim_idx: Option<usize> = None;
                let mut last_png: Option<Vec<u8>> = None;
                for out_i in 0..total_out_frames {
                    let frac = (out_i as f64) / ((total_out_frames - 1).max(1) as f64);
                    let sim_idx = (start_idx + (frac * span as f64).round() as usize).min(end_idx);
                    if last_sim_idx != Some(sim_idx) {
                        cdp.eval(&format!("window.arusDirector.setFrame({sim_idx})"))?;
                        thread::sleep(Duration::from_millis(35));
                        let png = capture_png(&mut cdp)?;
                        last_sim_idx = Some(sim_idx);
                        last_png = Some(png);
                    }
                    if let Some(png) = &last_png {
                        frames_png.push(png.clone());
                    }
                }
            }
            "set_frame" => {
                let frame = step.get("frame").and_then(Value::as_u64).unwrap_or(0);
                cdp.eval(&format!("window.arusDirector.setFrame({frame})"))?;
                thread::sleep(Duration::from_millis(60));
                let wait_s = step.get("wait_s").and_then(Value::as_f64).unwrap_or(0.0);
                if wait_s > 0.0 {
                    let count = ((wait_s * f64::from(fps)).round() as usize).max(1);
                    let png = capture_png(&mut cdp)?;
                    for _ in 0..count {
                        frames_png.push(png.clone());
                    }
                }
            }
            _ => {
                apply_instant_action(&mut cdp, step)?;
                let wait_s = step.get("wait_s").and_then(Value::as_f64).unwrap_or(0.5);
                if wait_s > 0.0 {
                    thread::sleep(Duration::from_millis((wait_s * 1000.0) as u64));
                    let png = capture_png(&mut cdp)?;
                    frames_png.push(png);
                }
            }
        }

        let state = cdp.eval_json("window.arusDirector.getState()")?;
        step_logs.push(StepLog {
            step: step_num,
            action,
            elapsed_s: (t0.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
            frames_captured: frames_png.len().saturating_sub(before_frames),
            state,
        });
    }

    let stem = spec
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("recording")
        .to_owned();
    let mut video_mp4 = None;
    let mut frames_encoded = 0u64;
    if !frames_png.is_empty()
        && spec
            .get("export_mp4")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    {
        let mp4_name = format!("{stem}.mp4");
        let mp4_path = args.out.join(&mp4_name);
        frames_encoded = video::encode_png_frames_to_mp4(&frames_png, fps, &mp4_path)?;
        video_mp4 = Some(mp4_name);
    }

    let summary = RecordSummary {
        name: stem,
        spec: args.spec.display().to_string(),
        resolution: res_label,
        fps,
        video_mp4,
        frames_encoded,
        screenshots: saved_shots,
        steps: step_logs,
    };
    let summary_json = serde_json::to_string_pretty(&summary)
        .map_err(|e| format!("serialize summary failed: {e}"))?;
    fs::write(args.out.join("summary.json"), &summary_json)
        .map_err(|e| format!("write summary.json failed: {e}"))?;
    println!("{summary_json}");
    Ok(())
}

fn apply_instant_action(cdp: &mut CdpClient, step: &Value) -> Result<(), String> {
    let action = step
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match action {
        "caption" => {
            let text = step.get("text").and_then(Value::as_str).unwrap_or("");
            let js_arg = serde_json::to_string(text).map_err(|e| e.to_string())?;
            cdp.eval(&format!("window.arusDirector.setCaption({js_arg})"))?;
        }
        "set_outlook" => {
            let mut obj = step.as_object().cloned().unwrap_or_default();
            obj.remove("action");
            let arg = if obj.is_empty() {
                "null".to_owned()
            } else {
                serde_json::to_string(&Value::Object(obj)).map_err(|e| e.to_string())?
            };
            cdp.eval(&format!("window.arusDirector.setPublicOutlook({arg})"))?;
        }
        "set_frame" => {
            let frame = step.get("frame").and_then(Value::as_u64).unwrap_or(0);
            cdp.eval(&format!("window.arusDirector.setFrame({frame})"))?;
        }
        "set_layer" => {
            let layer = step.get("layer").and_then(Value::as_str).unwrap_or("depth");
            let js_arg = serde_json::to_string(layer).map_err(|e| e.to_string())?;
            cdp.eval(&format!("window.arusDirector.setLayer({js_arg})"))?;
        }
        "set_basemap" => {
            let mode = step.get("mode").and_then(Value::as_str).unwrap_or("topo");
            let js_arg = serde_json::to_string(mode).map_err(|e| e.to_string())?;
            cdp.eval(&format!("window.arusDirector.setBasemap({js_arg})"))?;
        }
        "fly_to" | "jump_to" => {
            let lat = step.get("lat").and_then(Value::as_f64).unwrap_or(0.0);
            let lon = step.get("lon").and_then(Value::as_f64).unwrap_or(0.0);
            let zoom = step.get("zoom").and_then(Value::as_f64).unwrap_or(14.3);
            let pitch = step.get("pitch").and_then(Value::as_f64).unwrap_or(48.0);
            let bearing = step.get("bearing").and_then(Value::as_f64).unwrap_or(-16.0);
            let dur = if action == "jump_to" {
                0
            } else {
                step.get("duration_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(900)
            };
            cdp.eval(&format!(
                "window.arusDirector.flyTo({lat}, {lon}, {zoom}, {pitch}, {bearing}, {dur})"
            ))?;
        }
        "set_scenario" => {
            let mut obj = step.as_object().cloned().unwrap_or_default();
            obj.remove("action");
            let arg = serde_json::to_string(&Value::Object(obj)).map_err(|e| e.to_string())?;
            cdp.eval(&format!("window.arusDirector.setScenario({arg})"))?;
        }
        "run_simulation" => {
            cdp.eval("window.arusDirector.runSimulation()")?;
        }
        "fetch_nwp" => {
            cdp.eval("window.arusDirector.fetchNwp()")?;
        }
        "clear_inspector" => {
            cdp.eval("window.arusDirector.clearInspector()")?;
        }
        "clear_interventions" => {
            cdp.eval("window.arusDirector.clearInterventions()")?;
        }
        "add_bund_line" => {
            let pts = step
                .get("points")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()));
            let height_m = step.get("height_m").and_then(Value::as_f64).unwrap_or(2.0);
            let reason = step
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("riverbank flood bund");
            let js_pts = serde_json::to_string(&pts).map_err(|e| e.to_string())?;
            let js_reason = serde_json::to_string(reason).map_err(|e| e.to_string())?;
            cdp.eval(&format!(
                "window.arusDirector.addBundLine({js_pts}, {height_m}, {js_reason})"
            ))?;
        }
        "breach_bund" => {
            let lat = step.get("lat").and_then(Value::as_f64).unwrap_or(0.0);
            let lon = step.get("lon").and_then(Value::as_f64).unwrap_or(0.0);
            let radius = step
                .get("radius_cells")
                .and_then(Value::as_u64)
                .unwrap_or(2);
            cdp.eval(&format!(
                "window.arusDirector.breachBund({lat}, {lon}, {radius})"
            ))?;
        }
        "compare" => {
            cdp.eval("window.arusDirector.compare()")?;
        }
        "load_boundary" => {
            let query = step.get("query").and_then(Value::as_str).unwrap_or("");
            let radius_km = step.get("radius_km").and_then(Value::as_f64).unwrap_or(5.0);
            let shape = step.get("shape").and_then(Value::as_str).unwrap_or("hex");
            let js_arg = serde_json::to_string(query).map_err(|e| e.to_string())?;
            let js_shape = serde_json::to_string(shape).map_err(|e| e.to_string())?;
            cdp.eval(&format!(
                "window.arusDirector.loadBoundaryByQuery({js_arg}, {{ radiusKm: {radius_km}, shape: {js_shape} }})"
            ))?;
        }
        "custom_boundary" => {
            let lon = step.get("lon").and_then(Value::as_f64).unwrap_or(101.415);
            let lat = step.get("lat").and_then(Value::as_f64).unwrap_or(3.310);
            let radius_km = step.get("radius_km").and_then(Value::as_f64).unwrap_or(5.0);
            let shape = step.get("shape").and_then(Value::as_str).unwrap_or("hex");
            let js_shape = serde_json::to_string(shape).map_err(|e| e.to_string())?;
            cdp.eval(&format!(
                "window.arusDirector.setCustomBoundary({lon}, {lat}, {radius_km}, {js_shape}, false)"
            ))?;
        }
        other => return Err(format!("unsupported storyboard action `{other}`")),
    }
    Ok(())
}

fn resolve_dimensions(spec: &Value, cli_res: Option<&str>) -> (u32, u32, String) {
    let key = cli_res
        .or_else(|| spec.get("resolution").and_then(Value::as_str))
        .unwrap_or("")
        .to_ascii_lowercase();
    match key.as_str() {
        "720p" => (1280, 720, "720p".to_owned()),
        "1080p" | "1k" => (1920, 1080, "1080p".to_owned()),
        "2k" => (2560, 1440, "2k".to_owned()),
        "4k" => (3840, 2160, "4k".to_owned()),
        _ => {
            let vp = spec.get("viewport");
            let w = vp
                .and_then(|v| v.get("width"))
                .and_then(Value::as_u64)
                .unwrap_or(1280) as u32;
            let h = vp
                .and_then(|v| v.get("height"))
                .and_then(Value::as_u64)
                .unwrap_or(720) as u32;
            (w, h, format!("{w}x{h}"))
        }
    }
}

fn encode_init_query(init: &Map<String, Value>) -> String {
    let mut pairs: BTreeMap<&str, String> = BTreeMap::new();
    for key in [
        "lat",
        "lon",
        "zoom",
        "pitch",
        "bearing",
        "basemap",
        "presentation",
        "rain",
        "dur",
        "wind",
        "soil",
        "hyetograph",
        "boundary",
        "start_iso",
        "tz",
        "span_hours",
        "antecedent_mm",
        "autoplay",
        "fixed",
    ] {
        if let Some(val) = init.get(key) {
            let s = match val {
                Value::Bool(b) => if *b { "1" } else { "0" }.to_owned(),
                Value::Number(n) => n.to_string(),
                Value::String(st) => st.clone(),
                _ => continue,
            };
            pairs.insert(key, s);
        }
    }
    pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={}", pct_encode(&v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn find_arus_api_binary() -> Result<PathBuf, String> {
    if let Ok(cur) = env::current_exe()
        && let Some(dir) = cur.parent()
    {
        let sibling = dir.join("arus-api");
        if sibling.exists() {
            return Ok(sibling);
        }
    }
    for rel in ["target/release/arus-api", "target/debug/arus-api"] {
        let p = PathBuf::from(rel);
        if p.exists() {
            return Ok(p);
        }
    }
    Err("arus-api binary not found. Build it first: cargo build --release -p arus-api".to_owned())
}

fn find_chromium_binary() -> Result<PathBuf, String> {
    if let Ok(bin) = env::var("CHROME_BIN") {
        let p = PathBuf::from(bin);
        if p.exists() {
            return Ok(p);
        }
    }
    for name in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        if let Ok(out) = Command::new("which").arg(name).output()
            && out.status.success()
        {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if !s.is_empty() {
                return Ok(PathBuf::from(s));
            }
        }
    }
    if let Ok(home) = env::var("HOME") {
        let pw_root = PathBuf::from(home).join(".cache/ms-playwright");
        if let Ok(entries) = fs::read_dir(&pw_root) {
            for entry in entries.flatten() {
                let cand = entry.path().join("chrome-linux64/chrome");
                if cand.exists() {
                    return Ok(cand);
                }
            }
        }
    }
    Err("no Chromium binary found (set CHROME_BIN or install chromium / google-chrome)".to_owned())
}

fn spawn_headless_chromium(
    bin: &Path,
    user_data_dir: &Path,
    width: u32,
    height: u32,
) -> Result<(Option<Child>, u16), String> {
    let mut child = Command::new(bin)
        .args([
            "--headless=new",
            "--remote-debugging-port=0",
            &format!("--window-size={width},{height}"),
            &format!("--user-data-dir={}", user_data_dir.display()),
            "--use-gl=angle",
            "--use-angle=swiftshader",
            "--enable-webgl",
            "--ignore-gpu-blocklist",
            "--no-sandbox",
            "--disable-gpu-sandbox",
            "--disable-dev-shm-usage",
            "--hide-scrollbars",
            "about:blank",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn chromium {} failed: {e}", bin.display()))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "chromium stderr unavailable".to_owned())?;
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("read chromium stderr failed: {e}"))?;
        if n == 0 {
            break;
        }
        if let Some(rest) = line.split("DevTools listening on ws://127.0.0.1:").nth(1)
            && let Some(port_str) = rest.split('/').next()
            && let Ok(port) = port_str.parse::<u16>()
        {
            thread::spawn(move || {
                let mut sink = std::io::sink();
                let _ = std::io::copy(&mut reader, &mut sink);
            });
            return Ok((Some(child), port));
        }
    }
    let _ = child.kill();
    Err("timed out waiting for Chromium DevTools port".to_owned())
}

fn wait_for_tcp(host: &str, port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect((host, port)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("timed out waiting for {host}:{port}"))
}

fn open_cdp_page_target(port: u16) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("connect CDP HTTP failed: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let req = format!(
        "PUT /json/new?about:blank HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write CDP /json/new failed: {e}"))?;
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream
            .read_exact(&mut byte)
            .map_err(|e| format!("read CDP /json/new header failed: {e}"))?;
        head.push(byte[0]);
        if head.len() > 8192 {
            return Err("CDP /json/new header too large".to_owned());
        }
    }
    let head_str = String::from_utf8_lossy(&head);
    let content_len: usize = head_str
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            if k.eq_ignore_ascii_case("content-length") {
                v.trim().parse().ok()
            } else {
                None
            }
        })
        .ok_or_else(|| "missing Content-Length in CDP /json/new response".to_owned())?;
    let mut body = vec![0u8; content_len];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("read CDP /json/new body failed: {e}"))?;
    let parsed: Value = serde_json::from_slice(&body)
        .map_err(|e| format!("parse CDP /json/new JSON failed: {e}"))?;
    let ws_url = parsed
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing webSocketDebuggerUrl in CDP response".to_owned())?;
    let prefix = format!("ws://127.0.0.1:{port}");
    ws_url
        .strip_prefix(&prefix)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("unexpected CDP ws url: {ws_url}"))
}

fn wait_for_js_truthy(cdp: &mut CdpClient, expr: &str, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(v) = cdp.eval_json(expr)
            && v == Value::Bool(true)
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(150));
    }
    Err(format!("timed out waiting for JS condition: {expr}"))
}

fn capture_png(cdp: &mut CdpClient) -> Result<Vec<u8>, String> {
    let res = cdp.call("Page.captureScreenshot", json!({ "format": "png" }))?;
    let b64 = res
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| "CDP Page.captureScreenshot returned no data".to_owned())?;
    BASE64
        .decode(b64)
        .map_err(|e| format!("base64 decode screenshot failed: {e}"))
}

/// Minimal RFC 6455 WebSocket client for localhost Chrome DevTools Protocol.
struct CdpClient {
    stream: TcpStream,
    next_id: u64,
}

impl CdpClient {
    fn connect(host: &str, port: u16, path: &str) -> Result<Self, String> {
        let mut stream =
            TcpStream::connect((host, port)).map_err(|e| format!("CDP ws connect failed: {e}"))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(90)));
        let handshake = format!(
            "GET {path} HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(handshake.as_bytes())
            .map_err(|e| format!("CDP ws handshake write failed: {e}"))?;

        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            stream
                .read_exact(&mut byte)
                .map_err(|e| format!("CDP ws handshake read failed: {e}"))?;
            head.push(byte[0]);
            if head.len() > 8192 {
                return Err("CDP ws handshake header too large".to_owned());
            }
        }
        let head_str = String::from_utf8_lossy(&head);
        if !head_str.starts_with("HTTP/1.1 101") {
            return Err(format!(
                "CDP ws upgrade rejected: {}",
                head_str.lines().next().unwrap_or("")
            ));
        }
        Ok(Self { stream, next_id: 1 })
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let payload = serde_json::to_string(&json!({
            "id": id,
            "method": method,
            "params": params,
        }))
        .map_err(|e| e.to_string())?;
        self.send_text(&payload)?;

        loop {
            let msg = self.recv_text()?;
            let val: Value = match serde_json::from_str(&msg) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if val.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = val.get("error") {
                    return Err(format!("CDP {method} error: {err}"));
                }
                return Ok(val.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn eval(&mut self, expression: &str) -> Result<(), String> {
        let _ = self.call(
            "Runtime.evaluate",
            json!({ "expression": expression, "awaitPromise": true }),
        )?;
        Ok(())
    }

    fn eval_json(&mut self, expression: &str) -> Result<Value, String> {
        let res = self.call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true
            }),
        )?;
        Ok(res
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    fn send_text(&mut self, text: &str) -> Result<(), String> {
        let bytes = text.as_bytes();
        let mut frame = Vec::with_capacity(bytes.len() + 14);
        frame.push(0x81); // FIN + text opcode
        let mask_key = [0x37u8, 0xFA, 0x21, 0x3D];
        let len = bytes.len();
        if len < 126 {
            frame.push(0x80 | (len as u8));
        } else if len <= 65_535 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask_key);
        for (i, &b) in bytes.iter().enumerate() {
            frame.push(b ^ mask_key[i & 3]);
        }
        self.stream
            .write_all(&frame)
            .map_err(|e| format!("CDP ws send failed: {e}"))
    }

    fn recv_text(&mut self) -> Result<String, String> {
        let mut message = Vec::new();
        loop {
            let mut hdr = [0u8; 2];
            self.stream
                .read_exact(&mut hdr)
                .map_err(|e| format!("CDP ws read header failed: {e}"))?;
            let fin = (hdr[0] & 0x80) != 0;
            let opcode = hdr[0] & 0x0F;
            let masked = (hdr[1] & 0x80) != 0;
            let mut len = u64::from(hdr[1] & 0x7F);
            if len == 126 {
                let mut ext = [0u8; 2];
                self.stream
                    .read_exact(&mut ext)
                    .map_err(|e| e.to_string())?;
                len = u64::from(u16::from_be_bytes(ext));
            } else if len == 127 {
                let mut ext = [0u8; 8];
                self.stream
                    .read_exact(&mut ext)
                    .map_err(|e| e.to_string())?;
                len = u64::from_be_bytes(ext);
            }
            let mask = if masked {
                let mut m = [0u8; 4];
                self.stream.read_exact(&mut m).map_err(|e| e.to_string())?;
                Some(m)
            } else {
                None
            };
            let mut payload = vec![0u8; len as usize];
            self.stream
                .read_exact(&mut payload)
                .map_err(|e| format!("CDP ws read payload failed: {e}"))?;
            if let Some(m) = mask {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= m[i & 3];
                }
            }
            match opcode {
                0x8 => return Err("CDP ws closed by peer".to_owned()),
                0x9 => {
                    // Pong back
                    let mut pong = vec![0x8A, 0x80, 0, 0, 0, 0];
                    let _ = self.stream.write_all(&pong);
                    pong.clear();
                }
                0xA => {} // Pong ignored
                0x0..=0x2 => {
                    message.extend_from_slice(&payload);
                    if fin {
                        return String::from_utf8(message)
                            .map_err(|e| format!("CDP ws invalid utf-8: {e}"));
                    }
                }
                _ => {}
            }
        }
    }
}
