#!/usr/bin/env python3
"""密集 Mod 纯结算对照：交替运行三种模式，核验对象状态并保留独立样本。

链模式额外维护四个 namespace 计数；它是完整处理链加状态写回的负载，
不将额外工作解释为纯调用成本。采集阶段不运行玩家代码或世界投影。
"""

import argparse
import hashlib
import json
import math
import platform
import statistics
import subprocess
import tempfile
from pathlib import Path


MODES = ("native", "empty", "chain")
SCENARIOS = {"100": (1, 100), "400": (1, 400), "1600": (4, 400)}


def valid_measurement(value):
    # bool 是 Python int 的子类，但不是报告中的合法计时数值。
    return type(value) in (int, float) and value >= 0 and (
        type(value) is int or math.isfinite(value))


def validate_report(report, mode, rooms, links, warmup, ticks, profile=False):
    expected = {"tool": "kkscreeps mod-perf-probe", "mode": mode, "rooms": rooms,
                "links_per_room": links, "warmup": warmup, "ticks": ticks}
    if any(report.get(key) != value for key, value in expected.items()):
        raise RuntimeError("实际负载与请求不一致")
    if report.get("metadata", {}).get("debug_assertions") is not False:
        raise RuntimeError("基线要求 release 构建")
    wall = report.get("wall", {})
    if type(wall.get("count")) is not int or wall["count"] != ticks or any(key not in wall for key in ("p50_ms", "p95_ms", "p99_ms")):
        raise RuntimeError("结算采样数或分位数不符")
    quantiles = [wall[key] for key in ("p50_ms", "p95_ms", "p99_ms")]
    if any(not valid_measurement(value) for value in quantiles) or quantiles != sorted(quantiles):
        raise RuntimeError("结算分位数须为有序的有限非负数")
    if any(key in wall and not valid_measurement(wall[key]) for key in ("max_ms", "sum_ms")):
        raise RuntimeError("结算耗时无效")
    if "max_ms" in wall and wall["max_ms"] < quantiles[-1]:
        raise RuntimeError("结算最大值小于分位数")
    process = report.get("process", {})
    if process.get("scope") != "current_process_all_threads":
        raise RuntimeError("报告缺少进程 CPU/RSS")
    if not valid_measurement(process.get("cpu_core_ms_per_tick")):
        raise RuntimeError("进程 CPU 耗时无效")
    samples = report.get("profile")
    if profile:
        if not isinstance(samples, dict):
            raise RuntimeError("诊断采集缺少阶段计时")
        expected_calls = rooms * links * ticks
        phases = {"slot": 1, "js_call": 1, "budget_start": 1, "budget_finish": 1,
                  "op_native": 1, "host_native": 1, "op_read": 6, "host_read": 6,
                  "op_write": 6, "host_write": 6} if mode == "chain" else {}
        # 无版本字段的旧报告为 v1；v2 必须报告转换阶段，缺失不能静默通过。
        version = report.get("profile_schema_version", 1)
        if type(version) is not int or version not in (1, 2):
            raise RuntimeError("诊断报告版本不支持")
        if mode == "chain" and version == 2:
            phases["read_to_v8"] = 6
        if set(samples) != set(phases):
            raise RuntimeError("诊断阶段集合不符")
        for phase, factor in phases.items():
            sample = samples[phase]
            if type(sample.get("calls")) is not int or type(sample.get("errors")) is not int or sample["calls"] != factor * expected_calls or sample["errors"] != 0:
                raise RuntimeError("诊断调用次数或错误计数不符")
            if type(sample.get("wall_ns")) is not int or sample["wall_ns"] < 0:
                raise RuntimeError("诊断耗时无效")
    elif samples is not None or report.get("profile_schema_version") is not None:
        raise RuntimeError("无插桩基线不能包含诊断计时")
    for boundary, elapsed in (("before", 0), ("after_warmup", warmup), ("after_collection", warmup + ticks)):
        checked = report.get("checks", {}).get(boundary, {})
        calls = elapsed * rooms * links
        expected_state = dict.fromkeys(("before", "outer", "inner", "after"), calls) if mode == "chain" and elapsed else None
        if checked.get("tick") != elapsed or checked.get("verified_links") != rooms * links:
            raise RuntimeError("边界对象验证数量不符")
        if checked.get("namespace") != expected_state:
            raise RuntimeError("处理器调用次数不符")
        digest = checked.get("link_state_sha256", "")
        if len(digest) != 64 or any(char not in "0123456789abcdef" for char in digest):
            raise RuntimeError("对象状态摘要无效")


def summarize(runs):
    result = {}
    for scenario in dict.fromkeys(run["scenario"] for run in runs):
        result[scenario] = {}
        for mode in MODES:
            selected = [run for run in runs if run["scenario"] == scenario and run["mode"] == mode]
            result[scenario][mode] = {
                **{f"median_wall_{q}_ms": statistics.median(run["wall"][f"{q}_ms"] for run in selected)
                   for q in ("p50", "p95", "p99")},
                "median_cpu_core_ms_per_tick": statistics.median(run["process"]["cpu_core_ms_per_tick"] for run in selected),
                "median_rss_bytes_sampled_p50": statistics.median(run["process"]["rss_bytes_sampled_p50"] for run in selected),
                "max_rss_bytes_sampled_peak": max(run["process"]["rss_bytes_sampled_peak"] for run in selected),
                "run_wall_p99_ms": [run["wall"]["p99_ms"] for run in selected],
            }
            if selected[0].get("profile") is not None:
                result[scenario][mode]["median_profile_ms_per_tick"] = {
                    phase: statistics.median(run["profile"][phase]["wall_ns"] / run["ticks"] / 1e6
                                             for run in selected)
                    for phase in selected[0]["profile"]
                }
    return result


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=root / "target/release/kkscreeps")
    parser.add_argument("--fixtures", type=Path, default=root / "compat/fixtures/mod-perf")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--scenarios", nargs="+", choices=SCENARIOS, default=["100", "400"])
    parser.add_argument("--warmup", type=int, default=500)
    parser.add_argument("--ticks", type=int, default=3000)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--profile", action="store_true", help="开启包含计时，输出为诊断数据")
    args = parser.parse_args()
    if not 0 <= args.warmup <= 50000 or not 1 <= args.ticks <= 50000 or args.repetitions < 1:
        parser.error("warmup 须为 0..50000，ticks 须为 1..50000，repetitions 须为正")
    if len(set(args.scenarios)) != len(args.scenarios):
        parser.error("scenarios 不得重复")
    binary = args.binary.resolve()
    fixtures = args.fixtures.resolve()
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    runs = []
    references = {}
    package_references = {}
    partial = args.output.with_suffix(".partial.json")
    with tempfile.TemporaryDirectory(prefix="kk-mod-perf-") as temp:
        for repetition in range(args.repetitions):
            for scenario in args.scenarios:
                rooms, links = SCENARIOS[scenario]
                # 每轮旋转顺序，避免固定的最后运行模式总是承受较热的机器。
                order = MODES[repetition % 3:] + MODES[:repetition % 3]
                for mode in order:
                    if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_hash:
                        raise RuntimeError("测量期间二进制发生变化")
                    path = Path(temp) / "report.json"
                    completed = subprocess.run([
                        str(binary), "mod-perf-probe", "--mode", mode,
                        "--rooms", str(rooms), "--links-per-room", str(links),
                        "--warmup", str(args.warmup), "--ticks", str(args.ticks),
                        "--fixtures", str(fixtures), "--report", str(path),
                        *(["--profile"] if args.profile else []),
                    ], cwd=root, capture_output=True, text=True, timeout=600)
                    if completed.returncode:
                        raise RuntimeError(completed.stdout + completed.stderr)
                    if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_hash:
                        raise RuntimeError("测量期间二进制发生变化")
                    report = json.loads(path.read_text())
                    validate_report(report, mode, rooms, links, args.warmup, args.ticks, args.profile)
                    identity = (report["package_digests"], report["vm_limits"])
                    if package_references.setdefault(mode, identity) != identity:
                        raise RuntimeError("测量期间 Mod 包或预算配置发生变化")
                    states = {key: value["link_state_sha256"] for key, value in report["checks"].items()}
                    if references.setdefault(scenario, states) != states:
                        raise RuntimeError("三种模式的对象状态不等价")
                    report.update(scenario=scenario, repetition=repetition + 1)
                    runs.append(report)
                    partial.write_text(json.dumps({"complete": False, "binary_sha256": binary_hash,
                        "runs": runs}, ensure_ascii=False, indent=2) + "\n")
                    print(f"完成 links={scenario} mode={mode} repetition={repetition + 1}", flush=True)
    result = {
        "complete": True, "purpose": "密集 Mod 纯结算基准；不代表玩家闭环或跨引擎性能验收",
        "binary_sha256": binary_hash, "platform": platform.platform(),
        "warmup": args.warmup, "ticks": args.ticks, "repetitions": args.repetitions,
        "instrumented": args.profile,
        "summary": summarize(runs), "runs": runs,
        "limitations": [
            "合成 link 冷却保持每 tick 有 native 写入，不能代表全部真实玩法",
            "chain 比其他模式额外维护四个 namespace 计数，差值含状态写回",
            "不运行玩家代码或投影；验证全部 link，不比较整个世界所有对象",
            "CPU 为当前进程全部线程，不能当作 Mod 执行线程 CPU 账单",
            "RSS 峰值为边界采样，包含玩家工作者和 Mod VM，可能漏掉瞬时峰值",
            "本地机器频率与其他应用负载未控制，不设置性能硬门禁",
            "诊断阶段为包含墙钟；op 包含 host，JS 包含 op，不能将全部阶段相加",
            "开启诊断会增加计时与 op 包装开销；错误计数仅由 op 事件统计",
        ],
    }
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
    partial.unlink()
    print(json.dumps(result["summary"], ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
