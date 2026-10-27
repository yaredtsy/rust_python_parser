"""One context per combination of argument types, and one origin per
combination. For 01-summaries/04-one-call.md (plan/03-call-tree/15 fixture 3).

ID: 54000000-0000-0000-0000-000000000000
"""


class JsonKey:
    """ID: 54000000-0001-0000-0000-000000000000"""

    def encode(self):
        """ID: 54000000-0002-0000-0000-000000000000"""
        return None


class CsvKey:
    """ID: 54000000-0003-0000-0000-000000000000"""

    def encode(self):
        """ID: 54000000-0004-0000-0000-000000000000"""
        return None


class JsonHandler:
    """ID: 54000000-0005-0000-0000-000000000000"""

    def key(self):
        """ID: 54000000-0006-0000-0000-000000000000"""
        return JsonKey()

    def process(self, k):
        """ID: 54000000-0007-0000-0000-000000000000"""
        k.encode()


class CsvHandler:
    """ID: 54000000-0008-0000-0000-000000000000"""

    def key(self):
        """ID: 54000000-0009-0000-0000-000000000000"""
        return CsvKey()

    def process(self, k):
        """ID: 54000000-000a-0000-0000-000000000000"""
        k.encode()


class Job:
    """ID: 54000000-000b-0000-0000-000000000000"""

    def __init__(self, h, k):
        """ID: 54000000-000c-0000-0000-000000000000"""
        self.h = h
        self.k = k

    def go(self):
        """ID: 54000000-000d-0000-0000-000000000000"""
        self.h.process(self.k)


def build(h):
    """Split: one context per handler class.

    ID: 54000000-000e-0000-0000-000000000000
    """
    return Job(h, h.key())


def main(c):
    """JsonHandler.process > JsonKey.encode only; CsvHandler.process > CsvKey.encode only.

    ID: 54000000-000f-0000-0000-000000000000
    """
    h = JsonHandler() if c else CsvHandler()
    build(h).go()
