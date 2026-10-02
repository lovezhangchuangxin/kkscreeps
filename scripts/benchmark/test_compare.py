"""Regression checks for refusing invalid or incomparable benchmark reports."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import compare


class ValidationTests(unittest.TestCase):
    def setUp(self):
        self.shared = compare.fixture(1, 110)
        room = dict(room='W0N1', progress=0, level=4,
                    sources=[dict(x=2, y=2, energy=3000, capacity=3000),
                             dict(x=1, y=3, energy=3000, capacity=3000)],
                    creeps=[dict(name=f'w100_{x}_{y}', x=x, y=y, energy=50,
                                 hits=300, body=['work', 'carry', 'move'], spawning=False)
                            for x, y in ((1, 2), (2, 1), (3, 1))],
                    spawns=[dict(name='Spawn100', x=3, y=3, hits=5000, energy=300, spawning=False)])
        before, after = copy.deepcopy(room), copy.deepcopy(room)
        before['progress'], after['progress'] = 30, 337
        self.report = dict(engine='kkscreeps', players=1, warmup=10, ticks=100, concurrency=2,
                           metadata=dict(debug_assertions=False), cpu_budget=dict(limit=100,bucket=10000,tick_limit=500),
                           phases=dict(phases=dict(wall=dict(count=100, p50_ms=1, p95_ms=2, p99_ms=3))),
                           process=dict(cpu_core_ms_per_tick=1, rss_bytes_sampled_p50=100, rss_bytes_sampled_peak=200),
                           initial=[room], before=[before], after=[after],
                           completion={'100': ['BENCH_DONE:' + json.dumps(dict(ticks=110, spawn=2, harvest=20, upgrade=300))]})

    def validate(self):
        compare.validate(self.report, self.shared, 10, 100, 2)

    def test_complete_report(self):
        self.validate()
        compare.equivalent(self.report, copy.deepcopy(self.report))

    def test_missing_percentile(self):
        del self.report['phases']['phases']['wall']['p95_ms']
        with self.assertRaisesRegex(ValueError, 'p95'):
            self.validate()

    def test_equal_but_wrong_initial_state(self):
        self.report['initial'][0]['sources'][0]['x'] = None
        compare.equivalent(self.report, copy.deepcopy(self.report))
        with self.assertRaisesRegex(ValueError, 'initial state'):
            self.validate()

    def test_skipped_player_tick(self):
        self.report['completion']['100'] = ['BENCH_DONE:' + json.dumps(dict(ticks=109, spawn=2, harvest=20, upgrade=300))]
        with self.assertRaisesRegex(ValueError, 'skipped tick'):
            self.validate()

    def test_mismatched_budget(self):
        self.report['cpu_budget']['limit'] = 20
        with self.assertRaisesRegex(ValueError, 'CPU budget'):
            self.validate()

    def test_missing_player_receipt(self):
        self.report['completion'] = {}
        with self.assertRaisesRegex(ValueError, 'missing player'):
            self.validate()

    def test_different_world(self):
        other = copy.deepcopy(self.report)
        other['after'][0]['progress'] += 1
        with self.assertRaisesRegex(ValueError, 'unequal after'):
            compare.equivalent(self.report, other)

    def test_different_accepted_actions(self):
        other = copy.deepcopy(self.report)
        other['completion']['100'] = ['BENCH_DONE:' + json.dumps(dict(ticks=110, spawn=2, harvest=21, upgrade=300))]
        with self.assertRaisesRegex(ValueError, 'action counts'):
            compare.equivalent(self.report, other)

    def test_timeout_terminates_process(self):
        with tempfile.TemporaryDirectory() as directory:
            pid_path = Path(directory) / 'pid'
            script = 'import os,time;from pathlib import Path;Path("pid").write_text(str(os.getpid()));time.sleep(30)'
            with self.assertRaises(subprocess.TimeoutExpired):
                compare.run_process([sys.executable, '-c', script], directory, Path(directory) / 'log', 1)
            pid = int(pid_path.read_text())
            import os
            with self.assertRaises(ProcessLookupError):
                os.kill(pid, 0)


if __name__ == '__main__':
    unittest.main()
