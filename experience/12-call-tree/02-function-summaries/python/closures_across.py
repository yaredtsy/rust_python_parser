"""Step 02.07: closures across a summary. A closure leaves its walk carrying
its free values, never its activation, and enters a key the same way."""


def a():
    pass


def b():
    pass


def make(f):
    def run():
        f()

    return run


def call_it(cb):
    cb()


def wrap(f):
    return make(f)


def passed_along():
    call_it(make(a))


def two_makers():
    call_it(make(a))
    call_it(make(b))  # another key for call_it: the closure's free value differs


def two_levels():
    wrap(b)()


def outer(root):
    def walk(n):
        walk(n)  # walk's free variable `walk` is walk itself: Back(0)

    walk(root)


def chain(n):
    if n:
        r = chain(n - 1)
    else:
        r = a

    def g():
        r()

    return g


def chained():
    chain(3)()  # each round nests g one level deeper, until fp_depth


# expect:
# passed_along
#   make
#   call_it
#     make.run
#       a
# expect:
# two_makers
#   make
#   call_it
#     make.run
#       a
#   make
#   call_it
#     make.run
#       b
# expect:
# two_levels
#   wrap
#     make
#   make.run
#     b
# expect:
# outer
#   outer.walk
#     outer.walk [recursive]
# expect inline:
# chained
#   chain
#     chain [recursive]
#   chain.g
#     a
# expect summary:
# chained
#   chain
#     chain [recursive]
#   chain.g
#     a
#     chain.g [recursive]
