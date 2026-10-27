"""Attribute writes OUTSIDE `__init__`, and from OUTSIDE the class
entirely -- the generalisation plan/03-call-tree/10 promotes to the
default.

ID: c5000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: c5000000-0001-0000-0000-000000000000"""

    def process(self, data):
        """ID: c5000000-0002-0000-0000-000000000000"""
        return None


class OtherHandler:
    """ID: c5000000-0003-0000-0000-000000000000"""

    def process(self, data):
        """ID: c5000000-0004-0000-0000-000000000000"""
        return None


class Pipeline:
    """A pipeline with no handler set at construction time.

    ID: c5000000-0005-0000-0000-000000000000
    """

    def __init__(self):
        """ID: c5000000-0006-0000-0000-000000000000"""
        pass

    def configure(self):
        """A write OUTSIDE `__init__`, inside another method.

        ID: c5000000-0007-0000-0000-000000000000
        """
        self.handler = OtherHandler()

    def run(self, data):
        """ID: c5000000-0008-0000-0000-000000000000"""
        self.handler.process(data)


def write_outside_init():
    """ID: c5000000-0009-0000-0000-000000000000"""
    p = Pipeline()
    p.configure()
    p.run(1)


def write_from_outside_the_class():
    """A write from code that isn't even a method of `Pipeline`.

    ID: c5000000-000a-0000-0000-000000000000
    """
    p = Pipeline()
    p.handler = JsonHandler()
    p.run(2)
