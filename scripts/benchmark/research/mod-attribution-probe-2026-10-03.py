#!/usr/bin/env python3
"""交替比较 400-link 热路径：原版、插桩原版与优化版，校验状态及计数。"""
import argparse
import hashlib
import importlib.util
import json
import statistics
import subprocess
import tempfile
from pathlib import Path
root = Path(__file__).resolve().parents[3]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--before', type=Path, required=True)
parser.add_argument('--profile-before', type=Path, required=True)
parser.add_argument('--after', type=Path, default=root/'target/release/kkscreeps')
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
spec = importlib.util.spec_from_file_location('guard', root/'scripts/mod-perf-baseline.py')
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
binaries = {'before': args.before.resolve(),
            'profile_before': args.profile_before.resolve(),
            'after': args.after.resolve()}
hashes = {name: hashlib.sha256(path.read_bytes()).hexdigest() for name,path in binaries.items()}
runs=[]
reference=None
identities={}
profile_identities={}
out=args.output

def require(condition, message):
    if not condition:
        raise RuntimeError(message)

with tempfile.TemporaryDirectory(prefix='kk-mod-attribution-') as tmp:
    for rep in range(3):
        tasks=[('before',False,'chain'),('after',False,'chain'),
               ('profile_before',True,'chain'),('after',True,'chain')]
        if rep%2: tasks.reverse()
        tasks += [(version,False,mode) for mode in ('native','empty') for version in ('before','after')]
        for version,profile,mode in tasks:
            binary=binaries[version]
            require(hashlib.sha256(binary.read_bytes()).hexdigest()==hashes[version], '二进制发生变化')
            report_path=Path(tmp)/'report.json'
            cmd=[str(binary),'mod-perf-probe','--mode',mode,'--warmup','100','--ticks','500',
                 '--fixtures',str(root/'compat/fixtures/mod-perf'),'--report',str(report_path)]
            if profile: cmd.append('--profile')
            completed=subprocess.run(cmd,cwd=root,capture_output=True,text=True,timeout=120)
            if completed.returncode: raise RuntimeError(completed.stdout+completed.stderr)
            require(hashlib.sha256(binary.read_bytes()).hexdigest()==hashes[version], '二进制发生变化')
            report=json.loads(report_path.read_text())
            guard.validate_report(report,mode,1,400,100,500,profile)
            # 允许两端诊断版本不同，同一端的重复采集须保持阶段口径。
            guard.check_profile_identity(profile_identities,(version,profile,mode),report)
            states={k:v['link_state_sha256'] for k,v in report['checks'].items()}
            if reference is None: reference=states
            require(reference==states, '对象状态不等价')
            # 不同模式的包可以不同；同一模式跨版本和重复轮次必须保持身份。
            current=(report['package_digests'],report['vm_limits'])
            require(identities.setdefault(mode,current)==current, f'{mode} 包或预算配置发生变化')
            report.update(version=version,instrumented=profile,repetition=rep+1)
            runs.append(report)
            print(f'completed rep={rep+1} version={version} profile={profile} mode={mode}',flush=True)
            out.with_suffix('.partial.json').write_text(json.dumps({'complete':False,'binaries':hashes,'runs':runs},ensure_ascii=False,indent=2)+'\n')
summary={}
for version,profile,mode in dict.fromkeys((r['version'],r['instrumented'],r['mode']) for r in runs):
    selected=[r for r in runs if (r['version'],r['instrumented'],r['mode'])==(version,profile,mode)]
    key=f'{version}:{"profile" if profile else "plain"}:{mode}'
    item={f'median_wall_{q}_ms':statistics.median(r['wall'][f'{q}_ms'] for r in selected) for q in ('p50','p95','p99')}
    item['run_wall_p99_ms']=[r['wall']['p99_ms'] for r in selected]
    item['median_cpu_core_ms_per_tick']=statistics.median(r['process']['cpu_core_ms_per_tick'] for r in selected)
    if profile:
        item['profile_ms_per_tick']={phase:statistics.median(r['profile'][phase]['wall_ns']/r['ticks']/1e6 for r in selected) for phase in selected[0]['profile']}
    summary[key]=item
result={'complete':True,'binaries':hashes,'warmup':100,'ticks':500,'repetitions':3,
        'scope':'400-link pure settlement; paired local exploratory comparison',
        'limitations':['diagnostic phases are inclusive; profiling adds overhead',
                       'three local repeats, uncontrolled desktop load; no hard performance gate',
                       'no player execution, projection, persistence or network'],
        'summary':summary,'runs':runs}
out.write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
out.with_suffix('.partial.json').unlink()
print(json.dumps(summary,indent=2),flush=True)
