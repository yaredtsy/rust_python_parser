"""The base case: evaluate the callee, evaluate the args, bind, walk the
body. One call, one binding, one context.

ID: f1000000-0000-0000-0000-000000000000
"""


class JsonWriter:
    """ID: f1000000-0002-0000-0000-000000000000"""

    def write(self, payload):
        """ID: f1000000-0003-0000-0000-000000000000"""
        return None


def emit(writer, payload):
    """ID: f1000000-0001-0000-0000-000000000000"""
    writer.write(payload)


def main():
    """`emit(JsonWriter(), 1)` -- one call, one binding of `writer`, one
    recursive walk of `emit`'s body. Trace this one by hand first.

    ID: f1000000-0004-0000-0000-000000000000
    """
    emit(JsonWriter(), 1)


class CsvWriter:
    """ID: f1000000-0006-0000-0000-000000000000"""

    def write(self, payload):
        """ID: f1000000-0007-0000-0000-000000000000"""
        return None


def read_json():
    """ID: f1000000-0005-0000-0000-000000000000"""
    emit(JsonWriter(), 1)


def read_csv():
    """ID: f1000000-0008-0000-0000-000000000000"""
    emit(CsvWriter(), 2)


def entry():
    """Two DIFFERENT functions calling the same `emit` -- the fixture that
    actually proves context sensitivity (plan/03-call-tree/02's
    illustration). Compare the tree this produces against `main`'s
    single-function shape above: same callee, different question.

    ID: f1000000-0009-0000-0000-000000000000
    """
    read_json()
    read_csv()
