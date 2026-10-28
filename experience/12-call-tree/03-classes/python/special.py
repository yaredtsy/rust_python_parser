"""Step 03.06: staticmethod, classmethod, property. The decorator decides
the receiver; a property read is a call to its getter."""


def on_ok():
    pass


class JsonHandler:
    def handle(self):
        pass


class Registry:
    def __init__(self, h):
        self.h = h

    @staticmethod
    def make(cb):
        cb()

    @classmethod
    def create(cls, h):
        return cls(h)

    @property
    def active(self):
        return self.h

    def fire(self):
        self.active.handle()


def main():
    Registry.make(on_ok)  # no receiver: cb is on_ok
    r = Registry.create(JsonHandler())  # cls is Registry
    r.active.handle()  # the getter is called at `r.active`
    r.fire()
    r.make(on_ok)  # a staticmethod through an instance: still no receiver


# expect:
# main
#   Registry.make
#     on_ok
#   JsonHandler
#   Registry.create
#     Registry
#   Registry.active
#   JsonHandler.handle
#   Registry.fire
#     Registry.active
#     JsonHandler.handle
#   Registry.make
#     on_ok
