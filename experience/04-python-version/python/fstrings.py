"""f-string tokenisation -- the insidious one.

PEP 701 (Python 3.12) changed how f-strings are tokenised. The constructs
below are 3.12+ only. More importantly, the RANGES of tokens inside every
f-string can differ between targets -- including the plain ones.

ID: 13131313-0000-0000-0000-000000000000
"""

from helpers import build, wrap


def plain(name):
    """A call inside a plain f-string. Note the column of `build`.

    ID: 13131313-1111-1111-1111-111111111111
    """
    return f"value={build()} for {name}"


def nested_same_quotes(name):
    """3.12+: the inner f-string reuses the outer quote character.

    ID: 13131313-2222-2222-2222-222222222222
    """
    return f"outer {f"inner {build()}"} end"


def with_backslash(items):
    """3.12+: a backslash inside the expression part.

    ID: 13131313-3333-3333-3333-333333333333
    """
    return f"joined {"\n".join(items)}"


def multiline_expression(a, b):
    """3.12+: the expression part spans lines and holds a comment.

    ID: 13131313-4444-4444-4444-444444444444
    """
    return f"sum={
        wrap(a,  # the first operand
             b)
    }"


def deep(value):
    """Two calls in one f-string, one nested in the other's arguments.

    ID: 13131313-5555-5555-5555-555555555555
    """
    return f"{wrap(build(), key=value)}"
