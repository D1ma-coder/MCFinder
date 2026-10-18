#!/usr/bin/env python3
"""Collect license notices from Cargo metadata, without shipping local paths."""
import json
from pathlib import Path
import shutil
import sys

metadata = json.load(sys.stdin)
output = Path(sys.argv[1]) / "RustDependencies"
output.mkdir(parents=True, exist_ok=True)
index = []
for package in metadata["packages"]:
    if package["name"] == "mcfinder":
        continue
    source = Path(package["manifest_path"]).parent
    destination = output / (package["name"] + "-" + package["version"])
    copied = []
    for entry in source.iterdir():
        if entry.is_file() and entry.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE")):
            destination.mkdir(parents=True, exist_ok=True)
            shutil.copy2(entry, destination / entry.name)
            copied.append(entry.name)
    index.append({"name": package["name"], "version": package["version"],
                  "license": package.get("license"), "repository": package.get("repository"),
                  "notices": sorted(copied)})
(output / "index.json").write_text(json.dumps(index, indent=2) + "\n")
