# Every insert shape, in one file. NOTHING here has an `ID:`, so injection
# must touch all thirteen definitions (eleven at top level, two nested in
# NoDoc) — and the file must still import afterwards.


def triple_multi():
    """Several lines of prose.

    The key joins the end of this block, at this indentation.
    """
    return 1


def triple_single_line():
    """One line, triple quoted."""
    return 2


def single_quoted():
    'Single quotes: a newline before the closer would not parse.'
    return 3


def implicit_concat():
    """First part. """ \
        """Second part holds the closing quotes."""
    return 4


def ends_with_backslash():
    """The content ends with a backslash: C:\\"""
    return 5


def raw_prefixed():
    r"""A raw docstring: the path is C:\new\table and \d+ is a regex.

    Dropping the r prefix changes what this string MEANS.
    """
    return 6


def ends_with_colon():
    """Example:"""
    return 7


def no_docstring():
    return 8


def one_liner(): return 9


def ellipsis_body(): ...


def semicolon_body(): a = 1; return a


class NoDoc:
    def method(self):
        return 10

    class Inner:
        """Inner has prose but no key."""
