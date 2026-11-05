"""Stage an already-built core and its installed runtime dependency closure.

No build, registry access or writes to the shared checkout. This is a copied
installation for host qualification, not the core owner's tarball/package test.
"""

import json
from pathlib import Path
import shutil


def stage_core(source, destination):
    source, destination = Path(source), Path(destination)
    if not (source / "dist/index.js").is_file():
        raise RuntimeError("Installable core JS is not ready; await the core owner, or use --core-double for synthetic UI tests")
    destination.mkdir(parents=True)
    for name in ("package.json", "README.md"):
        shutil.copyfile(source / name, destination / name)
    shutil.copytree(source / "dist", destination / "dist")
    copied = {source.resolve(): destination}

    def dependencies(package, target):
        for name in json.loads((package / "package.json").read_text()).get("dependencies", {}):
            installed = next((parent / "node_modules" / name for parent in (package, *package.parents)
                              if (parent / "node_modules" / name / "package.json").is_file()), None)
            if installed is None:
                raise RuntimeError(f"Installed core dependency missing: {name}; await core package qualification")
            installed = installed.resolve()
            output = target / "node_modules" / name
            output.parent.mkdir(parents=True, exist_ok=True)
            if installed in copied:
                output.symlink_to(copied[installed], target_is_directory=True)
                continue
            shutil.copytree(installed, output, ignore=shutil.ignore_patterns("node_modules"))
            copied[installed] = output
            dependencies(installed, output)

    dependencies(source.resolve(), destination)
