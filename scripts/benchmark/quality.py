#!/usr/bin/env python3
"""同夹具交替比较固定的前后 release 产物；失败保留日志，不生成成功汇总。"""
import argparse
import hashlib
import importlib.util
import json
import math
import os
import time
import sys
from pathlib import Path
import platform
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("mod_perf", ROOT / "scripts/mod-perf-baseline.py")
mod_perf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod_perf)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_player(report, players, channel, warmup, ticks):
    if report.get("metadata", {}).get("debug_assertions") is not False:
        raise ValueError("必须使用 release")
    for key, expected in dict(players=players, channel=channel, warmup=warmup, ticks=ticks).items():
        if report.get(key) != expected:
            raise ValueError(f"负载不符：{key}")
    workload = report["workload"]
    names = {f"p{i+1}" for i in range(players)}
    if set(workload["actions"]) != names:
        raise ValueError("玩家回执缺失")
    for receipt in workload["actions"].values():
        if receipt["ticks"] != warmup + ticks or receipt["upgrade"] <= 0:
            raise ValueError("执行或升级计数不符")
        if warmup + ticks >= 100 and receipt["harvest"] <= 0:
            raise ValueError("采集臂空转")
    for boundary in ("initial", "before", "after"):
        if {room["room"] for room in workload[boundary]} != {f"W{i}N1" for i in range(players)}:
            raise ValueError("房间缺失")
    for before, after in zip(workload["before"], workload["after"]):
        if before["room"] != after["room"] or after["progress"] <= before["progress"]:
            raise ValueError("房间没有实际升级")
    wall = report["phases"]["phases"]["wall"]
    if wall["count"] != ticks:
        raise ValueError("采样数不符")
    for value in [wall[f"{q}_ms"] for q in ("p50", "p95", "p99")]:
        if not math.isfinite(value) or value < 0:
            raise ValueError("耗时无效")


def behavior(report, kind):
    if kind == "player":
        return report["workload"]
    # generation 属于实现版本，不影响 link 玩法状态；其余边界必须逐值相等。
    return {boundary: {key: value for key, value in state.items() if key != "generation"}
            for boundary, state in report["checks"].items()}


def run_command(command, log, timeout=600):
    started = time.monotonic()
    with log.open("w") as output:
        process = subprocess.Popen(command, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT)
        while True:
            pid, status, usage = os.wait4(process.pid, os.WNOHANG)
            if pid:
                process.returncode = os.waitstatus_to_exitcode(status)
                break
            if time.monotonic() - started > timeout:
                process.kill()
                process.wait()
                raise subprocess.TimeoutExpired(command, timeout)
            time.sleep(0.1)
    if process.returncode:
        raise subprocess.CalledProcessError(process.returncode, command)
    return dict(wall_seconds=time.monotonic()-started, cpu_seconds=usage.ru_utime+usage.ru_stime,
                max_rss_bytes=usage.ru_maxrss if sys.platform == "darwin" else usage.ru_maxrss*1024,
                scope="complete_child_process_including_startup")


def validate_pathfinder(report):
    cases = {"terrain", "callback_none", "callback_matrix", "room_grid"}
    if len(report) != 20 or {(r["round"], r["case"]) for r in report} != {(round, case) for round in range(1, 6) for case in cases}:
        raise ValueError("寻路样本缺失或重复")
    if any(not math.isfinite(r["micros_per_search"]) or r["micros_per_search"] <= 0 for r in report):
        raise ValueError("寻路耗时无效")


def bench_payload(text):
    # libtest 在同一行先输出 "test <名称> ... " 前缀再接测试体的 println!，
    # 标记不在行首；按子串定位并截到行尾。
    marker = "PATHFINDER_PLAYER_BENCH="
    index = text.find(marker)
    if index < 0:
        raise ValueError("寻路日志缺少基准输出行")
    return text[index + len(marker):text.find("\n", index)]


def fixture_hashes():
    paths = [* (ROOT / "compat/fixtures/mod-perf").rglob("*"), ROOT / "crates/kk-pathfinder/tests/fixtures/sample-terrain.bin"]
    return {str(path.relative_to(ROOT)): digest(path) for path in paths if path.is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=500)
    parser.add_argument("--ticks", type=int, default=3000)
    parser.add_argument("--players", type=int, nargs="+", default=[1, 4, 8, 32])
    parser.add_argument("--kinds", nargs="+", choices=["player", "mod", "pathfinder"], default=["player", "mod", "pathfinder"])
    args = parser.parse_args()
    if args.repetitions < 1 or args.ticks < 1 or args.warmup < 0:
        parser.error("采样窗口和重复次数非法")
    args.output.mkdir(parents=True, exist_ok=False)
    binaries = {version: directory.resolve() / "kkscreeps" for version, directory in
                (("before", args.before), ("after", args.after))}
    hashes = {version: digest(binary) for version, binary in binaries.items()}
    path_hashes = {version: digest(binary.parent / "pathfinder_perf") for version, binary in binaries.items()}
    fixtures = fixture_hashes()
    (args.output / "identity.json").write_text(json.dumps(dict(binary_sha256=hashes, pathfinder_sha256=path_hashes, fixture_sha256=fixtures), indent=2))
    tasks = []
    if "player" in args.kinds:
        tasks += [("player", f"{players}-{channel}", players, channel)
                  for players in args.players for channel in ("json", "kkpv")]
    if "mod" in args.kinds:
        tasks += [("mod", f"{links}-{mode}", links, mode)
                  for links in (100, 400) for mode in ("native", "empty", "chain")]
    if "pathfinder" in args.kinds:
        tasks += [("pathfinder", "player-search", 0, "")]
    runs, references = [], {}
    for repetition in range(args.repetitions):
        for kind, scenario, count, mode in tasks:
            for version in (("before", "after") if repetition % 2 == 0 else ("after", "before")):
                binary = binaries[version]
                if digest(binary) != hashes[version]:
                    raise ValueError("采集期间二进制变化")
                stem = f"{kind}-{scenario}-{repetition}-{version}"
                path = args.output / f"{stem}.json"
                log = args.output / f"{stem}.log"
                if kind == "pathfinder":
                    executable = binary.parent / "pathfinder_perf"
                    command = [str(executable), "--ignored", "--nocapture", "--test-threads=1"]
                elif kind == "player":
                    command = [str(binary), "perf-probe", "--players", str(count), "--concurrency", "2"]
                    if mode == "kkpv": command += ["--kkpv"]
                else:
                    command = [str(binary), "mod-perf-probe", "--mode", mode, "--links-per-room", str(count),
                               "--fixtures", str(ROOT / "compat/fixtures/mod-perf")]
                if kind != "pathfinder":
                    command += ["--warmup", str(args.warmup), "--ticks", str(args.ticks), "--report", str(path.resolve())]
                print(f"采集 {stem}", flush=True)
                usage = run_command(command, log)
                if digest(binary) != hashes[version] or digest(binary.parent / "pathfinder_perf") != path_hashes[version] or fixture_hashes() != fixtures:
                    raise ValueError("采集期间产物或固定夹具变化")
                if kind == "pathfinder":
                    report = json.loads(bench_payload(log.read_text()))
                    path.write_text(json.dumps(report, indent=2))
                    validate_pathfinder(report)
                else:
                    report = json.loads(path.read_text())
                    if kind == "player": validate_player(report, count, mode, args.warmup, args.ticks)
                    else: mod_perf.validate_report(report, mode, 1, count, args.warmup, args.ticks)
                    reference = references.setdefault((kind, scenario), behavior(report, kind))
                    if reference != behavior(report, kind): raise ValueError(f"行为不等价：{stem}")
                runs.append(dict(kind=kind, scenario=scenario, version=version, repetition=repetition, file=path.name, process=usage))
                (args.output / "progress.json").write_text(json.dumps(dict(complete=False, completed=len(runs), expected=args.repetitions * len(tasks) * 2)))
    summary = {}
    for kind, scenario, _, _ in tasks:
        summary[f"{kind}:{scenario}"] = {}
        for version in binaries:
            reports = [json.loads((args.output / run["file"]).read_text()) for run in runs
                       if (run["kind"], run["scenario"], run["version"]) == (kind, scenario, version)]
            if kind == "pathfinder":
                metrics = {case: statistics.median(statistics.median(r["micros_per_search"] for r in report if r["case"] == case)
                           for report in reports) for case in ("terrain", "callback_none", "callback_matrix", "room_grid")}
            else:
                wall = lambda report: report["wall"] if kind == "mod" else report["phases"]["phases"]["wall"]
                metrics = {f"wall_{q}_ms": statistics.median(wall(report)[f"{q}_ms"] for report in reports) for q in ("p50", "p95", "p99")}
                metrics.update({metric: statistics.median(report["process"][metric] for report in reports)
                                for metric in ("cpu_core_ms_per_tick", "rss_bytes_sampled_p50", "rss_bytes_sampled_peak")})
            summary[f"{kind}:{scenario}"][version] = metrics
    result = dict(complete=True, platform=platform.platform(), binary_sha256=hashes, pathfinder_sha256=path_hashes, fixture_sha256=fixtures, repetitions=args.repetitions,
                  warmup=args.warmup, ticks=args.ticks, runs=runs, summary=summary)
    (args.output / "summary.json").write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
