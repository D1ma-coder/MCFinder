#!/usr/bin/env python3
"""Optional vanilla runtime integration: heartbeat, per-seed progress and controls."""
import argparse
import json
import queue
import subprocess
import threading
import time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('runtime',type=Path)
p.add_argument('--binary',default='target/debug/mcfinder')
a=p.parse_args()
server=subprocess.Popen([a.binary,'serve'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
messages=queue.Queue()
def reader():
    for line in server.stdout:
        messages.put(json.loads(line))
threading.Thread(target=reader,daemon=True).start()
def send(value):
    server.stdin.write(json.dumps(value)+'\n');server.stdin.flush()
def until(event, predicate=lambda e:True):
    deadline=time.monotonic()+45
    while time.monotonic()<deadline:
        message=messages.get(timeout=max(.1,deadline-time.monotonic()))
        assert message.get('event')!='error',message
        if message.get('event')==event and predicate(message):return message
    raise AssertionError('Timed out waiting for '+event)
source='fn configure(){#{version:"26.2",radius:30000000,rules:[]}} fn matches(w,c){let r=w.high_mansion_exact(319,30000000);if r==(){return false;}r}'
settings={'script':{'source':source},'terrain_runtime':str(a.runtime.resolve()),'experimental':True,'cores':2,'count':'18446744073709551615'}
try:
    send({'cmd':'start','id':'world','settings':settings})
    first=until('progress',lambda e:e['state']['phase']=='searching' and int(e['state']['chunk_checks'])>0 and len(e['state']['active_seeds'])==2)
    for seed in first['state']['active_seeds']:
        assert int(seed['total_regions'])==2197359376
        assert int(seed['regions_checked'])>0
    send({'cmd':'pause','id':'pause'})
    paused=until('progress',lambda e:e['state']['paused'])
    stable=until('progress',lambda e:e['state']['paused'])
    assert paused['state']['chunk_checks']==stable['state']['chunk_checks']
    send({'cmd':'resume','id':'resume'})
    until('progress',lambda e:int(e['state']['chunk_checks'])>int(stable['state']['chunk_checks']))
    send({'cmd':'cancel','id':'cancel'})
    done=until('finished')
    assert done['summary']['status']=='cancelled',done
    assert done['summary']['state']['bases_committed']=='0',done
    assert done['summary']['state']['active_seeds']==[],done
    # Cancellation during runtime initialization must also finish as cancelled.
    send({'cmd':'start','id':'init-cancel','settings':settings})
    send({'cmd':'cancel','id':'cancel-init'})
    done=until('finished')
    assert done['summary']['status']=='cancelled',done
    send({'cmd':'shutdown','id':'bye'})
    server.wait(timeout=10)
    assert server.returncode==0
    print('Exact backend: live per-seed progress, pause/resume, cancel and initialization cancel passed')
finally:
    if server.poll() is None:
        server.kill();server.wait()
