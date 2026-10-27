"""Attribute chains of depth > 1: reads, writes at depth, an ambiguous link,
and a chain that dies at an unknown hop.

ID: c8000000-0000-0000-0000-000000000000
"""


class Handler:
    """ID: c8000000-0001-0000-0000-000000000000"""

    def handle(self, x):
        """ID: c8000000-0002-0000-0000-000000000000"""
        return None


class LoudHandler:
    """ID: c8000000-0003-0000-0000-000000000000"""

    def handle(self, x):
        """ID: c8000000-0004-0000-0000-000000000000"""
        return None


class Service:
    """ID: c8000000-0005-0000-0000-000000000000"""

    def __init__(self):
        """ID: c8000000-0006-0000-0000-000000000000"""
        self.handler = Handler()


class Session:
    """Depth-2 write INSIDE `__init__`: `self.service.handler = ...`.

    ID: c8000000-0007-0000-0000-000000000000
    """

    def __init__(self):
        """ID: c8000000-0008-0000-0000-000000000000"""
        self.service = Service()
        self.service.handler = LoudHandler()   # receiver is `self.service`, not `self`

    def go(self):
        """ID: c8000000-0009-0000-0000-000000000000"""
        self.service.handler.handle(1)         # -> LoudHandler.handle, NOT Handler.handle


def write_deep_then_read():
    """A depth-2 write from outside, read back at depth 2.

    ID: c8000000-000a-0000-0000-000000000000
    """
    s = Session()
    s.service.handler = Handler()              # overwrites the LoudHandler
    s.service.handler.handle(2)                # -> Handler.handle


def ambiguous_link(flag):
    """`holder` is one of two `Session` instances (two origins), but both
    resolve to the same callee, so the fan-out dedups to one edge.

    ID: c8000000-000b-0000-0000-000000000000
    """
    if flag:
        holder = Session()
    else:
        holder = Session()
    holder.service.handler.handle(3)           # two origins, one distinct callee: LoudHandler.handle


def unknown_mid_chain(thing):
    """`thing` is a bare parameter: `thing.service` is Unknown, so the whole
    chain is unresolved and the call produces no edge.

    ID: c8000000-000c-0000-0000-000000000000
    """
    thing.service.handler.handle(4)
