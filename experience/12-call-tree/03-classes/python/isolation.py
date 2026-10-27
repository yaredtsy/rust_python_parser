"""Fixture 2 from plan/03-call-tree/07: two instances alive at once must
NOT cross-contaminate. plan/03-call-tree/07 calls this "the first test you
write" -- treat it that way here too.

ID: c4000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: c4000000-0001-0000-0000-000000000000"""

    def process(self, data):
        """ID: c4000000-0002-0000-0000-000000000000"""
        return None


class CsvHandler:
    """ID: c4000000-0003-0000-0000-000000000000"""

    def process(self, data):
        """ID: c4000000-0004-0000-0000-000000000000"""
        return None


class Pipeline:
    """ID: c4000000-0005-0000-0000-000000000000"""

    def __init__(self, handler):
        """ID: c4000000-0006-0000-0000-000000000000"""
        self.handler = handler

    def run(self, data):
        """ID: c4000000-0007-0000-0000-000000000000"""
        self.handler.process(data)


def two_instances():
    """`a` and `b` must NOT share state: `a.run` must reach
    `JsonHandler.process`, `b.run` must reach `CsvHandler.process` -- if
    your origin allocation is wrong, one will leak into the other.

    ID: c4000000-0008-0000-0000-000000000000
    """
    a = Pipeline(JsonHandler())
    b = Pipeline(CsvHandler())
    a.run(1)
    b.run(2)
