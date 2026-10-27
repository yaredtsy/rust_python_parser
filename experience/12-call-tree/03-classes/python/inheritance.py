"""`super().__init__()` is a call on the SAME receiver, so the base
constructor's effects land on the child's object -- the child class reads
what the base class's constructor wrote.

ID: c6000000-0000-0000-0000-000000000000
"""


class Logger:
    """ID: c6000000-0001-0000-0000-000000000000"""

    def write(self, msg):
        """ID: c6000000-0002-0000-0000-000000000000"""
        return None


class Base:
    """ID: c6000000-0003-0000-0000-000000000000"""

    def __init__(self):
        """ID: c6000000-0004-0000-0000-000000000000"""
        self.log = Logger()


class Child(Base):
    """ID: c6000000-0005-0000-0000-000000000000"""

    def __init__(self):
        """`super().__init__()` -- same instance, same origin, as `self`
        in this `__init__`.

        ID: c6000000-0006-0000-0000-000000000000
        """
        super().__init__()
        self.x = 1

    def go(self):
        """Reads `self.log`, written by BASE's `__init__`, never Child's
        own.

        ID: c6000000-0007-0000-0000-000000000000
        """
        self.log.write("hi")


def main():
    """ID: c6000000-0008-0000-0000-000000000000"""
    Child().go()
