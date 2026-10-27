"""Functions as values. If binding (01-02) and returns (05) are already
right, everything in this file costs no new code at all.

ID: f6000000-0000-0000-0000-000000000000
"""


class Audit:
    """ID: f6000000-0001-0000-0000-000000000000"""

    def record(self, result):
        """ID: f6000000-0002-0000-0000-000000000000"""
        return None


class Alert:
    """ID: f6000000-0003-0000-0000-000000000000"""

    def page(self, err):
        """ID: f6000000-0004-0000-0000-000000000000"""
        return None


audit = Audit()
alert = Alert()


def on_success(result):
    """ID: f6000000-0005-0000-0000-000000000000"""
    audit.record(result)


def on_failure(err):
    """ID: f6000000-0006-0000-0000-000000000000"""
    alert.page(err)


def run(task, cb):
    """ID: f6000000-0007-0000-0000-000000000000"""
    cb(task)


def handle_ok():
    """Two DIFFERENT callers passing two DIFFERENT callbacks -- two `run`
    nodes, each with one correct leaf. Must be two callers, not two calls
    from one frame -- see 09-path-identity's merge rule for why the
    same-frame version would prove nothing.

    ID: f6000000-0008-0000-0000-000000000000
    """
    run("ok-task", on_success)


def handle_err():
    """ID: f6000000-0009-0000-0000-000000000000"""
    run("err-task", on_failure)


def main():
    """ID: f6000000-000a-0000-0000-000000000000"""
    handle_ok()
    handle_err()


def through_two_levels(cb):
    """ID: f6000000-000b-0000-0000-000000000000"""
    middle(cb)


def middle(cb):
    """ID: f6000000-000c-0000-0000-000000000000"""
    cb("value")


def call_through_levels():
    """A callback passed through TWO levels of parameters before being
    invoked -- the same eager-binding chain that makes
    `outer -> middle -> inner` pass-through work for ordinary values.

    ID: f6000000-000d-0000-0000-000000000000
    """
    through_two_levels(on_success)
