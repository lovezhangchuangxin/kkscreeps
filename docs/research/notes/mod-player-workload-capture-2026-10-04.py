"""顺序采集十个独立玩家/Mod 窗口；原始报告内嵌，失败保留已完成窗口。"""

import hashlib
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[3]
BIN = ROOT / 'target/release/kkscreeps'
OUTPUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_suffix('.json')


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def finite(value):
    return type(value) in (int, float) and math.isfinite(value) and value >= 0


def validate_report(report, channel):
    """常驻门禁：优化模式也不能跳过；拒绝缺阶段、假数值和一致的空转报告。"""
    try:
        require(report['metadata']['debug_assertions'] is False, '须使用 release')
        require(report['tool'] == 'kkscreeps mod-player-probe', '工具不符')
        expected = {'schema_version': 2, 'players': 2, 'player_concurrency': 2,
                    'warmup': 500, 'ticks': 3000}
        require(all(type(report[k]) is int and report[k] == v for k, v in expected.items()), '采集配置不符')
        require(report['channel'] == channel, '通道不符')
        phases = report['phases']
        require(type(phases['ticks']) is int and phases['ticks'] == 3000, '阶段样本数不符')
        require(set(phases['phases']) == {'project', 'run', 'settle', 'wall'}, '阶段集合不符')
        for phase in phases['phases'].values():
            require(type(phase['count']) is int and phase['count'] == 3000, '阶段样本数不符')
            values = [phase[key] for key in ['p50_ms', 'p95_ms', 'p99_ms', 'max_ms']]
            require(all(finite(v) for v in values) and values == sorted(values), '分位数不符')
            total = phase['total_ms']
            require(finite(total) and total + 1e-8 >= values[-1]
                    and total <= values[-1] * 3000 + 1e-8, '阶段总耗时不符')
        process = report['process']
        require(process['scope'] == 'current_process_all_threads'
                and process['window'] == 'after_warmup_through_collection_loop'
                and process['includes_sampling_and_workload_checks'] is True, '进程口径不符')
        require(type(process['rss_observations']) is int and process['rss_observations'] == 3001, 'RSS 样本数不符')
        for key in ['wall_ms', 'cpu_core_ms', 'cpu_core_ms_per_tick', 'rss_sampling_wall_ms']:
            require(finite(process[key]), 'CPU/墙钟数值无效')
        require(math.isclose(process['cpu_core_ms'] / 3000, process['cpu_core_ms_per_tick'], rel_tol=1e-9, abs_tol=1e-9), 'CPU 均值不符')
        for key in ['rss_bytes_after', 'rss_bytes_sampled_peak', 'rss_bytes_sampled_p50']:
            require(type(process[key]) is int and process[key] > 0, 'RSS 数值无效')
        require(process['rss_bytes_sampled_peak'] >= max(process['rss_bytes_after'], process['rss_bytes_sampled_p50']), 'RSS 峰值不符')
        import re
        require(set(report['package_digests']) == {'workload'}, '包身份缺失')
        require(all(isinstance(v, str) and re.fullmatch('[0-9a-f]{64}', v) for v in
                    [report['package_digests']['workload'], report['player_code_sha256']]), '指纹无效')
        require(set(report['vm_limits']) == {'registration_cpu_ms', 'registration_wall_ms', 'settlement_cpu_ms', 'settlement_wall_ms', 'memory_bytes'}
                and all(type(v) is int and v > 0 for v in report['vm_limits'].values()), 'Mod 预算不符')
        require(set(report['player_budget']) == {'limit', 'bucket_cap', 'initial_bucket', 'max_per_tick'}
                and all(finite(v) and v > 0 for v in report['player_budget'].values()), '玩家预算不符')
        workload = report['workload']
        initial = workload['before']
        for name, round_count in [('before', 0), ('warmed', 500), ('after', 3500)]:
            boundary = workload[name]
            for key in ['time', 'generation']:
                require(type(boundary[key]) is int and boundary[key] == initial[key] + round_count, '边界时钟不符')
            require(boundary['mod_state'] == ({'increments': round_count * 2} if round_count else None), 'Mod 累计次数不符')
            require([p['handle'] for p in boundary['players']] == ['p1', 'p2'], '玩家集合不符')
            harvests = max(0, (round_count - 48) // 3)
            upgrades = round_count - harvests
            for player in boundary['players']:
                require(type(player['upgrades']) is int and player['upgrades'] == upgrades
                        and type(player['harvests']) is int and player['harvests'] == harvests, '原生动作次数不符')
                counter=player['counter']['fields']['value']
                progress=player['controller']['progress']
                energy=player['creep']['store'].get('energy',0)
                ttl=player['creep']['ticksToLive']
                require(type(counter) is int and counter == round_count, 'Mod 对象计数不符')
                require(player['controller']['level'] == 4 and type(progress) is int and progress == upgrades, '控制器效果不符')
                require(type(energy) is int and energy == 50 + 2 * harvests - upgrades
                        and type(ttl) is int and ttl == 3501 - round_count, 'worker 状态不符')
                require(len(player['sources']) == 1 and player['sources'][0]['energyCapacity'] == 3000, 'source 缺失')
                require(type(player['sources'][0]['energy']) is int and 0 <= player['sources'][0]['energy'] <= 3000, 'source 能量无效')
        require(workload['window_actions'] == [dict(player=h, executions=3000, mod_increments=3000,
                                                   upgrades=2000, harvests=1000) for h in ['p1', 'p2']], '窗口动作次数不符')
        return {key: report[key] for key in
                ['workload', 'package_digests', 'player_code_sha256', 'vm_limits', 'player_budget']}
    except (KeyError, TypeError, ValueError, OverflowError) as error:
        raise RuntimeError(f'报告字段无效: {error}') from error


def collect(output, binary=BIN):
    binary_sha = sha(binary)
    result = {'complete': False, 'binary_sha256': binary_sha,
              'capture_sha256': sha(Path(__file__)), 'windows': []}
    reference = None
    # 独占路径；后续同目录原子替换，写入中断仍保留上一份完整归档。
    with output.open('x'):
        pass

    def save():
        import os
        with tempfile.NamedTemporaryFile(mode='w', dir=output.parent, delete=False) as temp:
            temp_path = Path(temp.name)
            try:
                json.dump(result, temp, ensure_ascii=False, indent=2, allow_nan=False)
                temp.flush()
                os.fsync(temp.fileno())
                os.replace(temp_path, output)
            finally:
                temp_path.unlink(missing_ok=True)

    save()
    try:
        with tempfile.TemporaryDirectory(prefix='kk-mod-player-capture-') as temp:
            for repetition in range(1, 6):
                channels = ['json', 'kkpv'] if repetition % 2 else ['kkpv', 'json']
                for channel in channels:
                    require(sha(binary) == binary_sha, '采集中二进制变化')
                    report_path = Path(temp) / f'{repetition}-{channel}.json'
                    args = [str(binary), 'mod-player-probe', '--players', '2',
                            '--concurrency', '2', '--warmup', '500', '--ticks', '3000',
                            '--report', str(report_path)]
                    if channel == 'kkpv':
                        args.append('--kkpv')
                    started = time.time()
                    run = subprocess.run(['rtk', 'proxy', *args], cwd=ROOT,
                                         capture_output=True, text=True, timeout=120)
                    require(run.returncode == 0, f'{args}: {run.returncode}: {run.stderr}')
                    require(sha(binary) == binary_sha, '采集中二进制变化')
                    report = json.loads(report_path.read_text())
                    identity = validate_report(report, channel)
                    if reference is None:
                        reference = identity
                    require(identity == reference, '工作量或身份变化')
                    result['windows'].append({'repetition': repetition, 'channel': channel,
                                              'argv': args, 'exit_code': run.returncode,
                                              'started_unix_s': started, 'elapsed_s': time.time() - started,
                                              'stdout': run.stdout, 'stderr': run.stderr, 'report': report})
                    save()
                    print(f'{repetition} {channel}: captured', flush=True)
        result['complete'] = True
    except BaseException as error:
        result['error'] = repr(error)
        save()
        raise
    save()


if __name__ == '__main__':
    collect(OUTPUT)
