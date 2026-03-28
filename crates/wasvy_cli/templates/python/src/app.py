# filename: src/app.py
import bindings
from bindings.imports.app import App, Commands, Query, QueryFor_Mut, Schedule_ModStartup, Schedule_Update, System

@dataclass
class Name:
    value: str

class Example(bindings.Example):
    def setup(self, app: App):
        app.add_systems(Schedule_ModStartup(), [System("start")])
        app.add_systems(Schedule_Update(), [System("update")])

    def start(self, commands: Commands):
        commands.spawn([
            ("bevy_ecs::name::Name", json.dumps(component_2)),
        ])

        print(f"Hello from {{ name }}")

    def update(self, query: Query):
        # Do some cool stuff!
