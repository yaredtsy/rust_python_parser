"""Return-value flow: factories, chained returns, recursion through
returns, and generators. Twelve lines, six distinct behaviours -- this file
is the one plan/03-call-tree/10 calls "not optional."

ID: f5000000-0000-0000-0000-000000000000
"""


class JsonWriter:
    """ID: f5000000-0001-0000-0000-000000000000"""

    def write(self, payload):
        """ID: f5000000-0002-0000-0000-000000000000"""
        return None


class CsvWriter:
    """ID: f5000000-0003-0000-0000-000000000000"""

    def write(self, payload):
        """ID: f5000000-0004-0000-0000-000000000000"""
        return None


def make():
    """ID: f5000000-0005-0000-0000-000000000000"""
    return JsonWriter()


def simple_factory():
    """1. The base case: a value born from a `return`, not a literal
    construction at the use site.

    ID: f5000000-0006-0000-0000-000000000000
    """
    w = make()
    w.write(1)


def build(use_csv):
    """ID: f5000000-0007-0000-0000-000000000000"""
    return CsvWriter() if use_csv else JsonWriter()


def two_returns_union():
    """2. Two `return`s in the callee -> a union of 2, both explored. No
    branch evaluation -- both arms, always.

    ID: f5000000-0008-0000-0000-000000000000
    """
    build(True).write(1)


def outer(flag):
    """ID: f5000000-0009-0000-0000-000000000000"""
    return build(flag)


def chained_returns():
    """3. A return whose OWN value is itself another function's return --
    depth 2.

    ID: f5000000-000a-0000-0000-000000000000
    """
    outer(False).write(1)


def rec(n):
    """4. Recursive through its OWN return value. Walked once; the inner
    call is pending, and the fixpoint finds no value: rec returns nothing.

    ID: f5000000-000b-0000-0000-000000000000
    """
    return rec(n - 1)


def uses_rec():
    """ID: f5000000-000c-0000-0000-000000000000"""
    rec(3)


def gen():
    """5. A generator returns a GENERATOR, not the yielded value -- detect
    `yield` in the body and answer Unknown rather than unioning the yield
    expressions.

    ID: f5000000-000d-0000-0000-000000000000
    """
    yield JsonWriter()


def uses_gen():
    """ID: f5000000-000e-0000-0000-000000000000"""
    for w in gen():
        w.write(1)


async def afetch():
    """6. `await` is transparent -- unwrap it, evaluate the inner call
    directly.

    ID: f5000000-000f-0000-0000-000000000000
    """
    return JsonWriter()


async def uses_await():
    """ID: f5000000-0010-0000-0000-000000000000"""
    w = await afetch()
    w.write(1)
