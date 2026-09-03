"""The file the whole port exists for.

`emit` is called from two different frames with two different writers.
Ask ty what `writer.write` is and it must answer with BOTH, because both
are possible somewhere in the program. Ask jedi from inside one path and
it answers with ONE, because it simulated that path.

The call tree needs the second answer. That gap is exercise 09.

FileID: 18181818-0000-0000-0000-000000000000
"""

import json


class JsonWriter:
    """ID: 18181818-1111-1111-1111-111111111111"""

    def write(self, data):
        """ID: 18181818-2222-2222-2222-222222222222"""
        return json.dumps(data)

    def close(self):
        """ID: 18181818-3333-3333-3333-333333333333"""
        return None


class XmlWriter:
    """ID: 18181818-4444-4444-4444-444444444444"""

    def write(self, data):
        """ID: 18181818-5555-5555-5555-555555555555"""
        return "<x>%s</x>" % data

    def close(self):
        """ID: 18181818-6666-6666-6666-666666666666"""
        return None


def emit(writer, data):
    """One definition. Two behaviours, depending on who called it.

    ID: 18181818-7777-7777-7777-777777777777
    """
    writer.write(data)
    writer.close()


def run_json(payload):
    """Frame A.

    ID: 18181818-8888-8888-8888-888888888888
    """
    emit(JsonWriter(), payload)


def run_xml(payload):
    """Frame B. Same callee, different argument.

    ID: 18181818-9999-9999-9999-999999999999
    """
    emit(XmlWriter(), payload)


def pass_through(writer, payload):
    """Three levels deep: the writer travels down two calls before use.

    ID: 18181818-aaaa-aaaa-aaaa-aaaaaaaaaaaa
    """
    emit(writer, payload)


def run_deep(payload):
    """ID: 18181818-bbbb-bbbb-bbbb-bbbbbbbbbbbb"""
    pass_through(JsonWriter(), payload)
