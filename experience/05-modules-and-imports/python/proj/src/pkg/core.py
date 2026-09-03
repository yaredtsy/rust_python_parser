"""Every import kind, in one file.

FileID: 16161616-1111-1111-1111-111111111111
"""

import json                      # stdlib -> typeshed stub, NOT project code
import os.path                   # dotted stdlib
from typing import Any           # stdlib, from-import
from collections import OrderedDict as ODict   # aliased

from pkg.sub.deep import descend  # absolute, first-party
from .sub.deep import descend as descend_rel  # relative, same target

try:
    import tomllib               # 3.11+ stdlib
except ImportError:              # pragma: no cover
    tomllib = None

import definitely_not_a_real_module_xyz  # unresolvable, on purpose


def load(path):
    """Calls into stdlib and into first-party code.

    ID: 16161616-2222-2222-2222-222222222222
    """
    raw = json.loads(open(path).read())
    normalised = os.path.normpath(path)
    return descend(raw), normalised


def transform(items: Any) -> ODict:
    """ID: 16161616-3333-3333-3333-333333333333"""
    out = ODict()
    for key in items:
        out[key] = descend_rel(items[key])
    return out
