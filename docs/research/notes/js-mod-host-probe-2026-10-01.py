#!/usr/bin/env python3
"""Capture five independent release runs plus a debug reentrancy check.

Build both profiles first with rtk cargo run --manifest-path
spikes/deno-spike/Cargo.toml --example js_mod_host [--release] --locked.
This script executes already-built binaries; it does not benchmark compilation.
"""
import hashlib
import json
import platform
import statistics
import subprocess
from pathlib import Path


def main():
    root = Path(__file__).resolve().parents[3]
    fixture = root / "spikes/deno-spike"
    release = fixture / "target/release/examples/js_mod_host"
    debug = fixture / "target/debug/examples/js_mod_host"

    def run(binary):
        process = subprocess.run([str(binary)], cwd=root, capture_output=True, text=True, timeout=30)
        if process.returncode:
            raise RuntimeError(process.stderr + process.stdout)
        return json.loads(process.stdout)

    releases = [run(release) for _ in range(5)]
    debug_run = run(debug)
    summary = {
        n: {
            mode: statistics.median(r["timings"][n][mode]["median_us_per_pass"] for r in releases)
            for mode in ("field_ops", "serde_batch")
        } for n in ("1", "100", "1000")
    }
    report = {
        "date": "2026-10-01",
        "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
        "fixture_sha256": {name: hashlib.sha256((fixture / "examples" / name).read_bytes()).hexdigest()
                           for name in ("js_mod_host.rs", "js_mod_host.js")},
        "binary_sha256": hashlib.sha256(release.read_bytes()).hexdigest(),
        "platform": platform.platform(),
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "summary_median_of_run_medians_us": summary,
        "release_runs": releases,
        "debug_run": debug_run,
        "limitations": [
            "Rust-owned synthetic f64 vector, no real RoomState, game handlers or per-viewer security",
            "100 passes/sample; 5 warmup samples + 5 measured samples/mode/count within each process",
            "5 independent release processes, one debug process; no cross-process benchmark contention",
            "serde_v8 batch copies all values; not typed buffers, shared memory, sparse patch or lazy access",
            "direct same-thread ops; no thread/channel IPC; timings are wall time",
            "timeout test interrupts a pure JS loop only, not a blocking native op or process crash",
            "candidate rollback and runtime reset explicitly performed by the probe host",
        ],
    }
    output = Path(__file__).with_suffix(".json")
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"summary_us": summary, "debug_checks": debug_run["checks"]}, indent=2))


if __name__ == "__main__":
    main()
