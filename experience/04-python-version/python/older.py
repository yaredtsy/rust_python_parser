"""Syntax gated on versions BELOW 3.12, for calibration.

`match` is 3.10+. The walrus is 3.8+. Both parse fine at the 3.10 default,
so a corpus made of files like this one will never reveal a version bug.

ID: 14141414-0000-0000-0000-000000000000
"""

from helpers import build, log


def matcher(command):
    """3.10+ structural pattern matching.

    ID: 14141414-1111-1111-1111-111111111111
    """
    match command:
        case {"action": "build"}:
            return build()
        case [action, *rest]:
            log(action, rest)
            return None
        case _:
            return None


def walrus(items):
    """3.8+ assignment expression.

    ID: 14141414-2222-2222-2222-222222222222
    """
    if (count := len(items)) > 3:
        log(count)
    return count
