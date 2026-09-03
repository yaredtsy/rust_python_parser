"""PEP 695 syntax -- Python 3.12+.

On a 3.10 target every construct below is an UnsupportedSyntaxError.

ID: 12121212-0000-0000-0000-000000000000
"""

from helpers import build

type Alias = int | str


def generic_fn[T](value: T) -> T:
    """A generic function, 3.12 syntax.

    ID: 12121212-1111-1111-1111-111111111111
    """
    build()
    return value


class Box[T]:
    """A generic class, 3.12 syntax.

    ID: 12121212-2222-2222-2222-222222222222
    """

    def __init__(self, item: T):
        """ID: 12121212-3333-3333-3333-333333333333"""
        self.item = item

    def unwrap(self) -> T:
        """ID: 12121212-4444-4444-4444-444444444444"""
        build()
        return self.item
