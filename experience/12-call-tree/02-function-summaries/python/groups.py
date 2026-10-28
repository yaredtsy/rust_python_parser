"""Step 02.06: recursion groups. Functions that call each other form one
group, walked once each and solved together, the same from any entry."""


def done():
    pass


def num():
    pass


def binop():
    pass


# two functions
def f(n):
    return g(n)


def g(n):
    if n:
        return f(n - 1)
    return done


# three: a -> b -> c -> a
def a(n):
    if n:
        return b(n)
    return done


def b(n):
    return c(n)


def c(n):
    return a(n - 1)


# a parser: combine is first reached during the fixpoint, and joins
def parse_expr(t):
    left = parse_atom(t)
    return combine(left, t)


def parse_atom(t):
    if t:
        return parse_expr(t)
    return num


def combine(left, t):
    if t:
        parse_expr(t)
        return binop
    return left


def main():
    f(1)()
    b(2)()
    parse_expr(1)()


# a recursive call's value used in the caller of the cycle
def ping(n):
    r = pong(n)
    r()  # summary: done. Inline: r is Unknown
    return done


def pong(n):
    return ping(n - 1)


def ping_entry():
    ping(2)


# a group that turns out to be part of a bigger one
def outer(n):
    return x(n)


def x(n):
    if n:
        return y(n)
    return outer


def y(n):
    h = x(n - 1)
    return h(n)  # in the fixpoint h is outer, still running further down: merge


def merge_entry():
    outer(1)


# expect:
# main
#   f
#     g
#       f [recursive]
#   done
#   b
#     c
#       a
#         b [recursive]
#   done
#   parse_expr
#     parse_atom
#       parse_expr [recursive]
#     combine
#       parse_expr [recursive]
#   binop
#   num
# expect inline:
# ping_entry
#   ping
#     pong
#       ping [recursive]
# expect summary:
# ping_entry
#   ping
#     pong
#       ping [recursive]
#     done
# expect inline:
# merge_entry
#   outer
#     x
#       y
#         x [recursive]
# expect summary:
# merge_entry
#   outer
#     x
#       y
#         x [recursive]
#         outer [recursive]
