"""Step 04.01: objects in keys. An object is keyed by its class and its
attributes, two steps deep. Where it was built is not in the key, so equal
objects share one summary. Box has no __init__: its state is written by the
caller, so this step needs no effects yet."""


class JsonHandler:
    def process(self):
        pass


class CsvHandler:
    def process(self):
        pass


class Box:
    def run(self):
        self.handler.process()


def json_box():
    b = Box()
    b.handler = JsonHandler()
    b.run()


def csv_box():
    b = Box()
    b.handler = CsvHandler()
    b.run()


def main():
    json_box()
    csv_box()
    json_box()


def shared():
    a = Box()
    a.handler = JsonHandler()
    b = Box()
    b.handler = JsonHandler()
    a.run()
    b.run()  # the same key as a.run(): one summary of Box.run


def use(b):
    b.run()


def passed():
    b = Box()
    b.handler = CsvHandler()
    use(b)


class Config:
    pass


class App:
    pass


class Service:
    def run(self):
        self.app.config.handler.process()  # three steps from self


def deep():
    c = Config()
    c.handler = JsonHandler()
    a = App()
    a.config = c
    s = Service()
    s.app = a
    s.run()  # fp_depth = 2: the key stops at self.app.config. A known limit


# expect:
# main
#   json_box
#     Box
#     JsonHandler
#     Box.run
#       JsonHandler.process
#   csv_box
#     Box
#     CsvHandler
#     Box.run
#       CsvHandler.process
#   json_box
#     Box
#     JsonHandler
#     Box.run
#       JsonHandler.process
# expect:
# shared
#   Box
#   JsonHandler
#   Box
#   JsonHandler
#   Box.run
#     JsonHandler.process
#   Box.run
#     JsonHandler.process
# expect:
# passed
#   Box
#   CsvHandler
#   use
#     Box.run
#       CsvHandler.process
# expect inline:
# deep
#   Config
#   JsonHandler
#   App
#   Service
#   Service.run
#     JsonHandler.process
# expect summary:
# deep
#   Config
#   JsonHandler
#   App
#   Service
#   Service.run
