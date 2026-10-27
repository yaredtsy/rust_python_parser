"""Summaries by example: Ref, Obj, strong and weak effects.
For 01-summaries/03-the-result.md.

ID: 52000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: 52000000-0001-0000-0000-000000000000"""

    def process(self):
        """ID: 52000000-0002-0000-0000-000000000000"""
        return None


class Box:
    """ID: 52000000-0003-0000-0000-000000000000"""

    def __init__(self, item):
        """ID: 52000000-0004-0000-0000-000000000000"""
        self.item = item


def ident(x):
    """returns [Ref(Param(0))]. ID: 52000000-0005-0000-0000-000000000000"""
    return x


def make():
    """returns [Obj(0)]. ID: 52000000-0006-0000-0000-000000000000"""
    return JsonHandler()


def install(p, h):
    """A strong effect on Param(0). ID: 52000000-0007-0000-0000-000000000000"""
    p.handler = h


def maybe(p, h, c):
    """A weak effect: one arm writes. ID: 52000000-0008-0000-0000-000000000000"""
    if c:
        p.handler = h


def swap_handler(p, h):
    """Returns the OLD handler. ID: 52000000-0009-0000-0000-000000000000"""
    old = p.handler
    p.handler = h
    return old


def wrap(h):
    """An escaping object with state. ID: 52000000-000a-0000-0000-000000000000"""
    return Box(h)
