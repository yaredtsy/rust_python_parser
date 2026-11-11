"""Step 05.02: the wire names each target by file, qualname and occurrence. This is the callee's file."""

import os


def twin():
    return 1


class Store:
    def save(self):
        return os.getcwd()


if os.name:

    def twin():  # a second def with this qualname: occurrence 2
        return 2
