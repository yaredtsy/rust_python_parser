"""Callees for the other fixtures in this folder.

ID: eeeeeeee-0000-0000-0000-000000000000
"""


class Rendered:
    """ID: eeeeeeee-1111-1111-1111-111111111111"""

    def render(self):
        """ID: eeeeeeee-2222-2222-2222-222222222222"""
        return self

    def strip(self):
        """ID: eeeeeeee-3333-3333-3333-333333333333"""
        return self

    def __getitem__(self, key):
        """ID: eeeeeeee-4444-4444-4444-444444444444"""
        return self

    def __call__(self):
        """ID: eeeeeeee-5555-5555-5555-555555555555"""
        return self


def build():
    """ID: eeeeeeee-6666-6666-6666-666666666666"""
    return Rendered()


def wrap(value, key=None):
    """ID: eeeeeeee-7777-7777-7777-777777777777"""
    return (value, key)


def log(*args):
    """ID: eeeeeeee-8888-8888-8888-888888888888"""
    print(*args)
