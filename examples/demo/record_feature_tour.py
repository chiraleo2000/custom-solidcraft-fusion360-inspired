#!/usr/bin/env python3
"""Play examples/demo/feature_tour.json in the desktop app and save one window frame per scene.

The scenes are the same ones feature_tour_scenes_cover_the_workspaces already runs headless.
"""
import json
import os
import shutil
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOUR = Path(__file__).with_name("feature_tour.json")
EXE = ROOT / "target" / "release" / "solidcraft.exe"
OUT = ROOT / "target" / "feature-tour"
PORT = 9876


def call(sock, method, params=None, timeout=60):
    request = {"id": 1, "method": method, "params": params or {}}
    sock.sendall((json.dumps(request) + "\n").encode())
    sock.settimeout(timeout)
    data = b""
    while b"\n" not in data:
        chunk = sock.recv(1 << 20)
        if not chunk:
            raise RuntimeError(f"{method} closed the control connection")
        data += chunk
    reply = json.loads(data.split(b"\n", 1)[0])
    if not reply.get("ok"):
        raise RuntimeError(f"{method} {params}: {reply.get('error')}")
    return reply.get("result")


def park_crash_reports():
    """A previous killed recording would otherwise open the crash window over the model."""
    base = Path(os.environ.get("LOCALAPPDATA", "")) / "SolidCraft" / "crashes"
    held = base.with_name("crashes-held-for-tour")
    if held.exists():
        shutil.rmtree(held, ignore_errors=True)
    if base.exists():
        base.rename(held)
    return base, held


def restore_crash_reports(base, held):
    fresh = base if base.exists() else None
    if fresh is not None and fresh != held:
        shutil.rmtree(fresh, ignore_errors=True)
    if held.exists() and not base.exists():
        held.rename(base)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    tour = json.loads(TOUR.read_text(encoding="utf-8"))
    parked = park_crash_reports()
    app = subprocess.Popen([str(EXE), "--control", str(PORT)], cwd=ROOT)
    sock = None
    try:
        deadline = time.time() + 30
        while time.time() < deadline:
            try:
                sock = socket.create_connection(("127.0.0.1", PORT), timeout=1)
                break
            except OSError:
                if app.poll() is not None:
                    raise RuntimeError(f"the app exited {app.returncode} before the control port opened")
                time.sleep(0.2)
        else:
            raise RuntimeError("control port did not open")
        sock_file_ok = sock
        time.sleep(1.5)
        call(sock_file_ok, "engine.execute", {"command": "ui.coaching.set", "params": {"enabled": False, "steps": 7}})
        call(sock_file_ok, "engine.execute", {"command": "ui.coaching.dismiss", "params": {"steps": 7}})
        time.sleep(0.4)
        frames = []
        for index, scene in enumerate(tour["scenes"]):
            title = scene["title"]
            call(sock_file_ok, "engine.execute", {"command": "file.new", "params": {"name": title}})
            for step in scene["commands"]:
                call(sock_file_ok, "engine.execute", {"command": step["command"], "params": step.get("params") or {}})
            # file.new replaces the session, which turns the first-run callout back on.
            call(sock_file_ok, "engine.execute", {"command": "ui.coaching.set", "params": {"enabled": False, "steps": 7}})
            call(sock_file_ok, "ui.view", {"view": "home", "animate": False})
            call(sock_file_ok, "ui.view", {"view": "fit", "animate": False})
            time.sleep(0.6)
            path = OUT / f"frame-{index:02d}.png"
            call(sock_file_ok, "ui.screenshot", {"path": str(path)})
            if not path.is_file() or path.stat().st_size < 1000:
                raise RuntimeError(f"{title} did not write a window frame")
            frames.append((title, path))
            print(f"captured {index:02d} {title} {path.stat().st_size}", flush=True)
        (OUT / "frames.json").write_text(json.dumps([{"title": t, "path": str(p)} for t, p in frames], indent=2), encoding="utf-8")
    finally:
        if sock is not None:
            sock.close()
        app.terminate()
        try:
            app.wait(timeout=8)
        except subprocess.TimeoutExpired:
            app.kill()
        restore_crash_reports(*parked)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"feature tour recording failed: {error}", file=sys.stderr)
        sys.exit(1)
