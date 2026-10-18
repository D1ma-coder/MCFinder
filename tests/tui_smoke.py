#!/usr/bin/env python3
"""Unix PTY smoke test: rendering, keyboard control and terminal restoration."""
import fcntl
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time

binary = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/debug/mcfinder")
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 110, 0, 0))
original = termios.tcgetattr(slave)
process = subprocess.Popen([binary, "search", "--version", "1.18.2", "--near",
                            "mansion:village:0", "--radius", "0", "--count",
                            "1000000000000", "--placement-only", "--cores", "1", "--ui", "tui"],
                           stdin=slave, stdout=slave, stderr=slave,
                           env={**os.environ, "TERM": "xterm-256color"})
capture = bytearray()
def pump(seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.05)[0]:
            capture.extend(os.read(master, 65536))
try:
    pump(0.8)
    os.write(master, b" ")
    pump(1.2)
    os.write(master, b" ")
    pump(0.3)
    os.write(master, b"q")
    deadline = time.monotonic() + 5
    while process.poll() is None and time.monotonic() < deadline:
        pump(0.1)
    assert process.poll() == 0, f"TUI did not exit successfully: {process.poll()}"
    pump(0.1)
    assert b"MCFinder" in capture, "TUI header was not rendered"
    assert b"\x1b[?1049h" in capture and b"\x1b[?1049l" in capture, "Alternate screen was not restored"
    assert termios.tcgetattr(slave) == original, "Terminal modes were not restored"
    print("TUI PTY: render, pause/resume, stop and terminal restoration passed")
finally:
    if process.poll() is None:
        process.kill()
    process.wait()
    os.close(master)
    os.close(slave)
