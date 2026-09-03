"""Docstring shapes. Your ID extraction must survive all of these.

FileID: cccccccc-0000-0000-0000-000000000000
"""


def plain():
    """ID: cccccccc-1111-1111-1111-111111111111"""


def raw():
    r"""A raw docstring with a backslash \n inside.

    ID: cccccccc-2222-2222-2222-222222222222
    """


def single_quotes():
    'ID: cccccccc-3333-3333-3333-333333333333'


def implicit_concat():
    """First part. """ \
    """ID: cccccccc-4444-4444-4444-444444444444"""


def no_docstring():
    pass


def not_first_statement():
    x = 1
    """ID: dddddddd-0000-0000-0000-000000000000"""
    return x


def id_like_text():
    """This mentions ID: but as prose, then the real one.

    ID: cccccccc-5555-5555-5555-555555555555
    """


def multiple_keys():
    """Metadata with several pairs.

    FileID: cccccccc-6666-6666-6666-666666666666
    ID: cccccccc-7777-7777-7777-777777777777
    Owner: platform
    """


def unicode_doc():
    """Résumé 🎉 — non-ASCII before the key.

    ID: cccccccc-8888-8888-8888-888888888888
    """


class Documented:
    """ID: cccccccc-9999-9999-9999-999999999999"""

    def method(self):
        """ID: cccccccc-aaaa-aaaa-aaaa-aaaaaaaaaaaa"""
