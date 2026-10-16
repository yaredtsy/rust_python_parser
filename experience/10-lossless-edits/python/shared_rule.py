"""Which defs exist. Collection and injection must agree on every one.

Several of these defs never *execute* — a `match` runs one case, a `try` body
that succeeds skips its handler. They all still exist, and a statement walk
finds them all. That is the point: "which defs exist" is a syntactic question.

FileID: 10101010-0000-0000-0000-000000000000
"""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    def only_when_type_checking(x):
        return x


try:
    def in_try():
        return 1
except ImportError:
    def in_except():
        return 2
else:
    def in_else():
        return 3
finally:
    def in_finally():
        return 4


with open(__file__) as fh:
    def in_with():
        return fh


for _ in range(1):
    def in_for():
        return 5
else:
    def in_for_else():
        return 6


while False:
    def in_while():
        return 7
else:
    def in_while_else():
        return 8


match len("x"):
    case 1:
        def in_match_case():
            return 9
    case _:
        def in_match_default():
            return 10


def outer():
    if True:
        def nested_in_if():
            return 11

    class NestedClass:
        if True:
            def method_in_if(self):
                return 12

    return nested_in_if, NestedClass
