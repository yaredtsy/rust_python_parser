"""Edge cases that decide whether your walk matches parser.py.

ID: bbbbbbbb-0000-0000-0000-000000000000
"""

import functools

from helpers import build, log


@functools.cache
def decorated():
    """Where does this node's range START -- at `@` or at `def`?

    ID: bbbbbbbb-1111-1111-1111-111111111111
    """
    build()


@functools.cache
@functools.wraps(build)
async def decorated_async():
    """Two decorators, and async.

    ID: bbbbbbbb-2222-2222-2222-222222222222
    """
    log()


async def plain_async():
    """Does the range start at `async` or at `def`?

    ID: bbbbbbbb-3333-3333-3333-333333333333
    """
    build()


def has_lambda():
    """parser.py drops lambdas AND their whole subtree.
    So `log()` inside the lambda must NOT appear in your output.

    ID: bbbbbbbb-4444-4444-4444-444444444444
    """
    fn = lambda x: log(x)
    build()
    return fn


def one_liner(): build()


class NoDocstring:
    def method(self):
        log()


def comprehensions():
    """Calls inside comprehensions still count.

    ID: bbbbbbbb-5555-5555-5555-555555555555
    """
    return [build() for _ in range(3)]


def default_args(x=build(), *, y=log()):
    """Calls in default arguments are evaluated at def time.
    Are they children of this function, or of its parent?

    ID: bbbbbbbb-6666-6666-6666-666666666666
    """
    return x, y
