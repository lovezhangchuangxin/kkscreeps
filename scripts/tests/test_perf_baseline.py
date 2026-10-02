"""以替身进程检查基线工具拒收旧报告及运行期间替换的二进制。"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class BaselineGuardTests(unittest.TestCase):
    def run_probe(self, directory, mode):
        binary = directory / "fake-probe"
        binary.write_text(f"#!{sys.executable}\n" + r'''
import json, sys
from pathlib import Path
players = int(sys.argv[sys.argv.index('--players') + 1])
ticks = int(sys.argv[sys.argv.index('--ticks') + 1])
phase = {'p50_ms': 1, 'p95_ms': 1, 'p99_ms': 1}
if MODE == 'stale':
    phase.pop('p99_ms')
if MODE == 'missing_p95':
    phase.pop('p95_ms')
report = {'players': players, 'ticks': ticks, 'metadata': {'debug_assertions': False},
    'phases': {'phases': {p: phase for p in ('project', 'run', 'settle', 'wall')}},
    'process': {'scope': 'current_process_all_threads', 'cpu_core_ms_per_tick': 1,
        'rss_bytes_sampled_p50': 1, 'rss_bytes_sampled_peak': 1}}
Path(sys.argv[sys.argv.index('--report') + 1]).write_text(json.dumps(report))
if MODE == 'replace_last' and players == 32:
    with Path(__file__).open('a') as output:
        output.write('\n# binary replaced\n')
'''.replace('MODE', repr(mode)))
        binary.chmod(0o755)
        script = Path(__file__).resolve().parents[1] / "perf-baseline.py"
        return subprocess.run(
            [sys.executable, str(script), "--binary", str(binary), "--ticks", "1",
             "--warmup", "0", "--repetitions", "1", "--output", str(directory / "result.json")],
            capture_output=True, text=True,
        )

    def test_stale_report_stops_at_first_run(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            result = self.run_probe(directory, "stale")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("报告缺少 p99", result.stderr)
            self.assertNotIn("完成 players=", result.stdout)
            self.assertFalse((directory / "result.json").exists())

    def test_missing_p95_stops_before_collecting_invalid_runs(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            result = self.run_probe(directory, "missing_p95")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("报告缺少 p95", result.stderr)
            self.assertNotIn("完成 players=", result.stdout)
            self.assertFalse((directory / "result.json").exists())

    def test_replacement_during_last_run_does_not_publish_report(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            result = self.run_probe(directory, "replace_last")
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("测量期间二进制发生变化", result.stderr)
            self.assertFalse((directory / "result.json").exists())
            partial = json.loads((directory / "result.partial.json").read_text())
            self.assertEqual(len(partial["runs"]), 3)


if __name__ == "__main__":
    unittest.main()
