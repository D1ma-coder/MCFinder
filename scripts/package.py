#!/usr/bin/env python3
"""Cross-platform portable release bundle with docs, examples and notices."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument('--target', required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--offline', '--format-version', '1'], cwd=root))
package = next(p for p in metadata['packages'] if p['name'] == 'mcfinder')
name = f"mcfinder-v{package['version']}-{args.target}"
bundle = root / 'dist' / name
bundle.mkdir(parents=True, exist_ok=True)
exe = 'mcfinder.exe' if 'windows' in args.target else 'mcfinder'
shutil.copy2(root / 'target' / args.target / 'release' / exe, bundle / exe)
for file in ('README.md', 'README.ru.md', 'LICENSE', 'CHANGELOG.md'):
    shutil.copy2(root / file, bundle / file)
for directory in ('docs', 'examples', 'benchmarks'):
    shutil.copytree(root / directory, bundle / directory, dirs_exist_ok=True, ignore=shutil.ignore_patterns('__pycache__'))
licenses = bundle / 'licenses'
licenses.mkdir(exist_ok=True)
for source, dest in [('vendor/cubiomes/LICENSE', 'Cubiomes-MIT.txt'), ('vendor/seedforge-patches/LICENSE', 'SeedForge-MIT.txt'), ('vendor/README.md', 'SOURCES.md')]:
    shutil.copy2(root / source, licenses / dest)
subprocess.run([sys.executable, str(root / 'scripts/package-licenses.py'), str(licenses)], input=json.dumps(metadata).encode(), check=True)
(bundle / 'BUILD.txt').write_text(f"MCFinder {package['version']}\nTarget: {args.target}\nPortable CPU build (--no-default-features).\nGPU profile detected at runtime. OpenCL driver required only for cpu-and-gpu.\nUnsigned binary; built by GitHub Actions.\n", encoding='utf-8')
if 'windows' in args.target:
    archive = root / 'dist' / (name + '.zip')
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as out:
        for path in sorted(bundle.rglob('*')):
            if path.is_file(): out.write(path, path.relative_to(bundle.parent))
else:
    archive = root / 'dist' / (name + '.tar.gz')
    with tarfile.open(archive, 'w:gz') as out: out.add(bundle, arcname=name)
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_name(archive.name + '.sha256').write_text(f'{digest}  {archive.name}\n')
print(archive)
