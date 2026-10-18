#!/usr/bin/env python3
"""Standard-library client for the persistent MCFinder JSONL backend."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--engine", default=str(ROOT / "target/release/mcfinder"))
parser.add_argument("--script", type=Path, default=ROOT / "examples/scripts/ship-mansion.rhai")
parser.add_argument("--mode", choices=["cpu-only", "cpu-and-gpu"], default="cpu-only")
parser.add_argument("--cores", type=int, default=1)
args = parser.parse_args()
with subprocess.Popen([args.engine, "serve"], stdin=subprocess.PIPE,
                      stdout=subprocess.PIPE, text=True, bufsize=1) as process:
    def send(command):
        process.stdin.write(json.dumps(command) + "\n")
        process.stdin.flush()

    send({"cmd": "start", "id": "search-1", "max_results": 3,
          "settings": {"script": {"source": args.script.read_text(), "name": str(args.script)},
                       "experimental": True, "count": "1000000", "mode": args.mode,
                       "cores": args.cores, "structure_seeds": True, "upper_count": 1}})
    try:
        for line in process.stdout:
            event = json.loads(line)
            kind = event["event"]
            if kind == "progress":
                state = event["state"]
                print(f"found={state['results_emitted']} bases/s={state['bases_per_second']:.0f}")
            elif kind == "match":
                print(json.dumps(event["hit"], ensure_ascii=False))
            elif kind == "error":
                print("ERROR:", event["message"])
            elif kind == "finished":
                print("FINISHED:", event["summary"]["status"])
                send({"cmd": "shutdown", "id": "shutdown-1"})
            elif kind == "ack" and event.get("action") == "shutdown":
                break
    except KeyboardInterrupt:
        send({"cmd": "cancel", "id": "cancel-1"})
    finally:
        # EOF also cancels an active search; the engine commits its safe prefix.
        process.stdin.close()
