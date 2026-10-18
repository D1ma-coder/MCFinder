#!/usr/bin/env python3
"""Prepare an optional vanilla 26.2 height runtime; game jars remain local."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import urllib.request
import zipfile


def fetch_json(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return json.load(response)


def prepare():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--directory', type=Path, default=Path.home() / '.cache/mcfinder/vanilla-26.2')
    p.add_argument('--java-home', type=Path)
    p.add_argument('--server-jar', type=Path)
    p.add_argument('--manifest', type=Path, help='Local official version metadata for offline preparation')
    args = p.parse_args()
    root = args.directory.expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    java_home = args.java_home or (Path(os.environ['JAVA_HOME']) if 'JAVA_HOME' in os.environ else None)
    javac = java_home / 'bin' / ('javac.exe' if os.name == 'nt' else 'javac') if java_home else Path(shutil.which('javac') or '')
    if not javac.is_file():
        p.error('A JDK 25+ is required; specify --java-home')
    java = javac.with_name('java.exe' if os.name == 'nt' else 'java').resolve()
    if args.manifest:
        metadata = json.loads(args.manifest.read_text())
    else:
        versions = fetch_json('https://piston-meta.mojang.com/mc/game/version_manifest_v2.json')
        entry = next(v for v in versions['versions'] if v['id'] == '26.2')
        metadata = fetch_json(entry['url'])
    if metadata['id'] != '26.2':
        p.error('This adapter supports vanilla Java 26.2 only')
    download = metadata['downloads']['server']
    jar = root / 'server-26.2.jar'
    if args.server_jar:
        source = args.server_jar.resolve()
        if source != jar:
            shutil.copyfile(source, jar)
    elif not jar.exists():
        with urllib.request.urlopen(download['url'], timeout=120) as response, jar.open('wb') as out:
            shutil.copyfileobj(response, out)
    if hashlib.sha1(jar.read_bytes()).hexdigest() != download['sha1']:
        p.error('Server SHA-1 does not match official version metadata; remove the file and retry')
    classpath = []
    with zipfile.ZipFile(jar) as archive:
        for index, prefix in [('META-INF/versions.list', 'META-INF/versions/'), ('META-INF/libraries.list', 'META-INF/libraries/')]:
            for line in archive.read(index).decode().splitlines():
                digest, _, name = line.split('\t')
                target = (root / prefix / name).resolve()
                if not target.is_relative_to(root):
                    p.error('Unsafe bundled path')
                data = archive.read(prefix + name)
                if hashlib.sha256(data).hexdigest() != digest:
                    p.error('Bundled jar checksum mismatch: ' + name)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
                classpath.append(str(target))
    classes = root / 'classes'
    if classes.exists():
        shutil.rmtree(classes)
    classes.mkdir()
    source = Path(__file__).resolve().parent.parent / 'java/MCFinderTerrain.java'
    helper = source.parent / 'mcfinder/terrain/MCFinderClimateCache.java'
    subprocess.run([str(javac), '--release', '25', '-cp', os.pathsep.join(classpath), '-d', str(classes), str(source), str(helper)], check=True)
    files = [Path(n) for n in classpath] + sorted(classes.rglob('*.class'))
    config = {'schema': 1, 'version': '26.2', 'java': str(java),
              'classpath': [str(classes)] + classpath,
              'source_sha256': hashlib.sha256(source.read_bytes() + helper.read_bytes()).hexdigest(),
              'files': {str(f): hashlib.sha256(f.read_bytes()).hexdigest() for f in files}}
    descriptor = root / 'runtime.json'
    descriptor.write_text(json.dumps(config, indent=2) + '\n')
    print(descriptor)


if __name__ == '__main__':
    prepare()
