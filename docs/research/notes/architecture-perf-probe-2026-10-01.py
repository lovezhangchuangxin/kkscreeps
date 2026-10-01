#!/usr/bin/env python3
"""Exploratory phase measurement, not the formal performance acceptance suite.

Build first: rtk cargo build --release -p kk-cli
Run: rtk proxy python3 docs/research/notes/architecture-perf-probe-2026-10-01.py
Runs are sequential to avoid cross-process benchmark contention. The default
window ends before the fixture's initial 1500-tick creep lifetime boundary.
"""
import argparse
import hashlib
import json
import platform
import statistics
import subprocess
import tempfile
from pathlib import Path


def main():
    root = Path(__file__).resolve().parents[3]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=root / "target/release/kkscreeps")
    parser.add_argument("--output", type=Path, default=Path(__file__).with_suffix(".json"))
    args = parser.parse_args()
    binary = args.binary.resolve()
    runs = []
    # Rotate player counts rather than grouping all repeats at one count.
    with tempfile.TemporaryDirectory(prefix="kk-architecture-perf-") as temp:
        for repetition in range(3):
            for players in (1, 2, 4, 8):
                path = Path(temp) / f"{players}-{repetition}.json"
                result = subprocess.run(
                    [str(binary), "perf-probe", "--players", str(players),
                     "--warmup", "500", "--ticks", "800", "--report", str(path)],
                    cwd=root, capture_output=True, text=True, timeout=90,
                )
                if result.returncode:
                    raise RuntimeError(result.stdout + result.stderr)
                run = json.loads(path.read_text())
                run["repetition"] = repetition + 1
                runs.append(run)
                print(f"completed players={players} repetition={repetition + 1}", flush=True)
    summary = {}
    for players in (1, 2, 4, 8):
        selected = [r for r in runs if r["players"] == players]
        summary[str(players)] = {
            phase: {
                "median_of_run_p50_ms": statistics.median(
                    r["phases"]["phases"][phase]["p50_ms"] for r in selected),
                "run_p95_ms": [r["phases"]["phases"][phase]["p95_ms"] for r in selected],
            } for phase in ("project", "run", "settle", "wall")
        }
    report = {
        "purpose": "exploratory current-build scaling; not official comparison or acceptance",
        "date": "2026-10-01",
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "build": "release, default Cargo features",
        "repetitions": 3,
        "warmup": 500,
        "ticks": 800,
        "limitations": [
            "1-8 players, one independent small room per player; no shared-room workload",
            "phase timers are wall time, not process CPU time or allocation measurements",
            "no official/xxscreeps baseline; no 100-player acceptance or mod overhead measurement",
            "3 runs and 800 samples do not meet the formal >=5 runs / >=3000 samples requirement",
            "sampling ends at tick 1300; later TTL/population changes are outside this window",
        ],
        "summary": summary,
        "runs": runs,
    }
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
