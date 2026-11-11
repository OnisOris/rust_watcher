#!/usr/bin/env python3
"""Capture a real wt PTY, replay keys, and export terminal cells as PNG/text.

QA-only dependencies: pyte, Pillow. No application instrumentation or fake events.
Scenario JSON: [{"wait": "RA ● Ready", "timeout": 90}, {"keys": "j\r"},
                {"capture": "overview"}, {"size": [80, 20]}].
Raw terminal output and timings are retained for independent reproduction.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import termios
import time

import pyte
from PIL import Image, ImageDraw, ImageFont


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--scenario", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--width", type=int, default=120)
    parser.add_argument("--height", type=int, default=35)
    args = parser.parse_args()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    master, slave = pty.openpty()
    screen = pyte.Screen(args.width, args.height)
    stream = pyte.ByteStream(screen)

    def resize(width, height):
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))
        screen.resize(height, width)

    resize(args.width, args.height)
    start = time.monotonic()
    proc = subprocess.Popen(
        [os.path.abspath(args.binary), os.path.abspath(args.project)],
        stdin=slave, stdout=slave, stderr=slave, start_new_session=True,
        env={**os.environ, "TERM": "xterm-256color", "COLORTERM": "truecolor"},
    )
    os.close(slave)
    raw = bytearray()
    events = []

    def pump(seconds):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if select.select([master], [], [], min(0.05, max(0, deadline-time.monotonic())))[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                if not raw:
                    events.append({"first_output_seconds": time.monotonic()-start})
                raw.extend(data)
                stream.feed(data)

    def display():
        # pyte.display indexes empty wide-character continuation cells. Read the
        # actual cell buffer instead (e.g. a crab emoji in a Rust source preview).
        return ["".join(screen.buffer[y][x].data for x in range(screen.columns))
                for y in range(screen.lines)]

    def capture(name):
        text = "\n".join(line.rstrip() for line in display()) + "\n"
        (output / f"{name}.txt").write_text(text)
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", 16)
        cell_w, cell_h = 10, 21
        img = Image.new("RGB", (screen.columns*cell_w+16, screen.lines*cell_h+16), "#10141c")
        draw = ImageDraw.Draw(img)
        colors = {"default": "#d1d5db", "black": "#111827", "red": "#fb7185",
                  "green": "#86efac", "brown": "#fbbf24", "blue": "#7dd3fc",
                  "magenta": "#c4b5fd", "cyan": "#7dd3fc", "white": "#e5e7eb"}
        for y in range(screen.lines):
            for x in range(screen.columns):
                cell = screen.buffer[y][x]
                color = colors.get(cell.fg, "#"+cell.fg if len(cell.fg) == 6 else "#d1d5db")
                draw.text((8+x*cell_w, 8+y*cell_h), cell.data, font=font, fill=color)
        img.save(output / f"{name}.png")

    try:
        for step in json.loads(Path(args.scenario).read_text()):
            if "probe_keys" in step:
                samples = []
                for key in step["probe_keys"] * step.get("repeats", 5):
                    pump(0.1)
                    before = len(raw)
                    sent = time.monotonic()
                    os.write(master, key.encode())
                    while len(raw) == before and time.monotonic()-sent < 2:
                        pump(0.001)
                    samples.append({"key": key, "first_output_ms": (time.monotonic()-sent)*1000,
                                    "observed": len(raw) > before})
                events.append({"input_to_first_terminal_output": samples})
            if "size" in step:
                resize(*step["size"])
                os.killpg(proc.pid, signal.SIGWINCH)
                pump(0.2)  # Observe the resize before injecting the next user action.
            if "keys" in step:
                os.write(master, step["keys"].encode())
            if "wait" in step:
                deadline = time.monotonic()+step.get("timeout", 60)
                while step["wait"] not in "\n".join(display()):
                    pump(0.1)
                    if proc.poll() is not None or time.monotonic() >= deadline:
                        capture("failure")
                        raise RuntimeError(f"Did not observe {step['wait']!r}")
            pump(step.get("settle", 0.3))
            if "capture" in step:
                capture(step["capture"])
            events.append({"seconds": time.monotonic()-start, "step": step})
        os.write(master, b"q")
        quitting = time.monotonic()
        while proc.poll() is None and time.monotonic()-quitting < 5:
            pump(0.05)
        events.append({"quit_seconds": time.monotonic()-quitting, "exit_code": proc.poll()})
    finally:
        (output / "session.ansi").write_bytes(raw)
        with open(args.binary, "rb") as binary:
            digest = hashlib.file_digest(binary, "sha256").hexdigest()
        (output / "session.json").write_text(json.dumps({
            "binary": args.binary, "project": args.project, "events": events,
            "binary_sha256": digest, "initial_size": [args.width, args.height],
        }, indent=2)+"\n")
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        os.close(master)


if __name__ == "__main__":
    main()
