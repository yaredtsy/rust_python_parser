"""Defaults and keyword overrides -- and the trap: defaults evaluate in the
DEFINING scope, not the caller's.

ID: f2000000-0000-0000-0000-000000000000
"""


class Handler:
    """ID: f2000000-0001-0000-0000-000000000000"""

    def go(self):
        """ID: f2000000-0002-0000-0000-000000000000"""
        return None


DEFAULT = Handler()
"""Module-level default instance. ID: f2000000-0003-0000-0000-000000000000"""


def process(h=DEFAULT):
    """`h`'s default lives in THIS module's scope -- not in whatever module
    calls `process`, even if a caller happens to have its own unrelated
    name `DEFAULT` in scope at the call site.

    ID: f2000000-0004-0000-0000-000000000000
    """
    h.go()


def call_with_default():
    """ID: f2000000-0005-0000-0000-000000000000"""
    process()


def call_with_override():
    """ID: f2000000-0006-0000-0000-000000000000"""
    process(Handler())


def keyword_order(a, b=None):
    """ID: f2000000-0007-0000-0000-000000000000"""
    pass


def call_keywords_out_of_order():
    """Keywords arrive in the OPPOSITE order from the parameter list.
    Binding must match by name, not by position.

    ID: f2000000-0008-0000-0000-000000000000
    """
    keyword_order(b=Handler(), a=Handler())
