"""Module docstring with no FileID yet."""


def already_has_id():
    """This one is done.

    ID: 1e1e1e1e-1111-1111-1111-111111111111
    """
    return 1


def has_doc_no_id():
    """A docstring, but no ID key. Injection must ADD the key without
    destroying the prose.
    """
    return 2


def raw_prefixed():
    r"""A raw docstring: the path is C:\new\table and \d+ is a regex.

    Dropping the r prefix changes what this string MEANS.
    """
    return 3


def contains_triple_quotes():
    """This docstring mentions \"\"\" inside itself, which the current
    Python implementation cannot rebuild correctly.
    """
    return 4


def single_quoted():
    'Short form, single quotes.'
    return 5


class Documented:
    """A class docstring with an ID.

    ID: 1e1e1e1e-2222-2222-2222-222222222222
    """

    def method(self):
        """No ID here."""
        return 6
