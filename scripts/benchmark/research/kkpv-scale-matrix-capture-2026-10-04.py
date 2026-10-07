#!/usr/bin/env python3
"""KKPV 双通道规模矩阵采集（4/8/32 玩家 × JSON/KKPV × 交错多样）。

用法（仓库根目录）：
    cargo build --release -p kk-cli
    python3 scripts/benchmark/research/kkpv-scale-matrix-capture-2026-10-04.py /tmp/matrix.json

每次运行为独立进程。两重交错：同一轮内两通道顺序逐轮交换（偶数轮
json→kkpv，奇数轮 kkpv→json），近似抵消等间隔线性漂移（偶数轮次下
精确）；规模顺序逐轮轮换（4/8/32 → 8/32/4 → 32/4/8），避免固定规模
永远占据轮内同一位置造成跨规模比较的位置混杂。输出文件含全部原始
报告（带轮内序号）与按（规模、通道）聚合的中位数/区间；写入经临时
文件原子替换，已存在的输出路径拒绝覆盖。

门禁全部使用显式 RuntimeError（不依赖 assert，python -O 下保持有效）。
"""

import hashlib
import json
import os
import statistics
import subprocess
import sys
import tempfile
import time

SCALES = (4, 8, 32)
CHANNELS = ("json", "kkpv")
WARMUP = 50
TICKS = 300
EXPECTED_CONCURRENCY = 2
EXPECTED_RSS_OBSERVATIONS = TICKS + 1
ELAPSED_S_LIMIT = 120.0
SETTLE_CANARY_RELATIVE_BAND = 0.25


def fail(message):
    raise RuntimeError(message)


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def rotated_scales(repetition):
    """规模顺序逐轮轮换：位置混杂在规模轴上均衡。"""
    shift = repetition % len(SCALES)
    return SCALES[shift:] + SCALES[:shift]


def run_probe(binary, players, channel, report_path):
    command = [
        binary, "perf-probe",
        "--players", str(players),
        "--warmup", str(WARMUP),
        "--ticks", str(TICKS),
        "--report", report_path,
    ]
    if channel == "kkpv":
        command.append("--kkpv")
    started = time.time()
    completed = subprocess.run(command, capture_output=True, text=True)
    if completed.returncode != 0:
        fail(f"perf-probe 退出码 {completed.returncode}（players={players} "
             f"channel={channel}）：{completed.stderr[-2000:]}")
    return time.time() - started


def gate_report(raw, players, channel, binary_sha):
    """单报告门禁：身份一致、负载真实、指标有限非负。返回提取的指标。"""
    if raw.get("channel") != channel:
        fail(f"通道不符：期望 {channel}，报告为 {raw.get('channel')}")
    if raw.get("players") != players:
        fail(f"玩家数不符：期望 {players}，报告为 {raw.get('players')}")
    if raw.get("ticks") != TICKS or raw.get("warmup") != WARMUP:
        fail(f"窗口不符：ticks={raw.get('ticks')} warmup={raw.get('warmup')}")
    meta = raw.get("metadata", {})
    if not meta.get("git_commit"):
        fail("报告缺 git_commit")
    if meta.get("debug_assertions") is not False:
        fail(f"debug 断言未关闭（debug_assertions="
             f"{meta.get('debug_assertions')!r}）——误用 debug 二进制")
    if raw.get("player_concurrency") != EXPECTED_CONCURRENCY:
        fail(f"玩家并发应为 {EXPECTED_CONCURRENCY}，报告为 "
             f"{raw.get('player_concurrency')}")
    process = raw.get("process", {})
    rss_observations = process.get("rss_observations")
    rss_retained = process.get("rss_retained_samples")
    if rss_observations != EXPECTED_RSS_OBSERVATIONS \
            or rss_retained != EXPECTED_RSS_OBSERVATIONS:
        fail(f"RSS 采样数应为 {EXPECTED_RSS_OBSERVATIONS}，报告为 "
             f"observations={rss_observations} retained={rss_retained}"
             f"——采样器退化会使峰值失真")
    phases = raw.get("phases", {}).get("phases", {})
    workload = raw.get("workload", {})
    if not isinstance(workload.get("progress_delta"), (int, float)) \
            or workload["progress_delta"] <= 0:
        fail("负载自检失败：progress_delta 非正（夹具未产生真实工作量）")
    for phase in ("project", "run", "settle", "wall"):
        block = phases.get(phase)
        if not isinstance(block, dict) or block.get("count") != TICKS:
            fail(f"相位 {phase} 样本数应为 {TICKS}，报告为 "
                 f"{block.get('count') if isinstance(block, dict) else block}")
    metrics = {
        "cpu_core_ms_per_tick": process.get("cpu_core_ms_per_tick"),
        "wall_p50_ms": phases["wall"]["p50_ms"],
        "wall_p95_ms": phases["wall"]["p95_ms"],
        "wall_p99_ms": phases["wall"]["p99_ms"],
        "project_p50_ms": phases["project"]["p50_ms"],
        "run_p50_ms": phases["run"]["p50_ms"],
        "settle_p50_ms": phases["settle"]["p50_ms"],
        "rss_bytes_peak": process.get("rss_bytes_sampled_peak"),
    }
    for name, value in metrics.items():
        if not isinstance(value, (int, float)) or isinstance(value, bool) \
                or value != value or value <= 0:
            fail(f"指标 {name} 非有限正数：{value!r}")
    injection = raw.get("injection_bytes_sample", {})
    if channel == "json":
        per_player = injection.get("world_json_bytes")
    else:
        envelope = injection.get("envelope_bytes")
        bundle = injection.get("bundle_bytes")
        if not isinstance(envelope, (int, float)) or envelope <= 0 \
                or not isinstance(bundle, (int, float)) or bundle <= 0:
            fail(f"kkpv 注入采样缺失：{injection!r}")
        per_player = envelope + bundle
    if not isinstance(per_player, (int, float)) or per_player <= 0:
        fail(f"每玩家注入字节非正：{injection!r}")
    return {
        "metrics": metrics,
        "per_player_injection_bytes": per_player,
        "git_commit": meta["git_commit"],
        "binary_sha256": binary_sha,
    }


def summarize(runs):
    """按（规模、通道）聚合：各指标五次运行的中位数与最小..最大。"""
    summary = {}
    reference_reps = None
    for players in SCALES:
        for channel in CHANNELS:
            entries = [r for r in runs
                       if r["players"] == players and r["channel"] == channel]
            reps = sorted(e["repetition"] for e in entries)
            if len(reps) != len(set(reps)) or any(r < 0 for r in reps):
                fail(f"{players}p/{channel} 轮次编号重复或非法：{reps}")
            if reference_reps is None:
                reference_reps = reps
            elif reps != reference_reps:
                fail(f"{players}p/{channel} 轮次集合 {reps} 与其他格 "
                     f"{reference_reps} 不一致——缺轮或编号错乱")
            aggregate = {}
            for key in entries[0]["extracted"]["metrics"]:
                values = [e["extracted"]["metrics"][key] for e in entries]
                aggregate[key] = {
                    "median": statistics.median(values),
                    "min": min(values),
                    "max": max(values),
                    "values": values,
                }
            injected = [e["extracted"]["per_player_injection_bytes"]
                        for e in entries]
            aggregate["per_player_injection_bytes"] = {
                "median": statistics.median(injected),
                "min": min(injected),
                "max": max(injected),
                "values": injected,
            }
            summary[f"{players}p/{channel}"] = aggregate
    return summary


def check_settle_canary(summary):
    """结算相与通道无关：两通道 settle p50 中位数之比须接近 1。"""
    for players in SCALES:
        json_settle = summary[f"{players}p/json"]["settle_p50_ms"]["median"]
        kkpv_settle = summary[f"{players}p/kkpv"]["settle_p50_ms"]["median"]
        ratio = kkpv_settle / json_settle
        if abs(ratio - 1.0) > SETTLE_CANARY_RELATIVE_BAND:
            fail(f"{players}p 结算金丝雀偏离超过 ±"
                 f"{SETTLE_CANARY_RELATIVE_BAND * 100:.0f}%：kkpv/json="
                 f"{ratio:.3f}（json={json_settle} kkpv={kkpv_settle}）——"
                 f"机器态或实现漂移，本组不可判读")


def atomic_write(path, payload):
    directory = os.path.dirname(os.path.abspath(path)) or "."
    if os.path.exists(path):
        fail(f"输出路径已存在，拒绝覆盖：{path}")
    if not os.path.isdir(directory):
        fail(f"输出目录不存在：{directory}")
    handle, temp = tempfile.mkstemp(dir=directory, suffix=".tmp")
    try:
        with os.fdopen(handle, "w") as stream:
            json.dump(payload, stream, ensure_ascii=False, indent=1)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temp, path)
    except BaseException:
        if os.path.exists(temp):
            os.unlink(temp)
        raise


def main():
    if len(sys.argv) != 2:
        fail(f"用法：{sys.argv[0]} <输出路径（须不存在）>")
    output = sys.argv[1]
    repo = os.path.dirname(os.path.dirname(os.path.dirname(
        os.path.dirname(os.path.abspath(__file__)))))
    binary = os.path.join(repo, "target", "release", "kkscreeps")
    if not os.path.exists(binary):
        fail(f"未找到 release 二进制：{binary}（先 cargo build --release -p kk-cli）")
    binary_sha = sha256_of(binary)
    repetitions = 5
    runs = []
    for repetition in range(repetitions):
        order = CHANNELS if repetition % 2 == 0 else tuple(reversed(CHANNELS))
        scales = rotated_scales(repetition)
        position = 0
        for players in scales:
            for channel in order:
                report_path = tempfile.mktemp(suffix=".json")
                try:
                    elapsed = run_probe(binary, players, channel, report_path)
                    with open(report_path) as stream:
                        raw = json.load(stream)
                finally:
                    if os.path.exists(report_path):
                        os.unlink(report_path)
                if elapsed > ELAPSED_S_LIMIT:
                    fail(f"单次运行墙钟 {elapsed:.1f}s 超上限 "
                         f"{ELAPSED_S_LIMIT}s（players={players} "
                         f"channel={channel}）——疑似卡死或外部抢载，整组拒收")
                extracted = gate_report(raw, players, channel, binary_sha)
                runs.append({
                    "repetition": repetition,
                    "position_in_repetition": position,
                    "scale_order": list(scales),
                    "order": list(order),
                    "players": players,
                    "channel": channel,
                    "elapsed_s": elapsed,
                    "extracted": extracted,
                    "report": raw,
                })
                position += 1
                print(f"rep{repetition} pos{position - 1} {players}p "
                      f"{channel} "
                      f"cpu={extracted['metrics']['cpu_core_ms_per_tick']:.4f} "
                      f"wall_p50={extracted['metrics']['wall_p50_ms']:.3f}",
                      flush=True)
    commits = {r["extracted"]["git_commit"] for r in runs}
    shas = {r["extracted"]["binary_sha256"] for r in runs}
    if len(commits) != 1 or len(shas) != 1:
        fail(f"采集期间身份变化：commits={commits} shas={shas}")
    summary = summarize(runs)
    check_settle_canary(summary)
    atomic_write(output, {
        "schema": "kkpv-scale-matrix/1",
        "binary_sha256": binary_sha,
        "git_commit": runs[0]["extracted"]["git_commit"],
        "scales": list(SCALES),
        "channels": list(CHANNELS),
        "warmup": WARMUP,
        "ticks": TICKS,
        "repetitions": repetitions,
        "summary": summary,
        "runs": runs,
    })
    print(f"完成：{len(runs)} 个运行，已写入 {output}")


if __name__ == "__main__":
    main()
