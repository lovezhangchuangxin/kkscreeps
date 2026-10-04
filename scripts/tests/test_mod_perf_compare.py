"""受控子进程覆盖成对采集口径、身份拒收与原证据保护。"""

import contextlib
import copy
import importlib.util
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

PROBE = Path(__file__).resolve().parents[1] / "mod-perf-compare.py"
SPEC = importlib.util.spec_from_file_location("mod_perf_compare", PROBE)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ModPerfCompareTests(unittest.TestCase):
    def collect(self, changed=None, separate_profile_before=False, verify_reservation=False,
                order_pattern="adjacent", linear_drift=False):
        calls = []
        seen = {}

        def child(command, **kwargs):
            def arg(name):
                return command[command.index(name) + 1]

            version = Path(command[0]).name
            mode, profile = arg("--mode"), "--profile" in command
            calls.append((version, mode, profile))
            if verify_reservation and len(calls) == 1:
                partial = json.loads(out.with_suffix(".partial.json").read_text())
                self.assertEqual(partial["runs"], [])
                self.assertIs(partial["complete"], False)
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    MODULE.main()
                self.assertEqual(len(calls), 1, "同路径竞争者不得启动子进程")
            if changed == "missing-report" and len(calls) == 2:
                return subprocess.CompletedProcess(command, 0, "", "")
            if changed == "timeout" and len(calls) == 2:
                raise subprocess.TimeoutExpired(command, kwargs["timeout"])
            key = (version, mode, profile)
            seen[key] = seen.get(key, 0) + 1
            warmup, ticks = int(arg("--warmup")), int(arg("--ticks"))
            rooms, links = int(arg("--rooms")), int(arg("--links-per-room"))
            objects = rooms * links
            checks = {boundary: {
                "tick": elapsed, "verified_links": objects, "link_state_sha256": "a" * 64,
                "namespace": dict.fromkeys(("before", "outer", "inner", "after"), elapsed * objects)
                if mode == "chain" and elapsed else None,
            } for boundary, elapsed in (("before", 0), ("after_warmup", warmup),
                                        ("after_collection", warmup + ticks))}
            schema = 2 if version in ("before", "profile-before") else 3
            if changed == "schema" and key == ("after", "chain", True) and seen[key] == 2:
                schema = 2
            phases = {"slot": 1, "js_call": 1, "budget_start": 1, "budget_finish": 1,
                      "op_native": 1, "host_native": 1, "op_read": 6, "host_read": 6,
                      "op_write": 6, "host_write": 6, "read_to_v8": 6}
            if schema == 3:
                phases["write_from_v8"] = 6
            report = {
                "tool": "kkscreeps mod-perf-probe", "mode": mode, "rooms": rooms,
                "links_per_room": links, "warmup": warmup, "ticks": ticks,
                "metadata": {"debug_assertions": False}, "checks": checks,
                "package_digests": {} if mode == "native" else {"perf": mode},
                "vm_limits": {"settlement_cpu_ms": 100},
                "wall": {"count": ticks, "p50_ms": 1, "p95_ms": 2, "p99_ms": 3},
                "process": {"scope": "current_process_all_threads", "cpu_core_ms_per_tick": 2},
                "profile_schema_version": schema if profile else None,
                "profile": {phase: {"calls": factor * objects * ticks, "errors": 0, "wall_ns": 1}
                            for phase, factor in phases.items()} if profile else None,
            }
            if linear_drift:
                value = 10 + len(calls)
                report['wall'].update(p50_ms=value, p95_ms=value + 1, p99_ms=value + 2)
                report['process']['cpu_core_ms_per_tick'] = value
            if changed == "package" and key == ("after", "chain", False):
                report["package_digests"] = {"perf": "changed"}
            if changed == "state" and key == ("after", "chain", False):
                report["checks"]["after_collection"]["link_state_sha256"] = "b" * 64
            Path(arg("--report")).write_text(json.dumps(report))
            if changed == "late-output" and len(calls) == 2:
                out.write_text("original evidence")
            if changed == "binary" and len(calls) == 2:
                Path(command[0]).write_bytes(b"changed binary")
            self.assertEqual(kwargs["timeout"], 9)
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory(prefix="kk-mod-compare-test-") as temp:
            root = Path(temp)
            for name in ("before", "after", "profile-before"):
                (root / name).write_bytes(name.encode())
            out = root / "result.json"
            argv = [str(PROBE), "--before", str(root / "before"), "--after", str(root / "after"),
                    "--output", str(out), "--rooms", "2", "--links-per-room", "3", "--warmup", "2",
                    "--ticks", "3", "--repetitions", "3", "--timeout", "9", "--profile"]
            argv.extend(["--order-pattern", order_pattern])
            if separate_profile_before:
                argv.extend(["--profile-before", str(root / "profile-before")])
            with patch.object(sys, "argv", argv), patch.object(MODULE.platform, "platform", return_value="test-platform"), \
                    patch("subprocess.run", side_effect=child), \
                    contextlib.redirect_stdout(io.StringIO()):
                if changed:
                    error, message = {
                        "schema": (RuntimeError, "诊断版本或阶段集合"),
                        "package": (RuntimeError, "包或预算"),
                        "state": (RuntimeError, "状态不等价"),
                        "binary": (RuntimeError, "二进制发生变化"),
                        "missing-report": (FileNotFoundError, "report.json"),
                        "timeout": (subprocess.TimeoutExpired, "timed out"),
                        "late-output": (FileExistsError, "result.json"),
                    }[changed]
                    with self.assertRaisesRegex(error, message):
                        MODULE.main()
                    if changed == "late-output":
                        self.assertEqual(out.read_text(), "original evidence")
                    else:
                        self.assertFalse(out.exists())
                    partial = json.loads(out.with_suffix(".partial.json").read_text())
                    self.assertIs(partial["complete"], False)
                    self.assertEqual(len(partial["runs"]), len(calls) if changed == "late-output" else len(calls) - 1)
                    return
                MODULE.main()
            result = json.loads(out.read_text())
            self.assertIs(result["complete"], True)
            abba = order_pattern == 'abba'
            self.assertEqual(len(result["runs"]), 48 if abba else 24)
            self.assertFalse(out.with_suffix(".partial.json").exists())
            # 两端逐对相邻，第二轮先跑 after，模式轮换。
            self.assertEqual(calls[:2], [("before", "native", False), ("after", "native", False)])
            offset = 16 if abba else 8
            self.assertEqual(calls[offset:offset + 2], [("after", "empty", False), ("before", "empty", False)])
            if abba:
                self.assertEqual(calls[2:4], [("after", "native", False), ("before", "native", False)])
                self.assertEqual(calls[18:20], [("before", "empty", False), ("after", "empty", False)])
            summary = result["summary"]
            self.assertEqual(summary["plain:chain"]["pairs"], 6 if abba else 3)
            self.assertEqual(summary["plain:chain"]["rounds"], 3)
            self.assertEqual(summary["plain:chain"]["order_pattern"], order_pattern)
            self.assertNotIn("profile_ms_per_tick", summary["plain:chain"])
            self.assertNotIn("write_from_v8", summary["profile:chain"]["profile_ms_per_tick"]["before"])
            self.assertIn("write_from_v8", summary["profile:chain"]["profile_ms_per_tick"]["after"])
            self.assertEqual("profile_before" in result["binaries"], separate_profile_before)
            if separate_profile_before:
                self.assertIn(("profile-before", "chain", True), calls)
                self.assertNotIn(("profile-before", "chain", False), calls)
            return result, calls

    def test_requested_load_order_and_separate_diagnostics(self):
        for separate in (False, True):
            with self.subTest(separate_profile_before=separate):
                self.collect(separate_profile_before=separate)

    def test_abba_balances_each_round_and_keeps_separate_diagnostics(self):
        result, _ = self.collect(order_pattern='abba', separate_profile_before=True, linear_drift=True)
        for item in result['summary'].values():
            self.assertEqual(item['rounds'], 3)
            self.assertEqual(item['pairs'], 6)
            for metric in item['metrics'].values():
                self.assertEqual(metric['round_after_minus_before'], [0, 0, 0])
                self.assertEqual(metric['round_after_over_before'], [1, 1, 1])
                self.assertEqual(len(metric['paired_after_over_before']), 6)
        # 不以一个样本数组混淆相关的配对和独立轮次。
        p50 = result['summary']['plain:native']['metrics']['p50_ms']
        self.assertNotEqual(p50['median_paired_ratio'], p50['median_round_ratio'])

    def test_abba_failure_keeps_completed_prefix(self):
        for changed in ('schema', 'state', 'timeout'):
            with self.subTest(changed=changed):
                self.collect(changed, order_pattern='abba')

    def test_identity_schema_and_state_faults_preserve_only_valid_partial(self):
        for changed in ("package", "schema", "state", "binary", "missing-report", "timeout"):
            with self.subTest(changed=changed):
                self.collect(changed)

    def test_output_created_during_collection_is_not_overwritten(self):
        self.collect("late-output")

    def test_partial_reservation_blocks_concurrent_collection(self):
        self.collect(verify_reservation=True)

    def test_interrupted_partial_update_preserves_previous_report(self):
        with tempfile.TemporaryDirectory(prefix="kk-mod-compare-write-") as temp:
            path = Path(temp) / "partial.json"
            path.write_text('{"complete": false, "runs": [1]}')

            def interrupted(_report, file, **_kwargs):
                file.write('{"broken":')
                raise OSError("模拟写入中断")

            with patch.object(MODULE.json, "dump", side_effect=interrupted):
                with self.assertRaisesRegex(OSError, "模拟写入中断"):
                    MODULE.write_report(path, {"runs": [1, 2]})
            self.assertEqual(json.loads(path.read_text())["runs"], [1])
            self.assertEqual(list(Path(temp).iterdir()), [path])

    def pair_reports(self, before, after):
        return [{"mode": "chain", "instrumented": False, "profile": None,
                 "version": version, "repetition": rep,
                 "wall": dict.fromkeys(("p50_ms", "p95_ms", "p99_ms"), value),
                 "process": {"cpu_core_ms_per_tick": value}}
                for rep, pair in enumerate(zip(before, after), 1)
                for version, value in zip(("before", "after"), pair)]

    def test_pair_ratios_are_not_ratio_of_independent_medians(self):
        result = MODULE.paired_summary(self.pair_reports([1, 100, 1000], [10, 50, 1000]))
        metric = result["plain:chain"]["metrics"]["p99_ms"]
        self.assertEqual(metric["paired_after_over_before"], [10, 0.5, 1])
        self.assertEqual(metric["median_paired_ratio"], 1)
        self.assertEqual(metric["median_after"] / metric["median_before"], 0.5)
        self.assertEqual(metric["paired_after_minus_before"], [9, -50, 0])

    def test_zero_baseline_retains_sample_without_inventing_ratio(self):
        result = MODULE.paired_summary(self.pair_reports([0, 1], [2, 2]))
        metric = result["plain:chain"]["metrics"]["p99_ms"]
        self.assertEqual(metric["paired_after_over_before"], [None, 2])
        self.assertIsNone(metric["median_paired_ratio"])
        self.assertEqual(metric["paired_after_minus_before"], [2, 1])

    def test_abba_zero_denominator_and_missing_second_pair(self):
        runs = self.pair_reports([0, 0], [2, 2])
        for run in runs:
            run['pair'] = run['repetition']
            run['repetition'] = 1
            run['order_pattern'] = 'abba'
        result = MODULE.paired_summary(runs)['plain:chain']['metrics']['p99_ms']
        self.assertEqual(result['round_before_means'], [0])
        self.assertEqual(result['round_after_minus_before'], [2])
        self.assertEqual(result['round_after_over_before'], [None])
        self.assertIsNone(result['median_round_ratio'])
        with self.assertRaisesRegex(RuntimeError, '轮次不完整'):
            MODULE.paired_summary(runs[:2])
        mixed = copy.deepcopy(runs)
        mixed[-1]['order_pattern'] = 'adjacent'
        with self.assertRaisesRegex(RuntimeError, '采集顺序'):
            MODULE.paired_summary(mixed)
        for value in (True, 0, 3):
            bad = copy.deepcopy(runs)
            bad[0]['pair'] = value
            with self.subTest(pair=value), self.assertRaisesRegex(RuntimeError, '成对样本'):
                MODULE.paired_summary(bad)

    def test_missing_or_duplicate_pair_is_rejected(self):
        runs = self.pair_reports([1], [2])
        for bad in (runs[:1], runs + [copy.deepcopy(runs[0])]):
            with self.subTest(runs=len(bad)), self.assertRaisesRegex(RuntimeError, "成对样本"):
                MODULE.paired_summary(bad)

    def test_existing_output_and_invalid_options_never_start_child(self):
        with tempfile.TemporaryDirectory(prefix="kk-mod-compare-args-") as temp:
            out = Path(temp) / "result.json"
            for flags in (["--ticks", "0"], ["--repetitions", "0"], ["--rooms", "5"],
                          ["--links-per-room", "401"], ["--timeout", "0"],
                          ["--order-pattern", "random"],
                          ["--modes", "chain", "chain"], ["--profile-before", "unused"],
                          ["--profile", "--modes", "native"]):
                argv = [str(PROBE), "--before", "unused", "--output", str(out), *flags]
                with self.subTest(flags=flags), patch.object(sys, "argv", argv), \
                        patch("subprocess.run") as child, contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit):
                        MODULE.main()
                    child.assert_not_called()
            for existing in (out, out.with_suffix(".partial.json")):
                existing.write_text("original evidence")
                with patch.object(sys, "argv", [str(PROBE), "--before", "unused", "--output", str(out)]), \
                        patch("subprocess.run") as child, contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit):
                        MODULE.main()
                    child.assert_not_called()
                self.assertEqual(existing.read_text(), "original evidence")
                existing.unlink()


if __name__ == "__main__":
    unittest.main()
