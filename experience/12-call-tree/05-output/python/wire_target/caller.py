"""Step 05.02: the caller. Its calls go to `callee.py`; `os.getcwd` is outside the project."""

from callee import Store, twin


class Service:
    def run(self):
        Store().save()
        twin()

    def go(self):
        def inner():
            return twin()

        return inner()
