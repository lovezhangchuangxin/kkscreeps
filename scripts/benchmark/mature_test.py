"""成熟房夹具的合法配额、有效工作量与拒绝虚假等价回归。"""
import copy
import json
import unittest

import mature


class MatureTests(unittest.TestCase):
    def setUp(self):
        self.shared = mature.fixture(1, 1600)
        states = []
        for room in self.shared['shard']['rooms']:
            structures, creeps = [], []
            for obj in self.shared['objects']:
                if obj['room'] != room['name']:
                    continue
                s = obj['spec']
                item = dict(x=s['pos'][0], y=s['pos'][1], store=s.get('store', {}))
                if obj['kind'] == 'creep':
                    creeps.append(dict(item, name=s['name'], body=s['body']))
                elif obj['kind'] == 'structure' and s['structureType'] != 'source':
                    structures.append(dict(item, type=s['structureType']))
            states.append(dict(room=room['name'], level=8, structures=structures, creeps=creeps))
        self.report = dict(profile=mature.PROFILE, rooms=6,
                           initial=states, before=copy.deepcopy(states), after=copy.deepcopy(states))
        ledger = dict(ticks=1500, harvest=1, upgrade=1, transfer=1, withdraw=1, move=1,
                      reaction=1, factory=1, terminal=1, power=1, spawn=1, pathSearch=1, mineral=1, observe=1)
        self.done = {'100': dict(ticks=1600, rooms={r['name']: dict(ledger) for r in self.shared['shard']['rooms']})}

    def validate(self):
        mature.validate(self.report, self.shared, 100, 1500, self.done)

    def test_complete_report(self):
        self.validate()

    def test_full_target_has_300_rooms_and_independent_role_rosters(self):
        shared = mature.fixture(50, 3500, warmup=500)
        self.assertEqual(len(shared['shard']['rooms']), 300)
        self.assertEqual(len(shared['shard']['players']), 50)
        self.assertEqual(sum(o['kind'] == 'creep' for o in shared['objects']), 4200)
        self.assertEqual(shared['workload']['functional_quotas']['extension'], 60)
        for room in shared['shard']['rooms']:
            objects = [o for o in shared['objects'] if o['room'] == room['name']]
            structures = [o for o in objects if o['kind'] == 'structure']
            for kind, expected in shared['workload']['functional_quotas'].items():
                self.assertEqual(sum(o['spec']['structureType'] == kind for o in structures), expected)
            self.assertEqual(len({o['spec']['name'] for o in objects if o['kind'] == 'creep'}), 14)

    def test_terminal_pairs_are_adjacent_even_across_room_name_digit_boundaries(self):
        shared = mature.fixture(50, 3500)
        by_name = {r['name']: r for r in shared['shard']['rooms']}
        for source, destination in shared['workload']['destinations'].items():
            self.assertEqual(by_name[source]['owner'], by_name[destination]['owner'])
            sx, sy = map(int, source[1:].split('N'))
            dx, dy = map(int, destination[1:].split('N'))
            self.assertEqual(sy, dy)
            self.assertEqual(abs(sx - dx), 1)

    def test_missing_building_even_if_both_engines_agree_is_rejected(self):
        self.report['initial'][0]['structures'] = [o for o in self.report['initial'][0]['structures'] if o['type'] != 'factory']
        with self.assertRaisesRegex(ValueError, 'factory quota'):
            self.validate()

    def test_wrong_initial_store_is_rejected(self):
        self.report['initial'][0]['structures'][0]['store'] = {'energy': 123}
        with self.assertRaisesRegex(ValueError, 'initial store'):
            self.validate()

    def test_idle_measured_production_is_rejected(self):
        self.done['100']['rooms']['W0N1']['reaction'] = 0
        with self.assertRaisesRegex(ValueError, 'idle measured reaction'):
            self.validate()

    def test_lifetime_window_requires_replacement(self):
        self.done['100']['rooms']['W0N1']['spawn'] = 0
        with self.assertRaisesRegex(ValueError, 'replacement spawn'):
            self.validate()

    def test_missing_room_ticks_is_rejected(self):
        self.done['100']['rooms']['W0N1']['ticks'] = 1499
        with self.assertRaisesRegex(ValueError, 'room ticks'):
            self.validate()

    def test_mineral_role_is_not_an_energy_miner(self):
        mineral_workers = [o['spec'] for o in self.shared['objects']
                           if o['kind'] == 'creep' and '_mineral0_' in o['spec']['name']]
        self.assertEqual(len(mineral_workers), 6)
        for worker in mineral_workers:
            self.assertEqual(worker['body'].count('work'), 3)
            self.assertEqual(worker['store']['energy'], 500)
            self.assertNotIn(worker['pos'], [[9, 8], [9, 40]])

    def test_boolean_action_count_is_rejected(self):
        self.done['100']['rooms']['W0N1']['reaction'] = True
        with self.assertRaisesRegex(ValueError, 'idle measured reaction'):
            self.validate()

    def test_reserves_and_room_shape_are_bounded(self):
        for args in [(51, 3500), (50, 3751)]:
            with self.assertRaises(ValueError):
                mature.fixture(*args)
        with self.assertRaises(ValueError):
            mature.fixture(50, 3500, rooms_per_player=3)


if __name__ == '__main__':
    unittest.main()
