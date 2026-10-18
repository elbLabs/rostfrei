#!/usr/bin/env python3
"""Run the pinned upstream Agent Skills validator without pip or system installs.

Only its library API is used (so Click and a build backend are unnecessary).
All imported upstream source and pure-Python wheels are SHA-256 pinned. Downloads
and imports live in a disposable directory; network access is required in CI.
"""

from contextlib import contextmanager
import hashlib
import io
from pathlib import Path
import sys
import tempfile
from urllib.request import urlopen
import zipfile


REVISION = "69ef37e9424c0a7ea9dd2293b559e43ec8176379"
DEPENDENCIES = (
    (f"https://codeload.github.com/agentskills/agentskills/zip/{REVISION}",
     "9f2d86995c04129a74decb87427e0f9dd4353395a78800d479f152cd7c79c9a4"),
    ("https://files.pythonhosted.org/packages/96/7c/a81ef5ef10978dd073a854e0fa93b5d8021d0594b639cc8f6453c3c78a1d/strictyaml-1.7.3-py3-none-any.whl",
     "fb5c8a4edb43bebb765959e420f9b3978d7f1af88c80606c03fb420888f5d1c7"),
    ("https://files.pythonhosted.org/packages/ec/57/56b9bcc3c9c6a792fcbaf139543cee77261f3651ca9da0c93f5c1221264b/python_dateutil-2.9.0.post0-py2.py3-none-any.whl",
     "a8b2bc7bffae282281c8140a97d3aa9c14da0b136dfe83f850eea9a5f7470427"),
    ("https://files.pythonhosted.org/packages/b7/ce/149a00dd41f10bc29e5921b496af8b574d8413afcd5e30dfa0ed46c2cc5e/six-1.17.0-py2.py3-none-any.whl",
     "4721f391ed90541fddacab5acf947aa0d3dc7d27b2e1e8eda2be8970586c3274"),
)


@contextmanager
def reference_validator():
    with tempfile.TemporaryDirectory(prefix="rostfrei-skills-ref-") as temporary:
        root = Path(temporary)
        for url, checksum in DEPENDENCIES:
            with urlopen(url, timeout=60) as response:
                data = response.read()
            if hashlib.sha256(data).hexdigest() != checksum:
                raise ValueError(f"Reference validator dependency checksum mismatch: {url}")
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                archive.extractall(root)
        original = sys.path[:]
        sys.path[:0] = [str(root / f"agentskills-{REVISION}/skills-ref/src"), str(root)]
        try:
            from skills_ref import validate
            yield validate
        finally:
            sys.path[:] = original


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("Usage: python3 scripts/skills_ref_validator.py SKILL_DIRECTORY [...]")
    with reference_validator() as validate:
        failed = False
        for argument in sys.argv[1:]:
            errors = validate(Path(argument))
            print(f"{argument}: {errors or 'valid'}")
            failed |= bool(errors)
        sys.exit(int(failed))
