"""Shapes that exercise the call-tree contract, not just resolution.

Run ty's outgoing_calls on each entry point and compare what you get
against what quirks 1, 5 and 6 say the tree must look like.

FileID: 1d1d1d1d-0000-0000-0000-000000000000
"""


def leaf(x):
    """ID: 1d1d1d1d-1111-1111-1111-111111111111"""
    return x + 1


def twice(x):
    """Two calls to the same callee FROM THE SAME FRAME.
    Quirk 6: these merge into one child with call_count = 1.

    ID: 1d1d1d1d-2222-2222-2222-222222222222
    """
    return leaf(x) + leaf(x + 1)


def recurse(n):
    """Direct recursion. Quirk 5: the ancestor guard stops this,
    NOT a global visited set.

    ID: 1d1d1d1d-3333-3333-3333-333333333333
    """
    if n <= 0:
        return 0
    return recurse(n - 1)


def ping(n):
    """Mutual recursion, half one.

    ID: 1d1d1d1d-4444-4444-4444-444444444444
    """
    return pong(n - 1) if n else 0


def pong(n):
    """Mutual recursion, half two.

    ID: 1d1d1d1d-5555-5555-5555-555555555555
    """
    return ping(n - 1) if n else 0


def no_id_callee(x):
    return x * 2


def calls_undocumented(x):
    """`no_id_callee` has no docstring, so no ID.
    Quirk 4: it is dropped from the tree AND not descended into.

    ID: 1d1d1d1d-6666-6666-6666-666666666666
    """
    return no_id_callee(x) + leaf(x)


def diamond(x):
    """Reaches `leaf` by two different paths. Quirk 1: a TREE, not a
    graph -- so `leaf` appears twice, under two different parents.

    ID: 1d1d1d1d-7777-7777-7777-777777777777
    """
    return left(x) + right(x)


def left(x):
    """ID: 1d1d1d1d-8888-8888-8888-888888888888"""
    return leaf(x)


def right(x):
    """ID: 1d1d1d1d-9999-9999-9999-999999999999"""
    return leaf(x)


class Constructed:
    """Quirk 7: calling a class enters through __init__ and the
    target_id becomes ClassSchema/...

    ID: 1d1d1d1d-aaaa-aaaa-aaaa-aaaaaaaaaaaa
    """

    def __init__(self, value):
        """ID: 1d1d1d1d-bbbb-bbbb-bbbb-bbbbbbbbbbbb"""
        self.value = leaf(value)


def constructs():
    """ID: 1d1d1d1d-cccc-cccc-cccc-cccccccccccc"""
    return Constructed(1)
