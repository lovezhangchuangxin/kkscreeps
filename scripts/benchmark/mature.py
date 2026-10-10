"""成熟房共同夹具及验收；两端只共享公开字段，不比较对象 ID。"""
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
CONSTANTS = json.loads((HERE.parents[1] / 'crates/kk-runner/assets/constants.json').read_text())
PROFILE = 'mature-rcl8-v1'
ROLES = ['miner0', 'miner1', 'haul0', 'haul1', 'fill0', 'fill1', 'fill2',
         'upgrade0', 'repair0', 'mineral0', 'supply0', 'factory0', 'power0', 'link0']


def fixture(players, total_ticks, rooms_per_player=6, warmup=0):
    if not 1 <= players <= 50 or rooms_per_player != 6 or not 1 <= total_ticks <= 3750:
        raise ValueError('mature fixture requires 1..50 players, six rooms and at most 3750 ticks')
    rooms, objects, ids = [], [], []
    terrain = [int(x in (0, 49) or y in (0, 49) or (x, y) in ((1, 1), (8, 8), (8, 40), (40, 40)))
               for y in range(50) for x in range(50)]
    quotas = {kind: levels.get('8', 0) for kind, levels in CONSTANTS['CONTROLLER_STRUCTURES'].items()
              if kind not in ('road', 'constructedWall', 'rampart')}
    for i in range(players):
        owner = str(100 + i)
        ids.append(owner)
        for j in range(6):
            n = i * 6 + j
            room = f'W{n % 20}N{n // 20 + 1}'
            rooms.append(dict(name=room, owner=owner, rcl=8, terrain=terrain))
            occupied = {(1, 1), (8, 8), (8, 40), (40, 40)}
            def add(kind, pos, **fields):
                spec = dict(pos=list(pos), **fields)
                objects.append(dict(room=room, kind=kind, spec=spec))
                if kind != 'creep':
                    occupied.add(pos)
            def structure(kind, pos, **fields):
                if kind not in ('road', 'container', 'constructedWall'):
                    fields['owner'] = owner
                add('structure', pos, structureType=kind, **fields)
            for pos in ((8, 8), (8, 40)):
                structure('source', pos, energy=3000, energyCapacity=3000)
            add('mineral', (40, 40), mineralType='H', mineralAmount=50000, density=2)
            structure('extractor', (40, 40))
            structure('storage', (24, 25), store=dict(energy=700000, H=50000, O=50000))
            structure('terminal', (26, 25), store=dict(energy=100000, H=10000))
            structure('factory', (28, 25), store=dict(energy=45000))
            structure('powerSpawn', (30, 25), store=dict(energy=5000, power=100))
            structure('observer', (32, 25))
            structure('nuker', (34, 25), store=dict(energy=300000, G=5000))
            for k, pos in enumerate(((22, 23), (26, 23), (30, 23))):
                structure('spawn', pos, name=f'Spawn_{room}_{k}', store=dict(energy=300))
            for y in range(16, 22):
                for x in range(16, 36, 2):
                    structure('extension', (x, y), store=dict(energy=200))
            for pos in ((20, 26), (22, 26), (24, 26), (26, 26), (28, 26), (30, 26)):
                structure('link', pos, store=dict(energy=400))
            for pos in ((14, 14), (36, 14), (14, 36), (36, 36), (14, 25), (36, 25)):
                structure('tower', pos, store=dict(energy=1000))
            for k, pos in enumerate(((25, 30), (26, 30), (24, 29), (25, 29), (26, 29), (27, 29), (24, 30), (27, 30), (25, 31), (26, 31))):
                store = dict(energy=2000)
                if k == 0: store['H'] = 3000
                if k == 1: store['O'] = 3000
                structure('lab', pos, store=store)
            for pos in ((9, 8), (9, 40), (3, 3), (39, 40), (25, 27)):
                structure('container', pos, store=dict(energy=1000))
            # 环形防线留四个出入口；道路避开功能建筑，保持真实可通行走廊。
            ring = sorted({(x, y) for x in range(12, 39) for y in range(12, 39)
                           if x in (12, 38) or y in (12, 38)})
            for pos in ring:
                if pos not in ((25, 12), (25, 38), (12, 25), (38, 25)):
                    structure('rampart', pos, hits=1000000)
            for x in range(13, 38):
                if x not in (24, 25, 26):
                    structure('constructedWall', (x, 37), hits=1000000)
            road_positions = {(x, y) for x in range(3, 40) for y in (9, 24, 29, 39)}
            road_positions |= {(x, y) for y in range(3, 41) for x in (9, 15, 25, 35, 39)}
            for pos in sorted(road_positions - occupied):
                structure('road', pos)
            for role in ROLES:
                if role in ('miner0', 'miner1'):
                    pos = (9, 8 if role == 'miner0' else 40)
                    body = ['work'] * 5 + ['carry'] + ['move'] * 6
                elif role.startswith('haul') or role.startswith('fill'):
                    pos = (10 + ROLES.index(role), 24)
                    body = ['carry'] * 10 + ['move'] * 10
                else:
                    pos = (18 + ROLES.index(role), 28)
                    body = ['work'] * 3 + ['carry'] * 10 + ['move'] * 13
                add('creep', pos, name=f'{room}_{role}_initial', owner=owner,
                    body=body, store=dict(energy=50 if role in ('miner0', 'miner1') else 500))
    setup = dict(profile=PROFILE, rooms_per_player=6, functional_quotas=quotas, roles=ROLES,
                 total_ticks=total_ticks, warmup=warmup,
                 destinations={r['name']: rooms[n ^ 1]['name'] for n, r in enumerate(rooms)}, workload='mature economy, movement, replacement, maintenance, production and terminal transport')
    code = 'const BENCH = ' + json.dumps(setup) + ';\n' + (HERE / 'mature-player.js').read_text()
    return dict(profile=PROFILE, workload=setup, shard=dict(players=ids, rooms=rooms),
                objects=objects, code=code, cpu_limit=100, kkpv=True,
                xxs_mods=['xxscreeps/mods/classic', 'xxscreeps/mods/modern/factory',
                          'xxscreeps/mods/modern/observer', 'xxscreeps/mods/modern/powerspawn',
                          'xxscreeps/mods/modern/nuker'])


def validate(report, shared, warmup, ticks, done):
    if report.get('profile') != PROFILE or report.get('rooms') != len(shared['shard']['rooms']):
        raise ValueError('missing mature profile or room count')
    expected = {r['name']: r for r in shared['shard']['rooms']}
    for stage in ('initial', 'before', 'after'):
        states = report[stage]
        if [r['room'] for r in states] != list(expected):
            raise ValueError('missing mature room')
        for state in states:
            if state['level'] != 8:
                raise ValueError('mature controller downgraded')
            counts = {}
            for obj in state['structures']:
                counts[obj['type']] = counts.get(obj['type'], 0) + 1
            for kind, count in shared['workload']['functional_quotas'].items():
                if counts.get(kind, 0) != count:
                    raise ValueError(f'{stage} {state["room"]}: missing {kind} quota')
    # 校验初始库存/坐标/身体，防止双方共同漏放同一种建筑后仍被当作等价。
    for state in report['initial']:
        source_objects = [o for o in shared['objects'] if o['room'] == state['room']]
        for key, selected in [('structures', [o for o in source_objects if o['kind'] == 'structure' and o['spec']['structureType'] != 'source']),
                              ('creeps', [o for o in source_objects if o['kind'] == 'creep'])]:
            actual = {(o['x'], o['y'], o.get('type', o.get('name'))): o for o in state[key]}
            for obj in selected:
                spec = obj['spec']
                identity = (*spec['pos'], spec.get('structureType', spec.get('name')))
                if identity not in actual:
                    raise ValueError('mature initial object missing')
                item = actual[identity]
                if item['store'] != {k: v for k, v in spec.get('store', {}).items() if v}:
                    raise ValueError('mature initial store mismatch')
                if key == 'creeps' and item['body'] != spec['body']:
                    raise ValueError('mature initial body mismatch')
    for player, receipt in done.items():
        names = [r['name'] for r in shared['shard']['rooms'] if r['owner'] == player]
        if receipt.get('ticks') != warmup + ticks or set(receipt.get('rooms', {})) != set(names):
            raise ValueError('mature missing player or room tick receipt')
        for name in names:
            ledger = receipt['rooms'][name]
            if ledger.get('ticks') != ticks:
                raise ValueError('mature measured room ticks incomplete')
            for action in ('harvest', 'upgrade', 'transfer', 'withdraw', 'move', 'pathSearch', 'mineral', 'observe', 'reaction', 'factory', 'terminal', 'power'):
                if type(ledger.get(action)) is not int or ledger[action] <= 0:
                    raise ValueError(f'{name}: idle measured {action}')
            if ticks >= 1500 and (type(ledger.get('spawn')) is not int or ledger['spawn'] <= 0):
                raise ValueError(f'{name}: missing replacement spawn')
