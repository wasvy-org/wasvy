import example
from example.imports.app import App, Schedule_ModStartup, System

class Example(example.Example):
    def setup(self, app: App):
        my_system = System("my-system")

        app.add_systems(Schedule_ModStartup(), [my_system])

    def my_system(self):
        print(f"Hello from {{ name }}")
