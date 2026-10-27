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
    traversal with its own rule to pin. See 04-closures-and-nested-functions.md.

    ID: f4000000-0005-0000-0000-000000000000
    """
    run = lambda: Handler().go()  # noqa: E731
    run()


class JsonWriter:
    """ID: f4000000-0006-0000-0000-000000000000"""

    def write(self, payload):
        """ID: f4000000-0007-0000-0000-000000000000"""
        return None


class CsvWriter:
    """ID: f4000000-0008-0000-0000-000000000000"""

    def write(self, payload):
        """ID: f4000000-0009-0000-0000-000000000000"""
        return None


def make_handler(writer):
    """Returns a CLOSURE over `writer` -- a free variable of `handle`,
    captured at the point this `def` statement runs, not at the point
    `handle` is later called.

    ID: f4000000-000a-0000-0000-000000000000
    """

    def handle(data):
        """ID: f4000000-000b-0000-0000-000000000000"""
        writer.write(data)

    return handle


def read_json():
    """`handle` is CALLED here -- a different function than the one that
    DEFINED it (`make_handler`, which already returned). Its captured
    `writer` travels with the closure value; this function's env never had
    a `writer` at all.

    ID: f4000000-000c-0000-0000-000000000000
    """
    h = make_handler(JsonWriter())
    h(1)


def read_csv():
    """ID: f4000000-000d-0000-0000-000000000000"""
    h = make_handler(CsvWriter())
    h(2)


def entry():
    """The escaping-closure fixture, cross-scope: Jedi resolves `read_json`'s
    `handle` to JsonWriter.write and `read_csv`'s to CsvWriter.write, with
    `dynamic_params` off -- confirmed against the real driver, and now a
    named row in plan/03-call-tree/01's "Present via Jedi" list.

    ID: f4000000-000e-0000-0000-000000000000
    """
    read_json()
    read_csv()
