"""A write deeper than the key reaches the caller by path.
For 03-classes/05-writes-anywhere-and-deep-writes.md
(plan/03-call-tree/07 fixtures 12 and 13).

ID: c9000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: c9000000-0001-0000-0000-000000000000"""

    def process(self):
        """ID: c9000000-0002-0000-0000-000000000000"""
        return None


class CsvHandler:
    """ID: c9000000-0003-0000-0000-000000000000"""

    def process(self):
        """ID: c9000000-0004-0000-0000-000000000000"""
        return None


class Sink:
    """ID: c9000000-0005-0000-0000-000000000000"""

    def __init__(self, h):
        """ID: c9000000-0006-0000-0000-000000000000"""
        self.h = h


class Config:
    """ID: c9000000-0007-0000-0000-000000000000"""

    def __init__(self, sink):
        """ID: c9000000-0008-0000-0000-000000000000"""
        self.sink = sink


class App:
    """ID: c9000000-0009-0000-0000-000000000000"""

    def __init__(self, config):
        """ID: c9000000-000a-0000-0000-000000000000"""
        self.config = config


class Service:
    """ID: c9000000-000b-0000-0000-000000000000"""

    def __init__(self, app):
        """ID: c9000000-000c-0000-0000-000000000000"""
        self.app = app


def rewire(svc):
    """Three steps below the key's depth (fp_depth = 2). The effect comes back
    as Param(0).app.config.sink.h.

    ID: c9000000-000d-0000-0000-000000000000
    """
    svc.app.config.sink.h = JsonHandler()


def main():
    """Default limits: JsonHandler.process only.
    With max_path = 2: CsvHandler.process, rewire marked Incomplete.

    ID: c9000000-000e-0000-0000-000000000000
    """
    svc = Service(App(Config(Sink(CsvHandler()))))
    rewire(svc)
    svc.app.config.sink.h.process()
