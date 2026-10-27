"""The four receiver-binding rules -- plain method, @staticmethod,
@classmethod, @property -- plus an attribute chain and a classmethod
factory.

ID: c7000000-0000-0000-0000-000000000000
"""


class Handler:
    """ID: c7000000-0001-0000-0000-000000000000"""

    def handle(self, x):
        """ID: c7000000-0002-0000-0000-000000000000"""
        return x


class Service:
    """ID: c7000000-0003-0000-0000-000000000000"""

    def __init__(self, handler):
        """ID: c7000000-0004-0000-0000-000000000000"""
        self.handler = handler

    def method(self, x):
        """Plain method: first param <- the instance.

        ID: c7000000-0005-0000-0000-000000000000
        """
        self.handler.handle(x)

    @staticmethod
    def static(x):
        """@staticmethod: NO receiver. Positionals start at param 0.

        ID: c7000000-0006-0000-0000-000000000000
        """
        return x

    @classmethod
    def create(cls, handler):
        """@classmethod: first param <- the CLASS, not an instance.

        ID: c7000000-0007-0000-0000-000000000000
        """
        return cls(handler)

    @property
    def active_handler(self):
        """@property: accessed WITHOUT `()`. The getter still runs through
        `bind_parameters` like any method -- it's just invoked implicitly
        by the attribute access itself.

        ID: c7000000-0008-0000-0000-000000000000
        """
        return self.handler


class Session:
    """ID: c7000000-0009-0000-0000-000000000000"""

    def __init__(self, service):
        """ID: c7000000-000a-0000-0000-000000000000"""
        self.service = service


def plain_method_call():
    """ID: c7000000-000b-0000-0000-000000000000"""
    Service(Handler()).method(1)


def static_call():
    """ID: c7000000-000c-0000-0000-000000000000"""
    Service.static(1)


def classmethod_call():
    """`Service.create(...)` -- a classmethod constructor, the same SHAPE
    as a `Pipeline.create` factory in real code.

    ID: c7000000-000d-0000-0000-000000000000
    """
    Service.create(Handler()).method(2)


def property_access():
    """ID: c7000000-000e-0000-0000-000000000000"""
    s = Service(Handler())
    s.active_handler.handle(3)


def attribute_chain():
    """`self.service.handler.handle(...)` -- three hops: Session ->
    Service -> Handler.

    ID: c7000000-000f-0000-0000-000000000000
    """
    session = Session(Service(Handler()))
    session.service.handler.handle(4)
