"""Step 03.08: entries. Every function is its own system, started like main.
The prologue walks down from the module to the entry: each enclosing function
is run (closures read its run), and a method gets an object built for it
(__init__ runs). Its effects are kept, its nodes are not shown. Callers never
matter."""


def log():
    pass


class Handler:
    def handle(self):
        pass


class JsonHandler:
    def process(self):
        pass


class A:
    def go(self):
        pass


class B:
    def go(self):
        pass


def process(h: Handler, n=3):
    h.handle()  # a function: no prologue, h from its annotation


class Pipeline:
    def __init__(self, handler):
        self.handler = handler


class Svc:
    def __init__(self):
        self.p = Pipeline(JsonHandler())  # an object graph only __init__ knows
        log()

    def run(self):
        self.p.handler.process()

    def build(self):
        def step():
            self.p.handler.process()  # `self` is a free variable of step

        return step


class Base:
    def __init__(self, h):
        self.h = h


class Child(Base):
    def __init__(self):
        super().__init__(JsonHandler())

    def go(self):
        self.h.process()


class Job:
    def __init__(self):
        self.h = A()

    def configure(self):
        self.h = B()

    def run(self):
        self.h.go()  # only __init__ is part of the start: A.go


class Reg:
    def __init__(self):
        log()

    @classmethod
    def create(cls):
        cls()  # the entry's own call: shown

    @staticmethod
    def make(cb):
        cb()  # cb is unknown: no edge


class Box:
    def __init__(self):
        self._h = JsonHandler()

    @property
    def h(self):
        self._h.process()
        return self._h


def outer():
    h = A()

    def inner():
        h.go()  # read when inner runs: after outer's last line, h is B

    h = B()


def a():
    x = A()

    def b():
        y = B()

        def c():
            x.go()  # from a's run
            y.go()  # from b's run

        return c

    return b


def make():
    h = JsonHandler()

    class Local:
        def __init__(self):
            self.h2 = h  # __init__ reads make's run too: the class is skipped

        def run(self):
            h.process()  # make's local, through Local
            self.h2.process()

    return Local


class Outer:
    def __init__(self):
        log()  # never run for Outer.Inner.m: Inner's methods can't see an Outer

    class Inner:
        def __init__(self):
            self.h = JsonHandler()

        def m(self):
            self.h.process()


class Tree:
    def __init__(self):
        Tree()  # recursive, inside the prologue: hidden

    def walk(self):
        log()


# expect:
# process
#   Handler.handle
# expect:
# Svc.run
#   JsonHandler.process
# expect:
# Child.go
#   JsonHandler.process
# expect:
# Job.run
#   A.go
# expect:
# Reg.create
#   Reg
#     log
# expect:
# Reg.make
# expect:
# Box.h
#   JsonHandler.process
# expect:
# outer.inner
#   B.go
# expect:
# Svc.build.step
#   JsonHandler.process
# expect:
# a.b.c
#   A.go
#   B.go
# expect:
# make.Local.run
#   JsonHandler.process
#   JsonHandler.process
# expect:
# Outer.Inner.m
#   JsonHandler.process
# expect:
# Tree.walk
#   log
