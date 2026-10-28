"""Step 03.07: chains, super() and annotations. One member lookup per hop;
super() binds the same receiver; a parameter nobody passed a value for
starts from its annotation."""


class Handler:
    def handle(self):
        pass


class LoudHandler(Handler):
    def handle(self):
        pass


class Service:
    def __init__(self, handler: Handler):
        self.handler = handler

    def call(self):
        self.handler.handle()


class Session:
    def __init__(self, service):
        self.service = service


class Base:
    def __init__(self, h):
        self.h = h


class Child(Base):
    def __init__(self, h):
        super().__init__(h)

    def go(self):
        self.h.handle()


def chain():
    s = Session(Service(LoudHandler()))
    s.service.handler.handle()


def write_then_read():
    s = Session(Service(Handler()))
    s.service.handler = LoudHandler()
    s.service.handler.handle()


def unknown_mid_chain(thing):
    thing.service.handler.handle()  # thing is Unknown: no edge, nothing guessed


def inherited():
    Child(LoudHandler()).go()


def annotated(h: Handler):
    h.handle()  # from the annotation


# expect:
# chain
#   LoudHandler
#   Service
#   Session
#   LoudHandler.handle
# expect:
# write_then_read
#   Handler
#   Service
#   Session
#   LoudHandler
#   LoudHandler.handle
# expect:
# unknown_mid_chain
# expect:
# inherited
#   LoudHandler
#   Child
#     Base.__init__
#   Child.go
#     LoudHandler.handle
# expect:
# annotated
#   Handler.handle
# expect:
# Service.call
#   Handler.handle
