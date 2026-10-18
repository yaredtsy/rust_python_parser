"""Direct and mutual recursion -- the ancestor guard's job, not the
return-memo's (00-values/05 vs the in-flight memo in 05-returns-and-value-
flow.md: these fixtures are about the TREE, not about a value query).

ID: f8000000-0000-0000-0000-000000000000
"""


def f():
    """Direct recursion. ID: f8000000-0001-0000-0000-000000000000"""
    f()


def use_direct():
    """ID: f8000000-0002-0000-0000-000000000000"""
    f()


def g():
    """ID: f8000000-0003-0000-0000-000000000000"""
    h()


def h():
    """Mutual recursion: g -> h -> g.

    ID: f8000000-0004-0000-0000-000000000000
    """
    g()


def use_mutual():
    """ID: f8000000-0005-0000-0000-000000000000"""
    g()


class Node:
    """ID: f8000000-0006-0000-0000-000000000000"""

    def __init__(self, children):
        """ID: f8000000-0007-0000-0000-000000000000"""
        self.children = children


def walk(node, visit):
    """Recursion WITH a callback along for the ride -- `visit` is not what
    recurses; `walk` is. The ancestor guard must stop `walk` calling
    itself without also refusing to resolve `visit` at the depth where
    it's first seen. See 06-callbacks-and-higher-order.md.

    ID: f8000000-0008-0000-0000-000000000000
    """
    visit(node)
    for child in node.children:
        walk(child, visit)
