#!/usr/bin/env python3
"""Audit exact height against vanilla's original structure method and benchmark both paths."""
import argparse
import json
import os
import platform
from pathlib import Path
import queue
import random
import statistics
import subprocess
import threading
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('runtime', type=Path)
p.add_argument('--samples', type=int, default=512)
p.add_argument('--threads', type=int, default=4)
p.add_argument('--output', type=Path)
p.add_argument('--hardware', default='unspecified')
a = p.parse_args()
c = json.loads(a.runtime.read_text())
log = a.runtime.with_suffix('.audit.log').open('w')
worker = subprocess.Popen([c['java'], '-Xms64m', '-Xmx768m', '-cp', os.pathsep.join(c['classpath']), 'MCFinderTerrain', str(a.threads)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True, bufsize=1)
replies = queue.Queue()
def read():
    for line in worker.stdout:
        replies.put(json.loads(line))
    replies.put({'error': 'worker exited'})
threading.Thread(target=read, daemon=True).start()
next_id = 1

def batch(points, **extra):
    global next_id
    results = {}
    pending = iter(points)
    active = 0
    def send(q):
        global next_id
        i = next_id
        next_id += 1
        worker.stdin.write(json.dumps({**q, **extra, 'id': i}) + '\n')
        worker.stdin.flush()
        return i
    ids = []
    for _ in range(a.threads * 2):
        q = next(pending, None)
        if q is None:
            break
        ids.append(send(q)); active += 1
    started = time.perf_counter()
    while active:
        r = replies.get(timeout=120)
        assert 'error' not in r, r
        results[r['id']] = r
        active -= 1
        q = next(pending, None)
        if q is not None:
            ids.append(send(q)); active += 1
    return [results[i] for i in ids], time.perf_counter() - started

try:
    ready = replies.get(timeout=60)
    assert ready.get('ready') and ready['version'] == '26.2', ready
    rng = random.Random(8642)
    points = [{'seed': str([0, 42, 4743858, 2**64 - 1][i % 4]), 'x': rng.randrange(-2560, 2561) * 16, 'z': rng.randrange(-2560, 2561) * 16} for i in range(a.samples)]
    points += [{'seed': '4743858', 'x': 1792, 'z': -2416}]
    points += [{'seed': str(s), 'x': x, 'z': z} for s in [0, 2**64 - 1] for x, z in [(-29999984, 29999984), (29999984, -29999984), (-16, -16), (0, 0)]]
    audit_cases = accepted = biome_rejected = columns = 0
    for threshold in [60, 150, 170, 190]:
        results, _ = batch(points, min_y=threshold, audit=True)
        for q, r in zip(points, results):
            expected = r['reference_viable'] and r['reference_y'] >= threshold
            assert r['viable'] == expected, (threshold, q, r)
            if expected:
                assert r['y'] == r['reference_y'], (q, r)
                accepted += 1
            biome_rejected += int(r['biome_rejected'])
            columns += r['columns']
            audit_cases += 1
    known, _ = batch([{'seed': '4743858', 'x': 1792, 'z': -2416}], audit=True)
    assert known[0]['y'] == known[0]['reference_y'] == 158, known
    groups = [{'seed': '0', 'points': points[i:i+32]} for i in range(0, len(points), 32)]
    batch_audits = 0
    for threshold in [60, 150, 190]:
        grouped, _ = batch(groups, min_y=threshold)
        for group, got in zip(groups, grouped):
            offset = batch_audits % len(groups) * 32
            expected = next((i for i, r in enumerate(results[offset:offset+len(group['points'])]) if r['reference_viable'] and r['reference_y'] >= threshold), None)
            assert got['viable'] == (expected is not None), (threshold, group, got)
            if expected is not None:
                assert got['index'] == expected
                assert got['y'] == results[offset+expected]['reference_y']
            batch_audits += 1
    # Warm both paths before timing; the same seeds/positions and RPC pipeline are used.
    batch(points, baseline=True)
    batch(points, min_y=190)
    benchmarks = []
    for threshold in [150, 190]:
        baseline_times, optimized_times = [], []
        for repeat in range(3):
            order = [False, True] if repeat % 2 else [True, False]
            runs = {}
            for optimized in order:
                values, elapsed = batch(points, **({'min_y': threshold} if optimized else {'baseline': True}))
                (optimized_times if optimized else baseline_times).append(elapsed)
                runs[optimized] = values
            for raw, optimized in zip(runs[False], runs[True]):
                expected = raw['viable'] and raw.get('y', -65) >= threshold
                assert optimized['viable'] == expected
                if expected:
                    assert optimized['y'] == raw['y']
        baseline = statistics.median(baseline_times)
        optimized = statistics.median(optimized_times)
        batch_times, batch_counts = [], []
        for _ in range(3):
            grouped, elapsed = batch(groups, min_y=threshold)
            batch_times.append(elapsed)
            batch_counts.append(sum(r['tested'] for r in grouped))
        benchmarks.append({'min_y': threshold, 'baseline_seconds_median': baseline, 'optimized_seconds_median': optimized, 'baseline_candidates_per_second': len(points) / baseline, 'optimized_candidates_per_second': len(points) / optimized, 'speedup': baseline / optimized, 'batch_size': 32, 'batch32_seconds_median': statistics.median(batch_times), 'batch32_candidates_tested': batch_counts[0], 'batch32_candidates_per_second': batch_counts[0] / statistics.median(batch_times)})
    report = {'hardware': a.hardware, 'architecture': platform.machine(), 'os': platform.system(), 'java': subprocess.run([c['java'], '-version'], capture_output=True, text=True).stderr.splitlines()[0], 'timing': 'JVM startup excluded; JSONL transport included; median of three passes', 'batch_audit_requests': batch_audits, 'version': '26.2', 'threads': a.threads, 'samples_per_pass': len(points), 'audit_cases': audit_cases, 'accepted_audit_cases': accepted, 'biome_rejections': biome_rejected, 'terrain_columns_evaluated': columns, 'vanilla_baseline_columns_upper_bound': audit_cases * 4, 'benchmarks': benchmarks}
    text = json.dumps(report, indent=2)
    if a.output:
        a.output.parent.mkdir(parents=True, exist_ok=True)
        a.output.write_text(text + '\n')
    print(text)
finally:
    worker.stdin.close()
    try:
        worker.wait(timeout=5)
    except subprocess.TimeoutExpired:
        worker.kill(); worker.wait()
    log.close()
