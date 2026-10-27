"""A recursive descent parser: a cycle with a context that joins during the
fixpoint. For 02-functions/08-recursion.md (plan/03-call-tree/12).

ID: f9000000-0000-0000-0000-000000000000
"""


class Tokens:
    """ID: f9000000-0001-0000-0000-000000000000"""

    def peek(self):
        """ID: f9000000-0002-0000-0000-000000000000"""
        return "("

    def more(self):
        """ID: f9000000-0003-0000-0000-000000000000"""
        return True


class Num:
    """ID: f9000000-0004-0000-0000-000000000000"""

    def emit(self):
        """ID: f9000000-0005-0000-0000-000000000000"""
        return None


class BinOp:
    """ID: f9000000-0006-0000-0000-000000000000"""

    def __init__(self, left, right):
        """ID: f9000000-0007-0000-0000-000000000000"""
        self.left = left
        self.right = right

    def emit(self):
        """ID: f9000000-0008-0000-0000-000000000000"""
        return None


def parse_expr(t):
    """`left` is pending, so `combine(left, t)` is a deferred Call.

    ID: f9000000-0009-0000-0000-000000000000
    """
    left = parse_atom(t)
    return combine(left, t)


def parse_atom(t):
    """ID: f9000000-000a-0000-0000-000000000000"""
    if t.peek() == "(":
        return parse_expr(t)        # cycle: parse_expr -> parse_atom -> parse_expr
    return Num()


def combine(left, t):
    """First walked during the fixpoint; calls back into the cycle, so it joins.

    ID: f9000000-000b-0000-0000-000000000000
    """
    if t.more():
        return BinOp(left, parse_expr(t))
    return left


def parse(t: Tokens):
    """Entry at the head. parse_expr returns Num and BinOp:
    parse > parse_expr > ... and Num.emit, BinOp.emit.

    ID: f9000000-000c-0000-0000-000000000000
    """
    parse_expr(t).emit()


def parse_from_combine(t: Tokens):
    """Entry at the other end. The group's summaries must be the same as
    parse()'s: compare under --no-reuse.

    ID: f9000000-000d-0000-0000-000000000000
    """
    combine(Num(), t).emit()
