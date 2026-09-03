"""Flow sensitivity: what ty DOES give you.

Every function here has a well-defined answer that depends on position in
the code, not on who called it. ty handles all of it.

FileID: 19191919-0000-0000-0000-000000000000
"""

import json
from typing import Optional


class Cache:
    """ID: 19191919-1111-1111-1111-111111111111"""

    def get(self, key):
        """ID: 19191919-2222-2222-2222-222222222222"""
        return key


def narrowing(value: Optional[Cache]):
    """After the guard, `value` is narrowed. Ask for its type on both lines.

    ID: 19191919-3333-3333-3333-333333333333
    """
    if value is None:
        return None
    return value.get("k")


def reassigned():
    """Same name, three types, three positions.

    ID: 19191919-4444-4444-4444-444444444444
    """
    thing = 1
    thing = "two"
    thing = Cache()
    return thing.get("k")


def branched(flag):
    """A union at the join point.

    ID: 19191919-5555-5555-5555-555555555555
    """
    if flag:
        thing = Cache()
    else:
        thing = json
    return thing


def unannotated(param):
    """No annotation, no assignment: ty knows nothing.

    ID: 19191919-6666-6666-6666-666666666666
    """
    return param.anything()


def literal_types():
    """ty tracks literal values, not just classes.

    ID: 19191919-7777-7777-7777-777777777777
    """
    n = 42
    s = "hello"
    b = True
    items = [1, 2, 3]
    mapping = {"a": 1}
    return n, s, b, items, mapping


def calls_returning(cache: Cache):
    """The type of a call expression is the callee's return type.

    ID: 19191919-8888-8888-8888-888888888888
    """
    result = cache.get("k")
    blob = json.dumps({"a": 1})
    return result, blob
