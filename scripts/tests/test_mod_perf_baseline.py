"""性能结果只有通过负载、回调次数与独立采样门才允许汇总。"""

import copy
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "mod_perf_baseline", Path(__file__).resolve().parents[1] / "mod-perf-baseline.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ModPerfGuardTests(unittest.TestCase):
    def report(self):
        checks = {}
        for name, tick in (("before", 0), ("after_warmup", 2), ("after_collection", 5)):
            checks[name] = {"tick": tick, "verified_links": 6, "link_state_sha256": "a" * 64,
                "namespace": dict.fromkeys(("before", "outer", "inner", "after"), tick * 6) if tick else None}
        return {"tool": "kkscreeps mod-perf-probe", "mode": "chain", "rooms": 2,
            "links_per_room": 3, "warmup": 2, "ticks": 3,
            "metadata": {"debug_assertions": False},
            "wall": {"count": 3, "p50_ms": 1, "p95_ms": 1, "p99_ms": 1},
            "process": {"scope": "current_process_all_threads", "cpu_core_ms_per_tick": 1}, "checks": checks}

    def validate(self, report):
        module.validate_report(report, "chain", 2, 3, 2, 3)

    def test_missing_or_repeated_chain_position_is_rejected(self):
        for calls in (0, 60):
            report = self.report()
            report["checks"]["after_collection"]["namespace"]["outer"] = calls
            with self.assertRaisesRegex(RuntimeError, "调用次数不符"):
                self.validate(report)

    def test_only_first_room_verified_is_rejected(self):
        report = self.report()
        report["checks"]["after_collection"]["verified_links"] = 3
        with self.assertRaisesRegex(RuntimeError, "对象验证数量不符"):
            self.validate(report)

    def test_reused_partial_or_debug_reports_are_rejected(self):
        valid = self.report()
        self.validate(valid)
        for key, value in (("count", 1), ("p99_ms", None)):
            report = copy.deepcopy(valid)
            if value is None:
                report["wall"].pop(key)
            else:
                report["wall"][key] = value
            with self.assertRaisesRegex(RuntimeError, "采样数或分位数不符"):
                self.validate(report)
        valid["metadata"]["debug_assertions"] = True
        with self.assertRaisesRegex(RuntimeError, "release"):
            self.validate(valid)

    def test_profile_cannot_be_mixed_into_baseline_or_skip_phase_checks(self):
        report = self.report()
        report["profile"] = {}
        with self.assertRaisesRegex(RuntimeError, "无插桩基线"):
            self.validate(report)
        with self.assertRaisesRegex(RuntimeError, "阶段集合"):
            module.validate_report(report, "chain", 2, 3, 2, 3, profile=True)
        phases = {"slot": 1, "js_call": 1, "budget_start": 1, "budget_finish": 1,
                  "op_native": 1, "host_native": 1, "op_read": 6, "host_read": 6,
                  "op_write": 6, "host_write": 6}
        report["profile"] = {phase: {"calls": factor * 18, "errors": 0, "wall_ns": 1}
                             for phase, factor in phases.items()}
        module.validate_report(report, "chain", 2, 3, 2, 3, profile=True)
        for key, value in (("calls", 1), ("errors", 1), ("wall_ns", -1)):
            bad = copy.deepcopy(report)
            bad["profile"]["op_read"][key] = value
            with self.assertRaises(RuntimeError):
                module.validate_report(bad, "chain", 2, 3, 2, 3, profile=True)

    def test_v2_requires_read_conversion_and_checks_its_counts(self):
        report = self.report()
        report["profile_schema_version"] = 2
        phases = {"slot": 1, "js_call": 1, "budget_start": 1, "budget_finish": 1,
                  "op_native": 1, "host_native": 1, "op_read": 6, "host_read": 6,
                  "op_write": 6, "host_write": 6, "read_to_v8": 6}
        report["profile"] = {phase: {"calls": factor * 18, "errors": 0, "wall_ns": 1}
                             for phase, factor in phases.items()}
        module.validate_report(report, "chain", 2, 3, 2, 3, profile=True)
        for key, value in (("calls", 0), ("calls", 108.0), ("errors", 1), ("errors", False), ("wall_ns", -1), ("wall_ns", True)):
            bad = copy.deepcopy(report)
            bad["profile"]["read_to_v8"][key] = value
            with self.assertRaises(RuntimeError):
                module.validate_report(bad, "chain", 2, 3, 2, 3, profile=True)

        missing = copy.deepcopy(report)
        del missing["profile"]["read_to_v8"]
        with self.assertRaisesRegex(RuntimeError, "阶段集合"):
            module.validate_report(missing, "chain", 2, 3, 2, 3, profile=True)
        for version in (3, "2", None, True):
            bad = copy.deepcopy(report)
            bad["profile_schema_version"] = version
            with self.assertRaisesRegex(RuntimeError, "版本不支持"):
                module.validate_report(bad, "chain", 2, 3, 2, 3, profile=True)

    def test_invalid_timing_numbers_cannot_be_summarized(self):
        for key in ("p50_ms", "p95_ms", "p99_ms"):
            for value in (float('nan'), float('inf'), -1, True, "1"):
                report = self.report()
                report['wall'][key] = value
                with self.subTest(key=key,value=value), self.assertRaises(RuntimeError):
                    self.validate(report)
        for value in (float('nan'), float('inf'), -1, True, None):
            report = self.report()
            report['process']['cpu_core_ms_per_tick'] = value
            with self.subTest(cpu=value), self.assertRaisesRegex(RuntimeError, "CPU"):
                self.validate(report)
        report = self.report()
        report['wall']['p50_ms'] = 2
        with self.assertRaisesRegex(RuntimeError, "有序"):
            self.validate(report)


if __name__ == "__main__":
    unittest.main()
