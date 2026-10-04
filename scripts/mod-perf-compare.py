#!/usr/bin/env python3
"""相同负载的 Mod 成对采集；保留每轮差值，不据桌面噪声设置性能门禁。"""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import statistics
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("mod_perf_guard", ROOT / "scripts/mod-perf-baseline.py")
GUARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_report(path, report, *, exclusive=False):
    # 同目录临时文件保证发布时内容完整；独占硬链接不覆盖后来出现的证据。
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=f".{path.name}.", delete=False) as file:
            temporary = Path(file.name)
            json.dump(report, file, ensure_ascii=False, indent=2, allow_nan=False)
            file.write("\n")
        if exclusive:
            os.link(temporary, path)
        else:
            os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def paired_summary(runs):
    result = {}
    for mode, profile in dict.fromkeys((run["mode"], run["instrumented"]) for run in runs):
        selected = [run for run in runs if (run["mode"], run["instrumented"]) == (mode, profile)]
        pairs = {}
        schemas = {}
        patterns = {run.get("order_pattern", "adjacent") for run in selected}
        if len(patterns) != 1 or not patterns <= {"adjacent", "abba"}:
            raise RuntimeError("采集顺序口径不一致或不支持")
        pattern = next(iter(patterns))
        for run in selected:
            GUARD.check_profile_identity(schemas, run["version"], run)
            repetition, pair_id = run["repetition"], run.get("pair", 1)
            if type(repetition) is not int or repetition < 1 or type(pair_id) is not int:
                raise RuntimeError("成对样本轮次或配对编号无效")
            pair = pairs.setdefault((repetition, pair_id), {})
            if run["version"] in pair:
                raise RuntimeError("重复的成对样本")
            pair[run["version"]] = run
        if any(set(pair) != {"before", "after"} for pair in pairs.values()):
            raise RuntimeError("成对样本不完整")
        rounds = {}
        for (repetition, pair_id), pair in pairs.items():
            rounds.setdefault(repetition, {})[pair_id] = pair
        expected_pairs = {1, 2} if pattern == "abba" else {1}
        if any(set(pairs_in_round) != expected_pairs for pairs_in_round in rounds.values()):
            raise RuntimeError("成对样本轮次不完整或配对编号无效")
        metrics = {}
        for metric in ("p50_ms", "p95_ms", "p99_ms", "cpu_core_ms_per_tick"):
            section = "process" if metric == "cpu_core_ms_per_tick" else "wall"
            before = [pair["before"][section][metric] for pair in pairs.values()]
            after = [pair["after"][section][metric] for pair in pairs.values()]
            deltas = [new - old for old, new in zip(before, after)]
            ratios = [new / old if old else None for old, new in zip(before, after)]
            # ABBA/BAAB 的每轮两端均值保留平衡位置；不将相关的两对当作两轮。
            round_before = [statistics.mean(pair["before"][section][metric] for pair in group.values())
                            for group in rounds.values()]
            round_after = [statistics.mean(pair["after"][section][metric] for pair in group.values())
                           for group in rounds.values()]
            round_deltas = [new - old for old, new in zip(round_before, round_after)]
            round_ratios = [new / old if old else None for old, new in zip(round_before, round_after)]
            metrics[metric] = {
                "before_runs": before, "after_runs": after,
                "median_before": statistics.median(before), "median_after": statistics.median(after),
                "paired_after_minus_before": deltas,
                "median_paired_delta": statistics.median(deltas),
                "paired_after_over_before": ratios,
                # 零基线不能定义倍率；保留全部轮次，不删除零值后再算中位数。
                "median_paired_ratio": None if None in ratios else statistics.median(ratios),
                "round_before_means": round_before, "round_after_means": round_after,
                "round_after_minus_before": round_deltas,
                "round_after_over_before": round_ratios,
                "median_round_delta": statistics.median(round_deltas),
                "median_round_ratio": None if None in round_ratios else statistics.median(round_ratios),
            }
        item = {"pairs": len(pairs), "rounds": len(rounds), "order_pattern": pattern, "metrics": metrics}
        if profile:
            item["profile_ms_per_tick"] = {
                version: {
                    phase: statistics.median(pair[version]["profile"][phase]["wall_ns"]
                                             / pair[version]["ticks"] / 1e6 for pair in pairs.values())
                    for phase in next(iter(pairs.values()))[version]["profile"]
                } for version in ("before", "after")
            }
        result[f'{"profile" if profile else "plain"}:{mode}'] = item
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, default=ROOT / "target/release/kkscreeps")
    parser.add_argument("--profile-before", type=Path, help="可选的旧端诊断构建，须配合 --profile")
    parser.add_argument("--fixtures", type=Path, default=ROOT / "compat/fixtures/mod-perf")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rooms", type=int, default=1)
    parser.add_argument("--links-per-room", type=int, default=400)
    parser.add_argument("--warmup", type=int, default=500)
    parser.add_argument("--ticks", type=int, default=3000)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--order-pattern", choices=("adjacent", "abba"), default="adjacent",
                        help="adjacent 每轮一对；abba 每轮 ABBA/BAAB 两对，另汇总每轮均值差")
    parser.add_argument("--modes", nargs="+", choices=GUARD.MODES, default=list(GUARD.MODES))
    parser.add_argument("--profile", action="store_true", help="另采完整链诊断，普通窗口保持无插桩")
    parser.add_argument("--timeout", type=int, default=600, help="每个子进程的外层期限，单位秒")
    args = parser.parse_args()
    if not (1 <= args.rooms <= 4 and 1 <= args.links_per_room <= 400
            and 0 <= args.warmup <= 50000 and 1 <= args.ticks <= 50000
            and args.repetitions >= 1 and args.timeout >= 1):
        parser.error("负载超出 CLI 范围，或重复次数/期限不是正数")
    if len(set(args.modes)) != len(args.modes):
        parser.error("modes 不得重复")
    if args.profile_before and not args.profile:
        parser.error("--profile-before 须配合 --profile")
    if args.profile and "chain" not in args.modes:
        parser.error("诊断采集要求 modes 包含 chain")
    out = args.output.resolve()
    partial = out.with_suffix(".partial.json")
    if out.exists() or partial.exists():
        parser.error("output 或 partial 已存在；请选择新路径以保留已有证据")
    if out == partial:
        parser.error("output 不得以 .partial.json 结尾")
    if not out.parent.is_dir():
        parser.error("output 父目录不存在")
    binaries = {"before": args.before.resolve(), "after": args.after.resolve()}
    if args.profile_before:
        binaries["profile_before"] = args.profile_before.resolve()
    hashes = {name: digest(path) for name, path in binaries.items()}
    configuration = {key: getattr(args, key) for key in
                     ("rooms", "links_per_room", "warmup", "ticks", "repetitions", "modes", "profile", "timeout", "order_pattern")}
    configuration["fixtures"] = str(args.fixtures.resolve())
    runs, identities, schemas = [], {}, {}
    def save_partial(*, exclusive=False):
        write_report(partial, {"complete": False, "binaries": hashes,
                              "configuration": configuration, "runs": runs}, exclusive=exclusive)

    # 在启动子进程前独占路径；同路径竞争者在此失败，不能互改 partial。
    save_partial(exclusive=True)
    reference = None
    with tempfile.TemporaryDirectory(prefix="kk-mod-compare-") as temp:
        for rep in range(args.repetitions):
            # 两端相邻运行，逐轮交换先后；模式轮换避免固定承受较热的机器。
            offset = rep % len(args.modes)
            order = args.modes[offset:] + args.modes[:offset]
            tasks = [(mode, False) for mode in order]
            if args.profile:
                tasks.append(("chain", True))
            versions = ("after", "before") if rep % 2 else ("before", "after")
            captures = [(1, version) for version in versions]
            if args.order_pattern == "abba":
                captures.extend((2, version) for version in reversed(versions))
            for mode, profile in tasks:
                for pair_id, version in captures:
                    label = "profile_before" if profile and version == "before" and args.profile_before else version
                    binary = binaries[label]
                    if digest(binary) != hashes[label]:
                        raise RuntimeError("测量期间二进制发生变化")
                    path = Path(temp) / "report.json"
                    path.unlink(missing_ok=True)
                    command = [str(binary), "mod-perf-probe", "--mode", mode,
                               "--rooms", str(args.rooms), "--links-per-room", str(args.links_per_room),
                               "--warmup", str(args.warmup), "--ticks", str(args.ticks),
                               "--fixtures", configuration["fixtures"], "--report", str(path)]
                    if profile:
                        command.append("--profile")
                    completed = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=args.timeout)
                    if completed.returncode:
                        raise RuntimeError(completed.stdout + completed.stderr)
                    if digest(binary) != hashes[label]:
                        raise RuntimeError("测量期间二进制发生变化")
                    report = json.loads(path.read_text())
                    GUARD.validate_report(report, mode, args.rooms, args.links_per_room,
                                          args.warmup, args.ticks, profile)
                    GUARD.check_profile_identity(schemas, (label, mode, profile), report)
                    identity = (report["package_digests"], report["vm_limits"])
                    if identities.setdefault(mode, identity) != identity:
                        raise RuntimeError("测量期间 Mod 包或预算配置发生变化")
                    states = {key: value["link_state_sha256"] for key, value in report["checks"].items()}
                    if reference is None:
                        reference = states
                    if reference != states:
                        raise RuntimeError("两端或模式的对象状态不等价")
                    report.update(version=version, binary_label=label, instrumented=profile,
                                  repetition=rep + 1, pair=pair_id, order_pattern=args.order_pattern)
                    runs.append(report)
                    save_partial()
                    print(f"完成 repetition={rep + 1} pair={pair_id} version={version} mode={mode} profile={profile}", flush=True)
    result = {
        "complete": True, "scope": "paired Mod pure settlement; no player execution or projection",
        "platform": platform.platform(), "binaries": hashes, "configuration": configuration,
        "summary": paired_summary(runs), "runs": runs,
        "limitations": [
            "桌面负载与频率未控制；增加窗口不能替代稳定基准机，无性能硬门禁",
            "成对倍率为逐对 after/before 的中位数；各窗口 P99 仍全部保留",
            "ABBA/BAAB 每轮四窗口、两对相关；另列各轮两端窗口指标均值的倍率，不等于合并 tick 后的分位数",
            "平衡顺序只抵消等间隔加性线性漂移，不消除非线性漂移、不同运行时长或外部负载",
            "诊断阶段为包含耗时，增加插桩成本，与普通窗口分开",
            "合成 link 世界，chain 额外维护四个计数；不代表真实玩家、持久化或网络",
        ],
    }
    write_report(out, result, exclusive=True)
    partial.unlink()
    print(json.dumps(result["summary"], ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
