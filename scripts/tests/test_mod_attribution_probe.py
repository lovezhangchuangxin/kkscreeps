"""用受控子进程报告检查前后对照门禁，不运行真实性能负载。"""

import contextlib
import io
import json
import runpy
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


PROBE = (Path(__file__).resolve().parents[2]
         / "docs/research/notes/mod-attribution-probe-2026-10-03.py")


class ModAttributionIdentityTests(unittest.TestCase):
    def run_probe(self, changed_mode=None, changed_field=None, changed_call=2,
                  schema_versions=(1, 1), schema_change=None):
        calls = {}

        def child(command, **_kwargs):
            def arg(name):
                return command[command.index(name) + 1]

            mode = arg("--mode")
            calls[mode] = calls.get(mode, 0) + 1
            warmup, ticks = int(arg("--warmup")), int(arg("--ticks"))
            objects = 400
            checks = {}
            for boundary, elapsed in (("before", 0), ("after_warmup", warmup),
                                      ("after_collection", warmup + ticks)):
                checks[boundary] = {
                    "tick": elapsed, "verified_links": objects,
                    "link_state_sha256": "a" * 64,
                    "namespace": dict.fromkeys(("before", "outer", "inner", "after"),
                                               objects * elapsed)
                    if mode == "chain" and elapsed else None,
                }
            profiled = "--profile" in command
            version = schema_versions[0 if Path(command[0]).name == "profile-before" else 1]
            profile_key = (command[0], profiled)
            calls[profile_key] = calls.get(profile_key, 0) + 1
            if profiled and Path(command[0]).name == "after" and calls[profile_key] == 2 and schema_change:
                version = schema_change
            phases = {"slot": 1, "js_call": 1, "budget_start": 1, "budget_finish": 1,
                      "op_native": 1, "host_native": 1, "op_read": 6, "host_read": 6,
                      "op_write": 6, "host_write": 6}
            if version >= 2:
                phases["read_to_v8"] = 6
            if version >= 3:
                phases["write_from_v8"] = 6
            report = {
                "tool": "kkscreeps mod-perf-probe", "mode": mode, "rooms": 1,
                "links_per_room": objects, "warmup": warmup, "ticks": ticks,
                "metadata": {"debug_assertions": False}, "checks": checks,
                "package_digests": {} if mode == "native" else {"perf": mode + "-v1"},
                "vm_limits": {"settlement_cpu_ms": 100},
                "wall": {"count": ticks, "p50_ms": 1, "p95_ms": 1, "p99_ms": 1},
                "process": {"scope": "current_process_all_threads",
                            "cpu_core_ms_per_tick": 1},
                "profile": {phase: {"calls": factor * objects * ticks,
                                    "errors": 0, "wall_ns": 1}
                            for phase, factor in phases.items()}
                if profiled else None,
                "profile_schema_version": version if profiled else None,
            }
            if mode == changed_mode and calls[mode] == changed_call:
                if changed_field == "package_digests":
                    report[changed_field] = {"perf": "changed-package"}
                else:
                    report[changed_field] = {"settlement_cpu_ms": 200}
            Path(arg("--report")).write_text(json.dumps(report))
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory(prefix="kk-attribution-test-") as temp:
            root = Path(temp)
            binaries = []
            for name in ("before", "profile-before", "after"):
                binary = root / name
                binary.write_bytes(name.encode())
                binaries.append(binary)
            output = root / "result.json"
            argv = [str(PROBE), "--before", str(binaries[0]), "--profile-before",
                    str(binaries[1]), "--after", str(binaries[2]), "--output", str(output)]
            with patch.object(sys, "argv", argv), patch("subprocess.run", side_effect=child), \
                    contextlib.redirect_stdout(io.StringIO()):
                if changed_mode or schema_change:
                    message = "诊断版本或阶段集合发生变化" if schema_change else "包或预算配置发生变化"
                    with self.assertRaisesRegex(RuntimeError, message):
                        runpy.run_path(str(PROBE), run_name="__main__")
                    self.assertFalse(output.exists(), "身份变化不得生成成功报告")
                    partial = json.loads(output.with_suffix(".partial.json").read_text())
                    self.assertIs(partial["complete"], False)
                    return
                runpy.run_path(str(PROBE), run_name="__main__")
            report = json.loads(output.read_text())
            self.assertIs(report["complete"], True)
            self.assertEqual(len(report["runs"]), 24)
            self.assertFalse(output.with_suffix(".partial.json").exists())
            for name, version in zip(("profile_before", "after"), schema_versions):
                phases = report["summary"][f"{name}:profile:chain"]["profile_ms_per_tick"]
                self.assertEqual("read_to_v8" in phases, version >= 2)
                self.assertEqual("write_from_v8" in phases, version >= 3)

    def test_stable_identity_is_compared_within_each_mode(self):
        self.run_probe()

    def test_changed_package_is_rejected_across_versions(self):
        for mode in ("empty", "chain"):
            with self.subTest(mode=mode):
                self.run_probe(mode, "package_digests")

    def test_changed_budget_is_rejected_in_every_mode(self):
        for mode in ("native", "empty", "chain"):
            with self.subTest(mode=mode):
                self.run_probe(mode, "vm_limits")

    def test_changed_identity_is_rejected_in_later_repetitions(self):
        self.run_probe("empty", "package_digests", changed_call=4)

    def test_different_binary_schema_versions_can_be_compared(self):
        self.run_probe(schema_versions=(2, 3))

    def test_schema_cannot_change_within_one_binary(self):
        for original, changed in ((2, 3), (3, 2)):
            with self.subTest(original=original, changed=changed):
                self.run_probe(schema_versions=(2, original), schema_change=changed)


if __name__ == "__main__":
    unittest.main()
