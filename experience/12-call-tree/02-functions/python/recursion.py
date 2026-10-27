"""Recursion: each body walked once per context, the recursive call recorded
as a Recursive leaf, and values found by the group's fixpoint.
For 02-functions/08-recursion.md.

ID: f8000000-0000-0000-0000-000000000000
"""


def f():
    """Direct recursion. ID: f8000000-0001-0000-0000-000000000000"""
    f()


def use_direct():
    """use_direct > f > f [Recursive]. ID: f8000000-0002-0000-0000-000000000000"""
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
    """use_mutual > g > h > g [Recursive]. ID: f8000000-0005-0000-0000-000000000000"""
    g()


def on_node_found(node):
    """ID: f8000000-0006-0000-0000-000000000000"""
    return None


def walk(node, visit):
    """Recursion with a callback along for the ride: `visit` doesn't recurse,
    `walk` does. `child` comes out of a container, so it is Unknown.

    ID: f8000000-0008-0000-0000-000000000000
    """
    visit(node)
    for child in node.children:
        walk(child, visit)


def use_walk(root):
    """use_walk > walk > [on_node_found, walk [Recursive]].

    ID: f8000000-0009-0000-0000-000000000000
    """
    walk(root, on_node_found)


class Handle:
    """ID: f8000000-000a-0000-0000-000000000000"""

    def close(self):
        """ID: f8000000-000b-0000-0000-000000000000"""
        return None


def walk_handle(node):
    """The recursive call's value, used in the same body: r.close() resolves
    in the fixpoint. walk_handle > Handle.close, Finished.

    ID: f8000000-000c-0000-0000-000000000000
    """
    r = walk_handle(node.first())
    r.close()
    return Handle()


def use_walk_handle(tree):
    """ID: f8000000-000d-0000-0000-000000000000"""
    walk_handle(tree)


def a(n):
    """ID: f8000000-000e-0000-0000-000000000000"""
    if n:
        return b(n)
    return Handle()


def b(n):
    """ID: f8000000-000f-0000-0000-000000000000"""
    return c(n)


def c(n):
    """ID: f8000000-0010-0000-0000-000000000000"""
    return a(n - 1)


def two_hops():
    """Handle travels a -> c -> b: two hops around the cycle.
    two_hops > b > ... and Handle.close.

    ID: f8000000-0011-0000-0000-000000000000
    """
    b(2).close()


def rec(n):
    """Returns nothing: its only return waits on itself.

    ID: f8000000-0012-0000-0000-000000000000
    """
    return rec(n - 1)


def uses_rec():
    """uses_rec > rec > rec [Recursive]; `.go()` has no target.

    ID: f8000000-0013-0000-0000-000000000000
    """
    rec(3).go()
