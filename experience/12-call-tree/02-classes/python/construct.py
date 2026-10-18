"""Constructing an instance: Class -> Instance, and `__init__` as just
another function call, with a fresh receiver bound to its first parameter.

ID: c1000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: c1000000-0004-0000-0000-000000000000"""

    def process(self, data):
        """ID: c1000000-0005-0000-0000-000000000000"""
        return None


class Pipeline:
    """ID: c1000000-0001-0000-0000-000000000000"""

    def __init__(self, handler):
        """`__init__` IS a function call -- `self` bound to the fresh
        instance, `handler` bound the ordinary way.

        ID: c1000000-0002-0000-0000-000000000000
        """
        self.handler = handler

    def run(self, data):
        """ID: c1000000-0003-0000-0000-000000000000"""
        self.handler.process(data)


def main():
    """ID: c1000000-0006-0000-0000-000000000000"""
    Pipeline(JsonHandler()).run(1)
