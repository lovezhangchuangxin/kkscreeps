#!/usr/bin/env python3
"""Run fresh production sandboxes against one shared fixture; reject unequal work."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import statistics
import subprocess
import tempfile
import time

import mature

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def tree_digest(root):
    """Include paths, source and compiled payload; exclude generated loader schema."""
    h = hashlib.sha256()
    for path in sorted(Path(root).rglob('*')):
        if path.is_file() and path.name != 'config.schema.json':
            h.update(str(path.relative_to(root)).encode())
            h.update(bytes.fromhex(digest(path)))
    return h.hexdigest()


def command(args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def fixture(players, total_ticks):
    terrain = [int(x in (0, 49) or y in (0, 49) or (x, y) in ((1, 1), (2, 2), (1, 3)))
               for y in range(50) for x in range(50)]
    rooms, objects, ids = [], [], []
    for i in range(players):
        owner, room = str(100 + i), f'W{i}N1'
        ids.append(owner)
        rooms.append(dict(name=room, owner=owner, rcl=4, terrain=terrain))
        for pos in ((2, 2), (1, 3)):
            objects.append(dict(room=room, kind='structure', spec=dict(
                pos=pos, structureType='source', energy=3000, energyCapacity=3000)))
        objects.append(dict(room=room, kind='structure', spec=dict(
            pos=[3, 3], structureType='spawn', owner=owner, name='Spawn' + owner, store=dict(energy=300))))
        for x, y in ((2, 1), (1, 2), (3, 1)):
            objects.append(dict(room=room, kind='creep', spec=dict(
                pos=[x, y], name=f'w{owner}_{x}_{y}', owner=owner,
                body=['work', 'carry', 'move'], store=dict(energy=50))))
    # Verify the loaded terrain through the player API before issuing the first intents.
    check = '''if (Game.time === 1) {
  if (Game.cpu.limit !== 100 || Game.cpu.tickLimit !== 500 || Game.cpu.bucket !== 10000)
    throw new Error('CPU budget mismatch');
  for (const room of Object.values(Game.rooms)) {
    if (!room.controller.my || room.controller.level !== 4) throw new Error('Controller mismatch');
    const terrain = room.getTerrain();
    for (let y = 0; y < 50; y++) for (let x = 0; x < 50; x++) {
      const expected = +(x === 0 || x === 49 || y === 0 || y === 49 ||
        x === 1 && y === 1 || x === 2 && y === 2 || x === 1 && y === 3);
      if (terrain.get(x, y) !== expected) throw new Error('Terrain mismatch ' + room.name + ':' + x + ',' + y);
    }
  }
}
'''
    code = check + (HERE / 'player.js').read_text() + (
        f'\nif (Game.time === {total_ticks}) console.log("BENCH_DONE:" + JSON.stringify(counters));\n')
    return dict(shard=dict(players=ids, rooms=rooms), objects=objects, code=code, cpu_limit=100)


def run_process(args, cwd, log, timeout):
    # Kill the whole process group on failure/timeout, including any child services.
    with Path(log).open('w') as output:
        proc = subprocess.Popen(args, cwd=cwd, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            status = proc.wait(timeout=timeout)
            if status:
                raise RuntimeError(f'exit {status}: {args[0]}; see {log}')
        except BaseException:
            try:
                os.killpg(proc.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            proc.wait()
            raise


def counters(report):
    result = {}
    for player, lines in report['completion'].items():
        matches = [line.split('BENCH_DONE:', 1)[1] for line in lines if 'BENCH_DONE:' in line]
        if len(matches) != 1:
            raise ValueError(f'{player}: expected one completion receipt')
        result[player] = json.loads(matches[0])
    return result


def validate(report, shared, warmup, ticks, concurrency):
    for key, expected in dict(players=len(shared['shard']['players']), warmup=warmup,
                              ticks=ticks, concurrency=concurrency).items():
        if report.get(key) != expected:
            raise ValueError(f'{key}: expected {expected}')
    if shared.get("profile") == mature.PROFILE:
        mature.validate(report, shared, warmup, ticks, counters(report))
    expected_initial = []
    for room in ([] if shared.get("profile") == mature.PROFILE else shared['shard']['rooms']):
        sources, creeps, spawns = [], [], []
        for obj in shared['objects']:
            if obj['room'] != room['name']:
                continue
            spec = obj['spec']
            x, y = spec['pos']
            if obj['kind'] == 'creep':
                creeps.append(dict(name=spec['name'], x=x, y=y, energy=spec['store']['energy'],
                                   hits=300, body=spec['body'], spawning=False))
            elif spec['structureType'] == 'source':
                sources.append(dict(x=x, y=y, energy=spec['energy'], capacity=spec['energyCapacity']))
            else:
                spawns.append(dict(name=spec['name'], x=x, y=y, energy=spec['store']['energy'], hits=5000, spawning=False))
        expected_initial.append(dict(room=room['name'], progress=0, level=room['rcl'],
            sources=sorted(sources, key=lambda o: (o['y'], o['x'])),
            creeps=sorted(creeps, key=lambda o: o['name']), spawns=sorted(spawns, key=lambda o: o['name'])))
    if shared.get('profile') != mature.PROFILE and report['initial'] != expected_initial:
        raise ValueError('decoded initial state does not match fixture')

    wall = report['phases']['phases']['wall']
    if wall['count'] != ticks:
        raise ValueError('missing tick samples')
    for key in ('p50_ms', 'p95_ms', 'p99_ms'):
        if type(wall.get(key)) not in (int, float) or not 0 < wall[key] < float('inf'):
            raise ValueError(f'invalid wall {key}')
    if not wall['p50_ms'] <= wall['p95_ms'] <= wall['p99_ms']:
        raise ValueError('unordered wall percentiles')
    for key in ('cpu_core_ms_per_tick', 'rss_bytes_sampled_p50', 'rss_bytes_sampled_peak'):
        if type(report['process'].get(key)) not in (int, float) or not 0 < report['process'][key] < float('inf'):
            raise ValueError(f'invalid process {key}')
    if report['process']['rss_bytes_sampled_peak'] < report['process']['rss_bytes_sampled_p50']:
        raise ValueError('RSS peak below median')
    done = counters(report)
    if report.get('cpu_budget') != dict(limit=100, bucket=10000, tick_limit=500):
        raise ValueError('inconsistent CPU budget')
    if set(done) != set(shared['shard']['players']):
        raise ValueError('missing player receipt')
    for player, counts in ([] if shared.get('profile') == mature.PROFILE else done.items()):
        if counts['ticks'] != warmup + ticks or min(counts['spawn'], counts['harvest'], counts['upgrade']) <= 0:
            raise ValueError(f'{player}: skipped tick or incomplete workload')
    for state in ('initial', 'before', 'after'):
        if [r['room'] for r in report[state]] != [r['name'] for r in shared['shard']['rooms']]:
            raise ValueError('missing room')
    for before, after in ([] if shared.get('profile') == mature.PROFILE else zip(report['before'], report['after'])):
        if after['progress'] <= before['progress']:
            raise ValueError('idle room')
    if report['engine'] == 'kkscreeps' and report['metadata']['debug_assertions']:
        raise ValueError('release build required')
    if report['engine'] == 'xxscreeps' and report['metadata']['sandbox'] != 'isolated':
        raise ValueError('production isolated sandbox required')


def first_difference(left, right, path='$'):
    """Return one inspectable difference, without dumping the full mature world."""
    if type(left) is not type(right):
        return f'{path}: {left!r} != {right!r}'
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return f'{path}: unequal keys'
        for key in left:
            found = first_difference(left[key], right[key], f'{path}.{key}')
            if found:
                return found
    elif isinstance(left, list):
        if len(left) != len(right):
            return f'{path}: length {len(left)} != {len(right)}'
        for index, (a, b) in enumerate(zip(left, right)):
            found = first_difference(a, b, f'{path}[{index}]')
            if found:
                return found
    elif left != right:
        return f'{path}: {left!r} != {right!r}'
    return None


def equivalent(left, right):
    for stage in ('initial', 'before', 'after'):
        difference = first_difference(left[stage], right[stage])
        if difference:
            raise ValueError(f'unequal {stage} state: {difference}; no speedup may be reported')
    difference = first_difference(counters(left), counters(right))
    if difference:
        raise ValueError(f'unequal accepted action counts: {difference}; no speedup may be reported')


def workload_comparison(left, right):
    """Same input and verified activity; disclose gameplay divergence separately."""
    difference = first_difference(left['initial'], right['initial'])
    if difference:
        raise ValueError(f'unequal initial state: {difference}; fixture must match')
    state = {stage: dict(equal=left[stage] == right[stage],
                        first_difference=first_difference(left[stage], right[stage]),
                        rooms_with_difference=sum(a != b for a, b in zip(left[stage], right[stage])))
             for stage in ('before', 'after')}
    totals = []
    for report in (left, right):
        actions = {}
        for player in counters(report).values():
            rooms = player.get('rooms', {'legacy': player})
            for room in rooms.values():
                for action, count in room.items():
                    if action != 'ticks':
                        actions[action] = actions.get(action, 0) + count
        totals.append(actions)
    action_deltas = {action: dict(kkscreeps=totals[0].get(action, 0),
                                 xxscreeps=totals[1].get(action, 0),
                                 delta_percent=(totals[0].get(action, 0) / totals[1][action] - 1) * 100
                                 if totals[1].get(action) else None)
                     for action in sorted(totals[0].keys() | totals[1].keys())}
    return dict(state=state, action_counts_equal=totals[0] == totals[1],
                actions=action_deltas,
                equivalent=all(row['equal'] for row in state.values()) and counters(left) == counters(right))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--xxs-root', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/kkscreeps')
    parser.add_argument('--node', default='node')
    parser.add_argument('--profile', choices=['legacy', 'mature'], default='mature')
    parser.add_argument('--players', type=int, nargs='+', default=None)
    parser.add_argument('--strict-equivalence', action='store_true',
                        help='reject any final-state or action-count divergence (compatibility debugging)')
    parser.add_argument('--runs', type=int, default=5)
    parser.add_argument('--warmup', type=int, default=500)
    parser.add_argument('--ticks', type=int, default=3000)
    parser.add_argument('--concurrency', type=int, default=2)
    parser.add_argument('--timeout', type=int, default=43200)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.players is None:
        args.players = [50] if args.profile == 'mature' else [1, 4, 8, 32]
    if args.runs < 1 or not 0 <= args.warmup <= 50000 or not 1 <= args.ticks <= 50000 or not 1 <= args.concurrency <= 32 or any(not 1 <= p <= (50 if args.profile == 'mature' else 32) for p in args.players) or len(set(args.players)) != len(args.players):
        parser.error('invalid run count, tick window, concurrency or player count')
    if args.profile == 'mature' and args.warmup + args.ticks > 3750:
        parser.error('mature resource reserves cover at most 3750 ticks')
    args.binary, args.xxs_root, args.output = args.binary.resolve(), args.xxs_root.resolve(), args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    pkg = args.xxs_root / 'packages/xxscreeps'
    loader = pkg / 'dist/config/mods.js'
    native = list((args.xxs_root / 'node_modules/.pnpm').glob('isolated-vm@*/node_modules/isolated-vm/out/isolated_vm.node'))
    if not loader.is_file() or len(native) != 1:
        parser.error('xxscreeps compiled distribution and one built isolated-vm installation required')
    def payloads():
        return dict(binary=digest(args.binary), node=digest(shutil.which(args.node) or args.node),
                    kk_sources=tree_digest(ROOT / 'crates'), cargo_lock=digest(ROOT / 'Cargo.lock'),
                    xxs=tree_digest(pkg), lodash=tree_digest(args.xxs_root / 'packages/lodash3'),
                    xxs_lock=digest(args.xxs_root / 'pnpm-lock.yaml'), isolated_vm=digest(native[0]),
                    harness=digest(__file__), adapter=digest(HERE / 'xxs-probe.mjs'), player=digest(HERE / 'player.js'),
                    mature=digest(HERE / 'mature.py'), mature_player=digest(HERE / 'mature-player.js'),
                    mature_adapter=digest(HERE / 'xxs-mature.mjs'))
    stale = []
    for source in pkg.rglob('*.ts'):
        relative = source.relative_to(pkg)
        if 'dist' in relative.parts or 'node_modules' in relative.parts or source.name.endswith('.d.ts'):
            continue
        compiled = pkg / 'dist' / relative.with_suffix('.js')
        if compiled.is_file() and source.stat().st_mtime > compiled.stat().st_mtime:
            stale.append(str(relative))
    if stale:
        parser.error('source newer than dist; build xxscreeps first: ' + ', '.join(stale))
    payload_manifest = payloads()
    provenance = dict(payload_manifest=payload_manifest, binary_sha256=digest(args.binary), xxs_tree_sha256=tree_digest(pkg),
                      isolated_vm_sha256=digest(native[0]), node=command([args.node, '--version']),
                      node_sha256=digest(shutil.which(args.node) or args.node),
                      machine_model=command(['sysctl', '-n', 'hw.model']) if platform.system() == 'Darwin' else platform.machine(),
                      memory_bytes=int(command(['sysctl', '-n', 'hw.memsize'])) if platform.system() == 'Darwin' else None,
                      xxs_commit=command(['git', 'rev-parse', 'HEAD'], args.xxs_root),
                      xxs_status=command(['git', 'status', '--porcelain'], args.xxs_root),
                      kk_commit=command(['git', 'rev-parse', 'HEAD'], ROOT),
                      kk_status=command(['git', 'status', '--porcelain'], ROOT),
                      adapter_sha256=digest(HERE / 'xxs-probe.mjs'), harness_sha256=digest(__file__),
                      machine=platform.platform(), processor=platform.processor(), logical_cpus=os.cpu_count(),
                      command=os.sys.argv, started_unix=time.time())
    (args.output / 'provenance.json').write_text(json.dumps(provenance, indent=2))
    results = []
    for players in args.players:
        shared = (mature.fixture(players, args.warmup + args.ticks, warmup=args.warmup)
                  if args.profile == 'mature' else fixture(players, args.warmup + args.ticks))
        fixture_path = args.output / f'fixture-{players}.json'
        fixture_path.write_text(json.dumps(shared))
        expected_states = None
        for repetition in range(args.runs):
            pair = {}
            for engine in (['kkscreeps', 'xxscreeps'] if repetition % 2 == 0 else ['xxscreeps', 'kkscreeps']):
                name = f'{engine}-{players}-{repetition + 1}'
                report = args.output / (name + '.json')
                with tempfile.TemporaryDirectory(prefix='kkscreeps-p2-') as directory:
                    temp = Path(directory)
                    if engine == 'kkscreeps':
                        cmd = [str(args.binary), 'compare-probe', '--fixture', str(fixture_path),
                               '--warmup', str(args.warmup), '--ticks', str(args.ticks),
                               '--concurrency', str(args.concurrency), '--report', str(report)]
                        cwd = ROOT
                    else:
                        shutil.copy(HERE / 'xxs-mature.mjs', temp / 'xxs-mature.mjs')
                        (temp / 'node_modules').symlink_to(args.xxs_root / 'node_modules', target_is_directory=True)
                        shutil.copy(HERE / 'xxs-probe.mjs', temp / 'probe.mjs')
                        config = dict(mods=shared.get('xxs_mods', ['xxscreeps/mods/classic']),
                                      database=dict(data='local://p2-db', pubsub='local://p2-db-pubsub', lock=None, saveInterval=99999),
                                      shards=[dict(name='shard0', data='local://p2-shard', pubsub='local://p2-shard-pubsub', scratch='local://p2-scratch')],
                                      runner=dict(concurrency=args.concurrency, sandbox='isolated', migrationTimeout=0,
                                                  cpu=dict(memoryLimit=64)),
                                      processor=dict(concurrency=args.concurrency))
                        (temp / '.screepsrc.yaml').write_text(json.dumps(config))
                        (temp / 'probe-args.json').write_text(json.dumps(dict(fixture=str(fixture_path), warmup=args.warmup,
                            ticks=args.ticks, concurrency=args.concurrency, report=str(report))))
                        cmd = [args.node, '--import', str(loader), str(temp / 'probe.mjs')]
                        cwd = temp
                    run_process(cmd, cwd, args.output / (name + '.log'), args.timeout)
                pair[engine] = json.loads(report.read_text())
                if pair[engine].get('engine') != engine:
                    raise ValueError('wrong engine report')
                validate(pair[engine], shared, args.warmup, args.ticks, args.concurrency)
                print(f'{name}: validated', flush=True)
            try:
                comparison = workload_comparison(pair['kkscreeps'], pair['xxscreeps'])
                if args.strict_equivalence:
                    equivalent(pair['kkscreeps'], pair['xxscreeps'])
            except ValueError as error:
                (args.output / 'failure.json').write_text(json.dumps(dict(
                    success=False, equivalent=False, players=players, rooms=len(shared['shard']['rooms']),
                    reason=str(error), performance_conclusion_allowed=False), indent=2))
                raise
            states = {engine: {stage: pair[engine][stage] for stage in ('initial', 'before', 'after')}
                      for engine in ('kkscreeps', 'xxscreeps')}
            if expected_states is not None and states != expected_states:
                raise ValueError('non-deterministic world states across independent runs')
            expected_states = states
            results.append(dict(players=players, repetition=repetition + 1, fixture_sha256=digest(fixture_path), reports=pair, comparison=comparison))
            # Stop immediately if either executable payload changed while collecting.
            if payloads() != payload_manifest:
                raise ValueError('executable payload changed during collection')
    summary = []
    for players in args.players:
        row = dict(profile=args.profile, players=players, rooms=players * (6 if args.profile == 'mature' else 1), runs=args.runs,
                   workload_comparisons=[r['comparison'] for r in results if r['players'] == players])
        for engine in ('kkscreeps', 'xxscreeps'):
            reports = [r['reports'][engine] for r in results if r['players'] == players]
            row[engine] = {key: statistics.median(r['phases']['phases']['wall'][key] for r in reports)
                           for key in ('p50_ms', 'p95_ms', 'p99_ms')}
            row[engine].update({key: statistics.median(r['process'][key] for r in reports)
                               for key in ('cpu_core_ms_per_tick', 'rss_bytes_sampled_p50', 'rss_bytes_sampled_peak')})
        row['kkscreeps_over_xxscreeps'] = {key: row['kkscreeps'][key] / row['xxscreeps'][key]
                                           for key in row['kkscreeps']}
        row['observed_goal'] = dict(faster_p50=row['kkscreeps']['p50_ms'] < row['xxscreeps']['p50_ms'],
                                    lower_peak_rss=row['kkscreeps']['rss_bytes_sampled_peak'] < row['xxscreeps']['rss_bytes_sampled_peak'])
        summary.append(row)
    provenance['finished_unix'] = time.time()
    provenance['xxs_status_after'] = command(['git', 'status', '--porcelain'], args.xxs_root)
    (args.output / 'summary.json').write_text(json.dumps(dict(
        success=True, equivalent=all(r['comparison']['equivalent'] for r in results),
        comparison_policy='same initial fixture, complete player/room execution and real activity; gameplay divergence disclosed',
        strict_equivalence=args.strict_equivalence, scope='engine execution/settlement/commit; no disk or HTTP',
        formal_window=args.runs >= 5 and args.warmup >= 500 and args.ticks >= 3000,
        provenance=provenance, summary=summary), indent=2))
    print(json.dumps(summary, indent=2))


if __name__ == '__main__':
    main()
