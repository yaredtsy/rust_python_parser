"""Curry, functools.partial, and decorators -- three ways a function hands
you back ANOTHER function.

ID: f7000000-0000-0000-0000-000000000000
"""

import functools


class Handler:
    """ID: f7000000-0001-0000-0000-000000000000"""

    def go(self, x):
        """ID: f7000000-0002-0000-0000-000000000000"""
        return x


def make_adder(n):
    """Manual curry: a closure returned from a function, capturing `n` as a
    free variable.

    ID: f7000000-0003-0000-0000-000000000000
    """

    def add(x):
        """ID: f7000000-0004-0000-0000-000000000000"""
        return x + n

    return add


def use_curry():
    """`make_adder(1)` returns a FUNCTION value, not a number -- and
    `add_one(5)` calls THAT returned closure, resolved through ordinary
    return-value flow (05-returns-and-value-flow.md) plus a closure that
    carries its captured `n` (04-closures-and-nested-functions.md).

    ID: f7000000-0005-0000-0000-000000000000
    """
    add_one = make_adder(1)
    add_one(5)


def takes_two(a, b):
    """ID: f7000000-0006-0000-0000-000000000000"""
    return a.go(b)


def use_partial():
    """`functools.partial` is stdlib, not project code -- the tree stops
    here, and the bound argument (`Handler()`) is never recovered. This is
    current behaviour, deliberately preserved, not a gap to close.

    ID: f7000000-0007-0000-0000-000000000000
    """
    bound = functools.partial(takes_two, Handler())
    bound(5)


def retry(fn):
    """A decorator -- returns a NEW function (`wrapper`) wrapping `fn`.

    ID: f7000000-0008-0000-0000-000000000000
    """

    def wrapper(*a, **kw):
        """ID: f7000000-0009-0000-0000-000000000000"""
        return fn(*a, **kw)

    return wrapper


@retry
def fetch():
    """Decorated. The call tree resolves `fetch()` to the UNDECORATED
    function -- a deliberate, documented imprecision (07-curry-partial-and-
    decorators.md, plan/03-call-tree/11 § Decorators).

    ID: f7000000-000a-0000-0000-000000000000
    """
    return Handler()


def use_decorated():
    """ID: f7000000-000b-0000-0000-000000000000"""
    fetch()
