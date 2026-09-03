"""Scope nesting. The scan must stop at a nested def or class.

ID: aaaaaaaa-0000-0000-0000-000000000000
"""

from helpers import build, log


def outer():
    """`build()` here belongs to outer. `log()` belongs to inner.

    ID: aaaaaaaa-1111-1111-1111-111111111111
    """
    build()

    def inner():
        """ID: aaaaaaaa-2222-2222-2222-222222222222"""
        log()

    inner()


class Container:
    """A class body can hold calls, defs and classes.

    ID: aaaaaaaa-3333-3333-3333-333333333333
    """

    registry = build()

    def method(self):
        """ID: aaaaaaaa-4444-4444-4444-444444444444"""
        log()

        class Inner:
            """ID: aaaaaaaa-5555-5555-5555-555555555555"""

            def deep(self):
                """ID: aaaaaaaa-6666-6666-6666-666666666666"""
                build()


def with_blocks(flag):
    """defs can hide inside if/with/try. The scan must find them.

    ID: aaaaaaaa-7777-7777-7777-777777777777
    """
    if flag:

        def conditional():
            """ID: aaaaaaaa-8888-8888-8888-888888888888"""
            log()

        conditional()

    try:
        build()
    except ValueError:
        log()
    finally:
        build()
