"""The attribute store: `__init__` writes, a later method reads -- same
instance, different method, the shape plan/03-call-tree/06 is named after.

ID: c3000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: c3000000-0001-0000-0000-000000000000"""

    def process(self, data):
        """ID: c3000000-0002-0000-0000-000000000000"""
        return None


class Pipeline:
    """ID: c3000000-0003-0000-0000-000000000000"""

    def __init__(self, handler):
        """ID: c3000000-0004-0000-0000-000000000000"""
        self.handler = handler
        self.name = "pipe"

    def run(self, data):
        """Reads `self.handler`, written in a DIFFERENT method
        (`__init__`), on the SAME instance.

        ID: c3000000-0005-0000-0000-000000000000
        """
        self.handler.process(data)


def main():
    """ID: c3000000-0006-0000-0000-000000000000"""
    Pipeline(JsonHandler()).run(1)
