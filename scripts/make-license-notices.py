#!/usr/bin/env python3
"""Collect notices embedded in the executable by the `licenses` command."""
import json
from pathlib import Path
import subprocess
root=Path(__file__).resolve().parents[1]
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--offline','--format-version','1'],cwd=root))
parts=['MCFinder and third-party license notices\n']
for title,file in [('MCFinder','LICENSE'),('Cubiomes','vendor/cubiomes/LICENSE'),('SeedForge version patches','vendor/seedforge-patches/LICENSE')]:
 parts.append('\n'+'='*72+'\n'+title+'\n\n'+(root/file).read_text())
for package in sorted(metadata['packages'],key=lambda p:(p['name'],p['version'])):
 if package['name']=='mcfinder': continue
 parts.append('\n'+'='*72+'\n'+package['name']+' '+package['version']+'\nLicense: '+str(package.get('license'))+'\nSource: '+str(package.get('repository'))+'\n')
 for file in sorted(Path(package['manifest_path']).parent.iterdir()):
  if file.is_file() and file.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE')):
   parts.append('\n'+file.name+'\n\n'+file.read_text(errors='replace'))
path=root/'licenses/THIRD_PARTY.txt';path.parent.mkdir(exist_ok=True);path.write_text('\n'.join(parts))
print(path.relative_to(root),path.stat().st_size)
