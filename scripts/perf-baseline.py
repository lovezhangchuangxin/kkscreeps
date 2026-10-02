#!/usr/bin/env python3
"""顺序运行本仓性能负载，保留全部独立运行及各档中位数。

先构建 release 二进制。此脚本不启动其他引擎，不作跨引擎性能结论。
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
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=root / "target/release/kkscreeps")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--warmup", type=int, default=500)
    parser.add_argument("--ticks", type=int, default=3000)
    parser.add_argument("--repetitions", type=int, default=5)
    args = parser.parse_args()
    if args.warmup < 0 or args.ticks < 1 or args.repetitions < 1:
        parser.error("warmup 须非负，ticks 与 repetitions 须为正数")
    binary = args.binary.resolve()
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    runs = []
    with tempfile.TemporaryDirectory(prefix="kk-perf-baseline-") as temp:
        for repetition in range(args.repetitions):
            for players in (1, 4, 8, 32):
                if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_hash:
                    raise RuntimeError("测量期间二进制发生变化，请重新开始")
                path = Path(temp) / f"{players}-{repetition}.json"
                result = subprocess.run(
                    [str(binary), "perf-probe", "--players", str(players),
                     "--warmup", str(args.warmup), "--ticks", str(args.ticks),
                     "--report", str(path)],
                    cwd=root, capture_output=True, text=True, timeout=300,
                )
                if result.returncode:
                    raise RuntimeError(result.stdout + result.stderr)
                if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_hash:
                    raise RuntimeError("测量期间二进制发生变化，请重新开始")
                run = json.loads(path.read_text())
                if run["players"] != players or run["ticks"] != args.ticks:
                    raise RuntimeError("实际负载与请求不一致")
                if run["metadata"]["debug_assertions"]:
                    raise RuntimeError("基线要求 release 构建")
                phases = run.get("phases", {}).get("phases", {})
                if any("p99_ms" not in phases.get(phase, {})
                       for phase in ("project", "run", "settle", "wall")):
                    raise RuntimeError("报告缺少 p99；请核对并重建二进制及 runtime 依赖")
                if run.get("process", {}).get("scope") != "current_process_all_threads":
                    raise RuntimeError("报告缺少当前进程 CPU/RSS 采样")
                run["repetition"] = repetition + 1
                runs.append(run)
                partial = args.output.with_suffix(".partial.json")
                partial.write_text(json.dumps({"complete": False,
                    "binary_sha256": binary_hash, "runs": runs},
                    ensure_ascii=False, indent=2) + "\n")
                print(f"完成 players={players} repetition={repetition + 1}", flush=True)
    summary = {}
    for players in (1, 4, 8, 32):
        selected = [r for r in runs if r["players"] == players]
        summary[str(players)] = {
            "median_cpu_core_ms_per_tick": statistics.median(
                r["process"]["cpu_core_ms_per_tick"] for r in selected),
            "median_rss_bytes_sampled_p50": statistics.median(
                r["process"]["rss_bytes_sampled_p50"] for r in selected),
            "max_rss_bytes_sampled_peak": max(
                r["process"]["rss_bytes_sampled_peak"] for r in selected),
            "median_wall_p50_ms": statistics.median(
                r["phases"]["phases"]["wall"]["p50_ms"] for r in selected),
            "run_wall_p99_ms": [
                r["phases"]["phases"]["wall"]["p99_ms"] for r in selected],
        }
    report = {
        "purpose": "本仓改造基线；不代表三引擎对照或完整架构性能验收",
        "binary_sha256": binary_hash,
        "platform": platform.platform(),
        "repetitions": args.repetitions,
        "warmup": args.warmup,
        "ticks": args.ticks,
        "limitations": [
            "每玩家独立小房间；负载自检仅覆盖首房",
            "CPU 为当前进程全部线程，包括采样与采集窗口内负载检查",
            "RSS 峰值为 tick 边界采样最大值，可能漏掉 tick 内瞬时峰值",
            "长期窗口含 creep 死亡、重新孵化和墓碑衰减",
            "本地机器未控制频率和其他应用负载；不设置性能硬门禁",
        ],
        "summary": summary,
        "runs": runs,
    }
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    args.output.with_suffix(".partial.json").unlink()
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
