"""Resolve against the snapshot, then write: replace and swap.
For 01-summaries/04-one-call.md (plan/03-call-tree/15 fixtures 1a and 1b).

ID: 53000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: 53000000-0001-0000-0000-000000000000"""

    def process(self):
        """ID: 53000000-0002-0000-0000-000000000000"""
        return None


class CsvHandler:
    """ID: 53000000-0003-0000-0000-000000000000"""

    def process(self):
        """ID: 53000000-0004-0000-0000-000000000000"""
        return None


class Box:
    """ID: 53000000-0005-0000-0000-000000000000"""

    def __init__(self, h):
        """ID: 53000000-0006-0000-0000-000000000000"""
        self.h = h

    def replace(self, h):
        """Returns the OLD value, writes the new one.

        ID: 53000000-0007-0000-0000-000000000000
        """
        old = self.h
        self.h = h
        return old


def main():
    """old.process() → JsonHandler.process only; b.h.process() → CsvHandler.process only.

    ID: 53000000-0008-0000-0000-000000000000
    """
    b = Box(JsonHandler())
    old = b.replace(CsvHandler())
    old.process()
    b.h.process()


class Pair:
    """ID: 53000000-0009-0000-0000-000000000000"""

    def __init__(self, x, y):
        """ID: 53000000-000a-0000-0000-000000000000"""
        self.x = x
        self.y = y

    def swap(self):
        """Both effects must read the old state.

        ID: 53000000-000b-0000-0000-000000000000
        """
        t = self.x
        self.x = self.y
        self.y = t


def swap_main():
    """p.x.process() → CsvHandler.process only; p.y.process() → JsonHandler.process only.

    ID: 53000000-000c-0000-0000-000000000000
    """
    p = Pair(JsonHandler(), CsvHandler())
    p.swap()
    p.x.process()
    p.y.process()
