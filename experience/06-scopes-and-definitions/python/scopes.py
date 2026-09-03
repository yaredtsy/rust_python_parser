"""Every scope kind ty knows about, in one file.

ScopeKind has seven variants: Module, TypeParams, Class, Function, Lambda,
Comprehension, TypeAlias. Six of them appear below.

FileID: 17171717-0000-0000-0000-000000000000
"""

COUNTER = 0                       # module scope


type Pair = tuple[int, str]       # TypeAlias scope (3.12+)


def outer(seed):                  # Function scope
    """ID: 17171717-1111-1111-1111-111111111111"""
    total = seed

    def inner(step):              # nested Function scope
        """ID: 17171717-2222-2222-2222-222222222222"""
        nonlocal total
        total += step
        return total

    doubler = lambda x: x * 2     # Lambda scope
    squares = [x * x for x in range(seed)]   # Comprehension scope
    lookup = {k: v for k, v in enumerate(squares)}

    return inner, doubler, lookup


class Service:                    # Class scope
    """ID: 17171717-3333-3333-3333-333333333333"""

    registry = {}                 # class body -- a Class scope, not a Function

    def __init__(self, name):
        """ID: 17171717-4444-4444-4444-444444444444"""
        self.name = name
        self.handler = None

    def run(self, payload):
        """ID: 17171717-5555-5555-5555-555555555555"""
        global COUNTER
        COUNTER += 1
        return self.dispatch(payload)

    def dispatch(self, payload):
        """ID: 17171717-6666-6666-6666-666666666666"""
        return payload

    class Nested:                 # class inside a class
        """ID: 17171717-7777-7777-7777-777777777777"""

        def deep(self):
            """A method of a nested class. What is its qualified name?

            ID: 17171717-8888-8888-8888-888888888888
            """
            return 1


def generic[T](value: T) -> T:    # TypeParams scope (3.12+)
    """ID: 17171717-9999-9999-9999-999999999999"""
    return value


def shadowing():
    """The same NAME, four different definitions.

    ID: 17171717-aaaa-aaaa-aaaa-aaaaaaaaaaaa
    """
    value = 1
    value = "two"
    if COUNTER:
        value = 3.0
    for value in range(2):
        pass
    return value
