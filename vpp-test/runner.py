#!/usr/bin/env python3
"""Minimal VPP python-test runner: one unittest name, no run_tests.py.

Invoked by vpp-test's attached bridge with the test venv interpreter,
cwd = <vpp>/test:

    venv/bin/python3 runner.py <config.py args...> -- <test name>

Everything before `--` is consumed by the framework's config.py (which
parses sys.argv at import time); everything after is loaded as a
unittest name (e.g. "test_acl_plugin.TestACLplugin"). This skips
run_tests.py's discovery/scheduling entirely — process management,
scheduling and parallelism live on the Rust side.
"""

import os
import sys
import unittest

split = sys.argv.index("--")
names = sys.argv[split + 1 :]
sys.argv = sys.argv[:split]

# --cpus is ours, not config.py's: the logical CPUs this run may use
# (first one becomes the test-process affinity / VPP main core). The
# scheduler that normally hands out cores lives in run_tests.py, which
# we replace — Rust decides, we apply.
cpus = None
for i, a in enumerate(sys.argv):
    if a.startswith("--cpus="):
        cpus = [int(c) for c in a.split("=", 1)[1].split(",")]
        del sys.argv[i]
        break
if cpus is None:
    cpus = sorted(os.sched_getaffinity(0))

# make the test framework importable (cwd is <vpp>/test)
sys.path.insert(0, os.getcwd())

from config import config  # noqa: E402  (parses the remaining argv)
from asfframework import VppTestRunner  # noqa: E402


def classes(suite):
    for t in suite:
        if isinstance(t, unittest.TestSuite):
            yield from classes(t)
        else:
            yield type(t)


suite = unittest.TestLoader().loadTestsFromNames(names)
# what run_tests.py's scheduler would have done: give each class its
# core allocation (cores = list of [logical siblings]; we treat each
# logical CPU as its own core)
for cls in set(classes(suite)):
    cls.assign_cores([[c] for c in cpus])

result = VppTestRunner(verbosity=2).run(suite)
sys.exit(0 if result.wasSuccessful() else 1)
