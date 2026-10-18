#!/usr/bin/env python3
"""Release archives contain exactly one executable; notices are embedded."""
import argparse
import hashlib
from pathlib import Path
import tarfile
import tomllib
import zipfile
parser=argparse.ArgumentParser()
parser.add_argument('--target',required=True)
args=parser.parse_args()
root=Path(__file__).resolve().parents[1]
version=tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
name=f'mcfinder-v{version}-{args.target}'
exe='mcfinder.exe' if 'windows' in args.target else 'mcfinder'
binary=root/'target'/args.target/'release'/exe
output=root/'dist';output.mkdir(exist_ok=True)
if 'windows' in args.target:
 archive=output/(name+'.zip')
 with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED) as bundle: bundle.write(binary,exe)
else:
 archive=output/(name+'.tar.gz')
 with tarfile.open(archive,'w:gz') as bundle: bundle.add(binary,arcname=exe)
archive.with_name(archive.name+'.sha256').write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n')
print(archive)
