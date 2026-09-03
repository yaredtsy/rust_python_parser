"""Callees for this folder's fixtures.

ID: 15151515-0000-0000-0000-000000000000
"""


def build():
    """ID: 15151515-1111-1111-1111-111111111111"""
    return "built"


def wrap(value, key=None):
    """ID: 15151515-2222-2222-2222-222222222222"""
    return (value, key)


def log(*args):
    """ID: 15151515-3333-3333-3333-333333333333"""
    print(*args)
