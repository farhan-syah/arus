#!/usr/bin/env python3
"""Local-only declarative storyboard recorder for ARUS Studio.

Executes a JSON storyboard against `arus-api` in headless Chromium via `window.arusDirector`,
saving a continuous `.webm` recording, numbered `.png` screenshots, and `summary.json` locally.

Usage:
    uv run --with playwright python3 scripts/record_storyboard.py \
        --spec docs/storyboard-example.json \
        --out /tmp/arus-storyboard-out
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from playwright.sync_api import sync_playwright


def find_worker_binary() -> str | None:
    repo = Path(__file__).resolve().parent.parent
    for rel in ("target/release/arus-worker", "target/debug/arus-worker"):
        cand = repo / rel
        if cand.exists():
            return str(cand)
    return None


def find_arus_api_binary() -> str:
    repo = Path(__file__).resolve().parent.parent
    for rel in ("target/release/arus-api", "target/debug/arus-api"):
        cand = repo / rel
        if cand.exists():
            return str(cand)
    raise FileNotFoundError(
        "arus-api binary not found. Build it first: cargo build --release -p arus-api"
    )


RESOLUTION_PRESETS = {
    "720p": (1280, 720),
    "1080p": (1920, 1080),
    "1k": (1920, 1080),
    "2k": (2560, 1440),
    "4k": (3840, 2160),
}


def run_storyboard(
    spec_path: Path,
    out_dir: Path,
    base_url: str | None,
    port: int,
    resolution: str | None = None,
) -> None:
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    out_dir.mkdir(parents=True, exist_ok=True)
    res_key = (resolution or spec.get("resolution") or "").lower()
    if res_key in RESOLUTION_PRESETS:
        width, height = RESOLUTION_PRESETS[res_key]
    else:
        vp = spec.get("viewport", {"width": 1280, "height": 720})
        width = int(vp.get("width", 1280))
        height = int(vp.get("height", 720))

    srv = None
    temp_archive = None
    url = base_url
    if url is None:
        temp_archive = tempfile.mkdtemp(prefix="arus-storyboard-")
        bin_path = find_arus_api_binary()
        srv = subprocess.Popen(
            [bin_path, "--archive", temp_archive, "--bind", f"127.0.0.1:{port}"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        url = f"http://127.0.0.1:{port}/"
        time.sleep(1.5)

    saved_shots: list[str] = []
    step_log: list[dict] = []
    video_raw_dir = out_dir / "_video_tmp"
    video_raw_dir.mkdir(parents=True, exist_ok=True)

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch(
                headless=True,
                args=[
                    "--use-gl=angle",
                    "--use-angle=swiftshader",
                    "--enable-webgl",
                    "--ignore-gpu-blocklist",
                ],
            )
            context = browser.new_context(
                viewport={"width": width, "height": height},
                record_video_dir=str(video_raw_dir),
                record_video_size={"width": width, "height": height},
            )
            page = context.new_page()
            page.goto(url, wait_until="domcontentloaded", timeout=30000)
            page.wait_for_function(
                "() => window.arusDirector && window.arusDirector.isReady()",
                timeout=35000,
            )
            if spec.get("presentation", True):
                page.evaluate("() => window.arusDirector.setPresentationMode(true)")
            time.sleep(1.5)

            for idx, step in enumerate(spec.get("steps", []), start=1):
                action = step.get("action")
                t0 = time.time()
                if action == "fly_to":
                    page.evaluate(
                        "([lat, lon, zoom, pitch, bearing, dur]) => window.arusDirector.flyTo(lat, lon, zoom, pitch, bearing, dur)",
                        [
                            float(step["lat"]),
                            float(step["lon"]),
                            float(step.get("zoom", 14.3)),
                            float(step.get("pitch", 48)),
                            float(step.get("bearing", -16)),
                            int(step.get("duration_ms", 900)),
                        ],
                    )
                    time.sleep(float(step.get("wait_s", 1.2)))
                elif action == "set_basemap":
                    page.evaluate(
                        "(m) => window.arusDirector.setBasemap(m)",
                        str(step["mode"]),
                    )
                    time.sleep(float(step.get("wait_s", 1.8)))
                elif action == "set_scenario":
                    opts = {
                        k: v
                        for k, v in step.items()
                        if k
                        in (
                            "rainfall_mm_per_hr",
                            "duration_s",
                            "wind_x_m_per_s",
                            "manning_n",
                            "hyetograph",
                            "soil",
                            "boundary",
                            "start_iso",
                            "tz_label",
                        )
                    }
                    page.evaluate("(o) => window.arusDirector.setScenario(o)", opts)
                elif action == "run_simulation":
                    page.evaluate("() => window.arusDirector.runSimulation()")
                    time.sleep(float(step.get("wait_s", 1.8)))
                elif action == "fetch_nwp":
                    page.evaluate("() => window.arusDirector.fetchNwp()")
                    time.sleep(float(step.get("wait_s", 2.0)))
                elif action == "set_layer":
                    page.evaluate("(l) => window.arusDirector.setLayer(l)", str(step["layer"]))
                    time.sleep(float(step.get("wait_s", 0.6)))
                elif action == "set_frame":
                    page.evaluate("(f) => window.arusDirector.setFrame(f)", int(step["frame"]))
                    time.sleep(float(step.get("wait_s", 0.5)))
                elif action == "play":
                    page.evaluate("() => window.arusDirector.setPlaying(true)")
                    time.sleep(float(step.get("seconds", 3.0)))
                    page.evaluate("() => window.arusDirector.setPlaying(false)")
                elif action == "cross_section":
                    a = step["a"]
                    b = step["b"]
                    page.evaluate(
                        "([a0, a1, b0, b1]) => window.arusDirector.crossSectionLatLon(a0, a1, b0, b1)",
                        [float(a[0]), float(a[1]), float(b[0]), float(b[1])],
                    )
                    time.sleep(float(step.get("wait_s", 1.0)))
                elif action == "clear_inspector":
                    page.evaluate("() => window.arusDirector.clearInspector()")
                elif action == "add_intervention":
                    page.evaluate(
                        "([k, pts]) => window.arusDirector.addInterventionLatLon(k, pts)",
                        [str(step.get("kind", "bund")), step["points"]],
                    )
                    time.sleep(float(step.get("wait_s", 0.4)))
                elif action == "clear_interventions":
                    page.evaluate("() => window.arusDirector.clearInterventions()")
                elif action == "compare":
                    page.evaluate("() => window.arusDirector.compare()")
                    time.sleep(float(step.get("wait_s", 1.8)))
                elif action == "caption":
                    page.evaluate("(t) => window.arusDirector.setCaption(t)", step.get("text", ""))
                    if "wait_s" in step:
                        time.sleep(float(step["wait_s"]))
                elif action == "screenshot":
                    fname = str(step.get("file", f"shot-{idx:02d}.png"))
                    dest = out_dir / fname
                    page.screenshot(path=str(dest))
                    saved_shots.append(fname)
                elif action == "wait":
                    time.sleep(float(step.get("seconds", 1.0)))
                else:
                    raise ValueError(f"unsupported storyboard action `{action}` at step {idx}")

                step_log.append(
                    {
                        "step": idx,
                        "action": action,
                        "elapsed_s": round(time.time() - t0, 3),
                        "state": page.evaluate("() => window.arusDirector.getState()"),
                    }
                )

            video_path = page.video.path() if page.video else None
            context.close()
            browser.close()

            final_video = None
            mp4_video = None
            if video_path and os.path.exists(video_path):
                stem = spec.get("name", "recording")
                dest_video = out_dir / f"{stem}.webm"
                shutil.move(video_path, dest_video)
                final_video = dest_video.name
                worker = find_worker_binary()
                if worker and spec.get("export_mp4", True):
                    dest_mp4 = out_dir / f"{stem}.mp4"
                    res = subprocess.run(
                        [worker, "transcode", "--input", str(dest_video), "--output", str(dest_mp4)],
                        capture_output=True,
                        text=True,
                    )
                    if res.returncode == 0 and dest_mp4.exists():
                        mp4_video = dest_mp4.name

            summary = {
                "name": spec.get("name", "storyboard"),
                "spec": str(spec_path),
                "video": mp4_video or final_video,
                "video_webm": final_video,
                "video_mp4": mp4_video,
                "screenshots": saved_shots,
                "steps": step_log,
            }
            (out_dir / "summary.json").write_text(
                json.dumps(summary, indent=2), encoding="utf-8"
            )
            print(json.dumps(summary, indent=2))
    finally:
        shutil.rmtree(video_raw_dir, ignore_errors=True)
        if srv is not None:
            srv.terminate()
            srv.wait(timeout=5)
        if temp_archive is not None:
            shutil.rmtree(temp_archive, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="ARUS local declarative storyboard recorder")
    parser.add_argument("--spec", required=True, help="Path to storyboard JSON file")
    parser.add_argument("--out", required=True, help="Local output directory for video & PNGs")
    parser.add_argument("--url", default=None, help="Optional running ARUS Studio URL")
    parser.add_argument("--port", type=int, default=18084, help="Local port if launching arus-api")
    parser.add_argument(
        "--resolution",
        choices=["720p", "1080p", "1k", "2k", "4k"],
        default=None,
        help="Output video/screenshot resolution preset (default 720p or spec viewport)",
    )
    args = parser.parse_args()
    run_storyboard(Path(args.spec), Path(args.out), args.url, args.port, args.resolution)
    return 0


if __name__ == "__main__":
    sys.exit(main())
