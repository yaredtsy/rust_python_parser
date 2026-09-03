"""Calls a mix of first-party, stdlib and unresolvable targets.
This is the file to run your project-code filter against.

FileID: 16161616-7777-7777-7777-777777777777
"""

import json

from pkg import load             # via the package re-export
from pkg.core import transform   # via the defining module
from pkg.sub.deep import descend


def main(path):
    """Six calls. How many should your call tree descend into?

    ID: 16161616-8888-8888-8888-888888888888
    """
    data = load(path)            # first-party, re-exported
    shaped = transform(data)     # first-party, direct
    blob = descend(shaped)       # first-party, subpackage
    text = json.dumps(shaped)    # stdlib -> skip
    size = len(text)             # builtin -> skip by NAME, before inference
    return list(blob), size      # `list` is a builtin too
