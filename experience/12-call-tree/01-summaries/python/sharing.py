"""Which calls share a summary: the key, fingerprints, relevant parameters
and fp_depth. For 01-summaries/02-the-key.md.

ID: 51000000-0000-0000-0000-000000000000
"""


class JsonHandler:
    """ID: 51000000-0001-0000-0000-000000000000"""

    def process(self):
        """ID: 51000000-0002-0000-0000-000000000000"""
        return None


class CsvHandler:
    """ID: 51000000-0003-0000-0000-000000000000"""

    def process(self):
        """ID: 51000000-0004-0000-0000-000000000000"""
        return None


class Pipeline:
    """ID: 51000000-0005-0000-0000-000000000000"""

    def __init__(self, handler):
        """ID: 51000000-0006-0000-0000-000000000000"""
        self.handler = handler

    def run(self):
        """ID: 51000000-0007-0000-0000-000000000000"""
        self.handler.process()


def main():
    """Three objects, two distinct fingerprints: two `run` contexts.

    ID: 51000000-0008-0000-0000-000000000000
    """
    a = Pipeline(JsonHandler())
    b = Pipeline(JsonHandler())
    c = Pipeline(CsvHandler())
    a.run()
    b.run()
    c.run()


class Sink:
    """ID: 51000000-0009-0000-0000-000000000000"""


def audit(event, sink):
    """Writes to an input: both parameters are relevant.

    ID: 51000000-000a-0000-0000-000000000000
    """
    sink.last = event


class Config:
    """ID: 51000000-000b-0000-0000-000000000000"""

    def __init__(self, handler):
        """ID: 51000000-000c-0000-0000-000000000000"""
        self.handler = handler


class App:
    """ID: 51000000-000d-0000-0000-000000000000"""

    def __init__(self, config):
        """ID: 51000000-000e-0000-0000-000000000000"""
        self.config = config


class Service:
    """ID: 51000000-000f-0000-0000-000000000000"""

    def __init__(self, app):
        """ID: 51000000-0010-0000-0000-000000000000"""
        self.app = app

    def run(self):
        """Three steps from `self`: past fp_depth = 2.

        ID: 51000000-0011-0000-0000-000000000000
        """
        self.app.config.handler.process()


def deep():
    """ID: 51000000-0012-0000-0000-000000000000"""
    svc = Service(App(Config(JsonHandler())))
    svc.run()
