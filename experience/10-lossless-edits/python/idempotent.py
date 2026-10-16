"""A second run must change nothing. Three of these four defs look done and
are not, or look undone and are not.

FileID: 10101010-1111-1111-1111-111111111111
"""


def has_id():
    """Done.

    ID: 10101010-2222-2222-2222-222222222222
    """
    return 1


def prose_mentions_id():
    """The text says ID: nothing-real, and there is no real key below.

    Owner: platform
    """
    return 2


def has_other_keys():
    """Metadata, but no ID.

    Owner: platform
    Status: draft
    """
    return 3


class Mixed:
    """ID: 10101010-3333-3333-3333-333333333333"""

    def needs_one(self):
        """Prose only."""
        return 4
