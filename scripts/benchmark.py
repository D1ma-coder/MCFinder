#!/usr/bin/env python3
"""Reproducible fixed-range, one-process-at-a-time backend benchmarks."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--engine', default='target/release/mcfinder')
parser.add_argument('--output', default='benchmarks/results/local.json')
parser.add_argument('--repetitions', type=int, default=3)
parser.add_argument('--cores', type=int, default=10)
parser.add_argument('--gpu', action='store_true')
args = parser.parse_args()
if args.repetitions < 1: parser.error('repetitions must be positive')
root = Path(__file__).resolve().parents[1]
binary = str((root / args.engine).resolve())
geometry = ['--version', '1.18.2', '--near', 'mansion:village:10', '--radius', '4096', '--count', '1000000', '--placement-only']
cases = [
    ('geometry_cpu', geometry + ['--cores', '4']),
    ('biomes_cpu', ['--version','1.18.2','--near','village:desert_pyramid:128','--radius','1024','--count','100000','--cores',str(args.cores)]),
    ('upper16_cpu', ['--version','1.18.2','--near','mansion:village:10','--radius','4096','--count','10000','--structure-seeds','--upper-count','256','--cores',str(args.cores)]),
    ('height_script_150', ['--script','examples/benchmarks/high-mansion-script.rhai','--experimental','--count','100000','--cores',str(args.cores)]),
    ('height_native_150', ['--script','examples/benchmarks/high-mansion-native.rhai','--experimental','--count','100000','--cores',str(args.cores)]),
    ('height_native_170', ['--script','examples/scripts/high-mansion.rhai','--experimental','--count','100000','--cores',str(args.cores)]),
    ('slime_2x2', ['--script','examples/scripts/slime-cluster.rhai','--count','100000','--cores',str(args.cores)]),
]
if args.gpu: cases.insert(1, ('geometry_gpu', geometry + ['--cores','4','--mode','cpu-and-gpu']))
report = {'schema':1, 'timestamp_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'system':platform.system(), 'machine':platform.machine(),
          'engine':subprocess.check_output([binary,'--version'],text=True).strip(),
          'devices':subprocess.check_output([binary,'devices'],text=True).strip(),
          'method':'sequential fixed ranges; engine-reported elapsed time; GPU setup included; no result limit; 1 warmup then repeated runs',
          'cases':[]}
for name, options in cases:
    rates, runs, checksum = [], [], None
    for repetition in range(args.repetitions + 1):
        completed = subprocess.run([binary,'search',*options,'--ui','backend'],cwd=root,capture_output=True,text=True,check=True)
        events = [json.loads(line) for line in completed.stdout.splitlines()]
        summary = next(e['summary'] for e in events if e['event']=='finished')
        if summary['status'] != 'complete': raise RuntimeError(summary)
        hits = sorted((e['hit'] for e in events if e['event']=='match'),key=lambda hit:int(hit['seed_bits']))
        digest = hashlib.sha256(json.dumps(hits,sort_keys=True,separators=(',',':')).encode()).hexdigest()
        if checksum is not None and digest != checksum: raise RuntimeError('Non-deterministic results: '+name)
        checksum = digest
        if repetition:
            state=summary['state']; rates.append(state['average_bases_per_second'])
            runs.append({key:state[key] for key in ('elapsed_seconds','average_bases_per_second','bases_tested','seeds_evaluated','geometry_passed','results_emitted')})
    row={'name':name,'arguments':options,'median_bases_per_second':statistics.median(rates),
         'min_bases_per_second':min(rates),'max_bases_per_second':max(rates),
         'result_sha256':checksum,'runs':runs}
    report['cases'].append(row)
    output=root/args.output; output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(report,indent=2)+'\n')
    print(f'{name}: {statistics.median(rates):,.0f} bases/s',flush=True)
by_name={case['name']:case for case in report['cases']}
assert by_name['height_script_150']['result_sha256']==by_name['height_native_150']['result_sha256']
print('Height implementations produce identical result payloads.')
