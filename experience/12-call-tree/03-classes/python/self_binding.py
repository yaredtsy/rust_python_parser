"""Self-binding order, and the unbound-call shape that proves it's an
ORDER rule, not a name-matching one.

ID: c2000000-0000-0000-0000-000000000000
"""


class Payload:
    """ID: c2000000-0003-0000-0000-000000000000"""


class Greeter:
    """ID: c2000000-0001-0000-0000-000000000000"""

    def greet(self, name):
        """ID: c2000000-0002-0000-0000-000000000000"""
        return name


def call_bound():
    """Ordinary method call -- `self` <- the instance, `name` <- the
    SECOND positional (`Payload()`), never the first.

    ID: c2000000-0004-0000-0000-000000000000
    """
    Greeter().greet(Payload())


def call_unbound():
    """`Cls.method(obj, ...)` -- `self` is passed EXPLICITLY as the first
    positional, not bound automatically. Getting the binding order wrong
    shifts every argument after it.

    ID: c2000000-0005-0000-0000-000000000000
    """
    Greeter.greet(Greeter(), Payload())
