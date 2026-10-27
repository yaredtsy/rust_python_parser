"""A cut result is never reused. Run with max_stack = 4.
For 01-summaries/05-the-solver.md (plan/03-call-tree/15 fixture 2).

ID: 55000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """No __init__: constructing it walks nothing.

    ID: 55000000-0001-0000-0000-000000000000
    """

    def process(self):
        """ID: 55000000-0002-0000-0000-000000000000"""
        return None


def leaf():
    """ID: 55000000-0003-0000-0000-000000000000"""
    return JsonHandler()


def mid():
    """ID: 55000000-0004-0000-0000-000000000000"""
    return leaf()


def top():
    """ID: 55000000-0005-0000-0000-000000000000"""
    return mid()


def b():
    """ID: 55000000-0006-0000-0000-000000000000"""
    top()


def a():
    """ID: 55000000-0007-0000-0000-000000000000"""
    b()


def main():
    """The first statement cuts top; the second walks it again and finds
    JsonHandler.process.

    ID: 55000000-0008-0000-0000-000000000000
    """
    a()
    top().process()
