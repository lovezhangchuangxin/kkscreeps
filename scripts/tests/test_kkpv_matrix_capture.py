"""规模矩阵采集脚本的门禁与聚合逻辑测试。"""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'docs/research/notes/kkpv-scale-matrix-capture-2026-10-04.py'
spec = importlib.util.spec_from_file_location('kkpv_matrix_capture', SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def raw_report(players=4, channel='json', progress=120, cpu=0.5,
               settle=0.06, injection=None):
    phases = {}
    for phase, p50 in [('project', 0.2), ('run', 0.2), ('settle', settle),
                       ('wall', 0.45)]:
        phases[phase] = {'count': module.TICKS, 'p50_ms': p50,
                         'p95_ms': p50 * 1.2, 'p99_ms': p50 * 1.3,
                         'max_ms': p50 * 1.4, 'total_ms': p50 * module.TICKS}
    if injection is None:
        if channel == 'json':
            injection = {'channel': channel, 'world_json_bytes': 16772}
        else:
            injection = {'channel': channel, 'envelope_bytes': 3000,
                         'bundle_bytes': 8000, 'public_bundle_bytes': 85000}
    return {
        'channel': channel,
        'players': players,
        'ticks': module.TICKS,
        'warmup': module.WARMUP,
        'player_concurrency': module.EXPECTED_CONCURRENCY,
        'metadata': {'git_commit': 'a' * 40, 'debug_assertions': False},
        'process': {
            'cpu_core_ms_per_tick': cpu,
            'rss_bytes_sampled_peak': 66_000_000,
            'rss_observations': module.EXPECTED_RSS_OBSERVATIONS,
            'rss_retained_samples': module.EXPECTED_RSS_OBSERVATIONS,
        },
        'phases': {'phases': phases},
        'workload': {'progress_delta': progress},
        'injection_bytes_sample': injection,
    }


class GateTests(unittest.TestCase):
    def test_valid_json_report_passes(self):
        extracted = module.gate_report(raw_report(), 4, 'json', 'sha')
        self.assertGreater(extracted['per_player_injection_bytes'], 0)
        self.assertEqual(extracted['metrics']['settle_p50_ms'], 0.06)

    def test_kkpv_injection_sums_envelope_and_private_bundle(self):
        injection = {'channel': 'kkpv', 'envelope_bytes': 3000,
                     'bundle_bytes': 8000, 'public_bundle_bytes': 85000}
        extracted = module.gate_report(
            raw_report(channel='kkpv', injection=injection), 4, 'kkpv', 'sha')
        self.assertEqual(extracted['per_player_injection_bytes'], 11000)

    def test_rejects_channel_mismatch(self):
        with self.assertRaises(RuntimeError):
            module.gate_report(raw_report(channel='kkpv'), 4, 'json', 'sha')

    def test_rejects_zero_progress(self):
        with self.assertRaises(RuntimeError):
            module.gate_report(raw_report(progress=0), 4, 'json', 'sha')

    def test_rejects_non_finite_metric(self):
        report = raw_report()
        report['process']['cpu_core_ms_per_tick'] = float('nan')
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')

    def test_rejects_missing_kkpv_injection_fields(self):
        report = raw_report(channel='kkpv', injection={'channel': 'kkpv'})
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'kkpv', 'sha')

    def test_rejects_phase_count_drift(self):
        report = raw_report()
        report['phases']['phases']['wall']['count'] = module.TICKS - 1
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')

    def test_rejects_debug_assertions_binary(self):
        report = raw_report()
        report['metadata']['debug_assertions'] = True
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')

    def test_rejects_concurrency_drift(self):
        report = raw_report()
        report['player_concurrency'] = 4
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')

    def test_rejects_degraded_rss_sampler(self):
        report = raw_report()
        report['process']['rss_observations'] = 1
        report['process']['rss_retained_samples'] = 1
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')

    def test_rejects_players_mismatch(self):
        with self.assertRaises(RuntimeError):
            module.gate_report(raw_report(players=8), 4, 'json', 'sha')

    def test_rejects_window_mismatch(self):
        report = raw_report()
        report['ticks'] = module.TICKS + 1
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')

    def test_rejects_missing_git_commit(self):
        report = raw_report()
        del report['metadata']['git_commit']
        with self.assertRaises(RuntimeError):
            module.gate_report(report, 4, 'json', 'sha')


class RotationTests(unittest.TestCase):
    def test_scale_order_rotates_every_repetition(self):
        orders = [module.rotated_scales(r) for r in range(module.SCALES.__len__())]
        self.assertEqual(orders[0], (4, 8, 32))
        self.assertEqual(orders[1], (8, 32, 4))
        self.assertEqual(orders[2], (32, 4, 8))
        # 每个规模在各位置恰出现一次（位置混杂均衡）。
        for position in range(3):
            column = {orders[r][position] for r in range(3)}
            self.assertEqual(column, {4, 8, 32})


class SummarizeTests(unittest.TestCase):
    def runs(self, settle_by_channel=None):
        settle_by_channel = settle_by_channel or {'json': 0.06, 'kkpv': 0.06}
        runs = []
        for repetition in range(5):
            order = module.CHANNELS if repetition % 2 == 0 else \
                tuple(reversed(module.CHANNELS))
            for players in module.SCALES:
                for channel in order:
                    report = raw_report(
                        players=players, channel=channel,
                        settle=settle_by_channel[channel])
                    runs.append({
                        'repetition': repetition,
                        'players': players,
                        'channel': channel,
                        'extracted': module.gate_report(
                            report, players, channel, 'sha'),
                    })
        return runs

    def test_summary_has_every_cell_with_five_values(self):
        summary = module.summarize(self.runs())
        for players in module.SCALES:
            for channel in module.CHANNELS:
                cell = summary[f'{players}p/{channel}']
                self.assertEqual(
                    len(cell['cpu_core_ms_per_tick']['values']), 5)

    def test_missing_repetition_is_rejected(self):
        runs = self.runs()[:-1]
        with self.assertRaises(RuntimeError):
            module.summarize(runs)

    def test_settle_canary_rejects_divergent_channels(self):
        summary = module.summarize(self.runs(
            settle_by_channel={'json': 0.06, 'kkpv': 0.10}))
        with self.assertRaises(RuntimeError):
            module.check_settle_canary(summary)

    def test_settle_canary_passes_equal_channels(self):
        module.check_settle_canary(module.summarize(self.runs()))


class AtomicWriteTests(unittest.TestCase):
    def test_refuses_existing_output(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'out.json'
            target.write_text('{}')
            with self.assertRaises(RuntimeError):
                module.atomic_write(str(target), {'a': 1})
            self.assertEqual(json.loads(target.read_text()), {})

    def test_writes_new_output(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'out.json'
            module.atomic_write(str(target), {'a': 1})
            self.assertEqual(json.loads(target.read_text()), {'a': 1})


if __name__ == '__main__':
    unittest.main()
