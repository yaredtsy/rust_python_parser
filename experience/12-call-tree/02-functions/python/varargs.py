"""*args and **kwargs, at both the parameter side and the call side.

ID: f3000000-0000-0000-0000-000000000000
"""


class Handler:
    """ID: f3000000-0001-0000-0000-000000000000"""

    def go(self):
        """ID: f3000000-0002-0000-0000-000000000000"""
        return None


def takes_splat(a, *rest, **opts):
    """`a` binds normally; `rest`/`opts` bind to Unknown (option (a) from
    05-binding-arguments.md) -- the body is still walked, `a.go()` still
    resolves.

    ID: f3000000-0003-0000-0000-000000000000
    """
    a.go()


def call_normally():
    """ID: f3000000-0004-0000-0000-000000000000"""
    takes_splat(Handler(), 1, 2, extra=True)


def call_with_call_site_splat(items):
    """The arity mapping becomes unknown here -- bind EVERY parameter
    (including `a`) to Unknown rather than guessing an alignment. A wrong
    alignment produces a confidently wrong tree, which is worse than an
    imprecise one.

    ID: f3000000-0005-0000-0000-000000000000
    """
    takes_splat(*items)
