"""Call shapes. Every line here becomes one or more CallNodes.

ID: 11111111-1111-1111-1111-111111111111
"""

from helpers import build, wrap


def simple():
    """ID: 22222222-2222-2222-2222-222222222222"""
    build()


def chained():
    """Two calls in one expression: call_index 0 and 1.

    ID: 33333333-3333-3333-3333-333333333333
    """
    build().render()


def deep_chain():
    """Three calls, one chain.

    ID: 44444444-4444-4444-4444-444444444444
    """
    build().render().strip()


def nested_args():
    """A call inside another call's arguments.

    ID: 55555555-5555-5555-5555-555555555555
    """
    wrap(build(), key=build())


def call_of_call():
    """The callee is itself a call.

    ID: 66666666-6666-6666-6666-666666666666
    """
    build()()


def subscripted():
    """Subscript between two calls.

    ID: 77777777-7777-7777-7777-777777777777
    """
    build()["key"].render()


def in_fstring(value):
    """A call inside an f-string. Tokenisation of this depends on the
    target Python version -- see exercise 04.

    ID: 88888888-8888-8888-8888-888888888888
    """
    return f"result={build()} and {wrap(value)}"


def multiline_callee(obj):
    """The callee spans lines. What is `name` here?

    ID: 99999999-9999-9999-9999-999999999999
    """
    return obj \
        . render (value=1)
