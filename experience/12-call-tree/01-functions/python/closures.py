"""Nested defs and the free variables they close over.

ID: f4000000-0000-0000-0000-000000000000
"""


class Handler:
    """ID: f4000000-0001-0000-0000-000000000000"""

    def go(self):
        """ID: f4000000-0002-0000-0000-000000000000"""
        return None


def outer():
    """`inner` is DEFINED here but not called here -- its calls belong to
    `inner`'s own frame once `inner` IS called, never to `outer`'s frame.

    ID: f4000000-0003-0000-0000-000000000000
    """
    h = Handler()

    def inner():
        """A free variable, `h`, from the ENCLOSING function's locals --
        not a parameter of `inner` itself.

        ID: f4000000-0004-0000-0000-000000000000
        """
        h.go()

    inner()


def with_lambda():
    """parso treats `Lambda` as a `Function` subclass, so the def-collecting
    scanner never descends into one -- but the CALL resolver is a separate
    traversal with its own rule to pin. See 06-callbacks-and-higher-order.md.

    ID: f4000000-0005-0000-0000-000000000000
    """
    run = lambda: Handler().go()  # noqa: E731
    run()
