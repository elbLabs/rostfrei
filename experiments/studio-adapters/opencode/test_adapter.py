#!/usr/bin/env python3
"""Run capture-isolation tests with Node 24 in an owned package installation."""

import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile
import sys

sys.dont_write_bytecode = True
from package_install import stage_core

here = Path(__file__).resolve().parent
repo = here.parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--node", default="node")
args = parser.parse_args()
temporary = repo / "artifacts/tmp"
temporary.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix="opencode-adapter-test-", dir=temporary) as root:
    package = Path(root) / "plugin"
    shutil.copytree(here, package, ignore=shutil.ignore_patterns("node_modules", "__pycache__"))
    stage_core(repo / "packages/tracer-presentation", package / "node_modules/@rostfrei/tracer-presentation")
    shutil.copytree(repo / "packages/tracer-presentation/test/canonical", package / "test-evidence")
    result = subprocess.run([args.node, "--test", "adapter.test.js"], cwd=package)
raise SystemExit(result.returncode)
