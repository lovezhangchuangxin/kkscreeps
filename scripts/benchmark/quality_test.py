"""性能验证器必须拒绝漏跑、空转及不完整寻路窗口。"""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("quality", Path(__file__).with_name("quality.py"))
quality = importlib.util.module_from_spec(spec)
spec.loader.exec_module(quality)

class ValidationTests(unittest.TestCase):
    def player(self):
        rooms = [dict(room=f"W{i}N1", progress=10) for i in range(4)]
        return dict(metadata=dict(debug_assertions=False), players=4, channel="json", warmup=500, ticks=3000,
            workload=dict(actions={f"p{i+1}":dict(ticks=3500,upgrade=100,harvest=100) for i in range(4)},
                initial=copy.deepcopy(rooms), before=rooms, after=[dict(room=r["room"],progress=20) for r in rooms]),
            phases=dict(phases=dict(wall=dict(count=3000,p50_ms=1,p95_ms=2,p99_ms=3))))
    def test_all_players_must_run_and_change_every_room(self):
        quality.validate_player(self.player(),4,"json",500,3000)
        for mutation in [lambda d:d["workload"]["actions"].pop("p4"),
            lambda d:d["workload"]["actions"]["p4"].update(ticks=3499),
            lambda d:d["workload"]["actions"]["p4"].update(harvest=0),
            lambda d:d["workload"]["after"][3].update(progress=10),
            lambda d:d["phases"]["phases"]["wall"].update(count=2999),
            lambda d:d["metadata"].update(debug_assertions=True)]:
            with self.subTest(mutation=mutation):
                data=self.player(); mutation(data)
                with self.assertRaises(ValueError): quality.validate_player(data,4,"json",500,3000)
    def test_pathfinder_requires_all_independent_windows(self):
        data=[dict(round=r,case=c,micros_per_search=1) for r in range(1,6) for c in ("terrain","callback_none","callback_matrix","room_grid")]
        quality.validate_pathfinder(data)
        for bad in [data[:-1],data[:-1]+[data[0]],[dict(row,micros_per_search=float("nan")) for row in data]]:
            with self.assertRaises(ValueError): quality.validate_pathfinder(bad)
    def test_bench_payload_accepts_libtest_line_prefix(self):
        # libtest 的 "test <名称> ... " 与测试体 println! 同行，标记不在行首。
        text = "\nrunning 1 test\ntest player_pathfinder_callback_cost ... PATHFINDER_PLAYER_BENCH=[{\"round\":1}]\nok\n\ntest result: ok. 1 passed\n"
        self.assertEqual(quality.bench_payload(text), "[{\"round\":1}]")
        with self.assertRaises(ValueError): quality.bench_payload("running 1 test\nok\n")

if __name__ == "__main__": unittest.main()
