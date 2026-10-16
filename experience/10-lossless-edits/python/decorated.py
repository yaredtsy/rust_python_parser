# No docstrings anywhere, and every def is decorated.
#
# Ruff's `StmtFunctionDef.range` starts at the `@`, not at `def` (exercise 02
# object 3, asserted by src/scan.rs::decorators_are_inside_the_range). So for
# `Decorated`, `body[0].start()` is the `@` of `@property` — and the only legal
# place for a new class docstring is ABOVE the decorator. Between `@property`
# and `def` is a syntax error.

import functools


@functools.cache
def decorated_function(x):
    return x


@functools.wraps(decorated_function)
def wrapped(x):
    return x


class Decorated:
    @property
    def first(self):
        return 1

    @staticmethod
    def second():
        return 2
