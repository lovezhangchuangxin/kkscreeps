"""玩家闭环报告门拒绝无效采样，并在失败时保留已验收窗口。"""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'docs/research/notes/mod-player-workload-capture-2026-10-04.py'
spec = importlib.util.spec_from_file_location('mod_player_capture', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ModPlayerCaptureTests(unittest.TestCase):
    def report(self):
        # v1 原始快照已有真实目标效果，v2 新增逐轮检查；只升级测试报告版本。
        report = json.loads((ROOT / 'docs/research/notes/mod-player-workload-windows-2026-10-04.json').read_text())['windows'][0]['report']
        report['schema_version'] = 2
        return report

    def test_real_workload_passes_and_old_schema_is_rejected(self):
        report = self.report()
        module.validate_report(report, 'json')
        report['schema_version'] = 1
        with self.assertRaises(RuntimeError):
            module.validate_report(report, 'json')

    def test_missing_phases_invalid_metrics_and_wrong_sample_count_are_rejected(self):
        for change in [
            lambda r: r['phases'].update(phases={}),
            lambda r: r['phases']['phases'].pop('settle'),
            lambda r: r['phases']['phases']['wall'].update(count=2999),
            lambda r: r['phases']['phases']['run'].update(p50_ms=True),
            lambda r: r['phases']['phases']['run'].update(total_ms=float('nan')),
            lambda r: r['process'].update(cpu_core_ms_per_tick=False),
            lambda r: r['process'].update(cpu_core_ms_per_tick=-1),
            lambda r: r['process'].update(cpu_core_ms=0),
            lambda r: r['process'].update(rss_bytes_sampled_peak=-1),
            lambda r: r.update(package_digests={}),
            lambda r: r.update(player_budget=None),
        ]:
            report = self.report()
            change(report)
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                module.validate_report(report, 'json')

    def test_consistent_but_empty_work_or_wrong_target_effect_is_rejected(self):
        for change in [
            lambda r: r['workload']['after']['players'][0]['controller'].update(progress=0),
            lambda r: r['workload']['after']['players'][0]['counter']['fields'].update(value=0),
            lambda r: r['workload']['after']['players'].pop(),
            lambda r: r['workload']['window_actions'][1].update(harvests=0),
            lambda r: r['workload']['after'].update(generation=0),
        ]:
            report = self.report()
            change(report)
            with self.subTest(change=change), self.assertRaises(RuntimeError):
                module.validate_report(report, 'json')

    def test_validation_remains_enabled_with_python_optimization(self):
        report=self.report()
        report['metadata']['debug_assertions']=True
        with tempfile.TemporaryDirectory() as temp:
            path=Path(temp)/'invalid.json'
            path.write_text(json.dumps(report))
            code = f"""
import importlib.util
import json
from pathlib import Path
spec=importlib.util.spec_from_file_location('capture',{str(SCRIPT)!r})
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
try: module.validate_report(json.loads(Path({str(path)!r}).read_text()),'json')
except RuntimeError: pass
else: raise RuntimeError('无效报告被接受')
"""
            result = subprocess.run(['rtk', 'proxy', sys.executable, '-O', '-c', code], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_later_failure_preserves_first_accepted_window(self):
        report = self.report()
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / 'capture.json'
            binary = Path(temp) / 'binary'
            binary.write_bytes(b'fixed binary')
            calls = 0

            def run(args, **kwargs):
                nonlocal calls
                calls += 1
                if calls == 1:
                    path = Path(args[args.index('--report') + 1])
                    path.write_text(json.dumps(report))
                    return subprocess.CompletedProcess(args, 0, '', '')
                return subprocess.CompletedProcess(args, 1, '', 'failed-window')

            with patch.object(module.subprocess, 'run', side_effect=run):
                with self.assertRaisesRegex(RuntimeError, 'failed-window'):
                    module.collect(output, binary)
            saved = json.loads(output.read_text())
            self.assertFalse(saved['complete'])
            self.assertEqual(len(saved['windows']), 1)
            self.assertIn('failed-window', saved['error'])
            original = output.read_bytes()
            with self.assertRaises(FileExistsError):
                module.collect(output, binary)
            self.assertEqual(output.read_bytes(), original)


if __name__ == '__main__':
    unittest.main()
