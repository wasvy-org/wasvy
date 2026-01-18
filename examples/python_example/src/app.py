import json
import math
from typing import List
from dataclasses import dataclass, asdict

import example
from example.imports.app import App, Commands, Query, QueryFor_Mut, QueryFor_With, Schedule_Update, System

class Example(example.Example):
    def setup(self, app: App):
        spin_cube = System("spin-cube")
        spin_cube.add_query([
            QueryFor_Mut("bevy_transform::components::transform::Transform"),
            QueryFor_With("host_example::MyMarker"),
        ])

        my_system = System("my-system")
        my_system.add_commands()
        my_system.add_query([
            QueryFor_With("python::MyComponent")
        ])

        app.add_systems(Schedule_Update(), [my_system, spin_cube])

    # Spin speed
    DELTA = 0.015

    def spin_cube(self, query: Query):
        """Advance rotation about the x-axis for the single component returned per iter()."""
        while True:
            components = query.iter()
            if components is None:
                break

            component = components[0]

            transform = json.loads(component.get())

            rotation = transform["rotation"]
            q = q_normalize([float(rotation[0]), float(rotation[1]), float(rotation[2]), float(rotation[3])])

            # Use the current quaternion (the "previous x rotation") and advance about X:
            dq = q_from_xaxis(self.DELTA)
            q_next = q_normalize(q_mul(q, dq))

            transform["rotation"] = q_next
            component.set(json.dumps(transform))
    
    def my_system(self, commands: Commands, query: Query):
        # Count how many entities we've spawned
        count = 0
        while True:
            components = query.iter()
            if components is None:
                break
            count += 1

        # Avoid spawning more than 10
        if count >= 10:
            return

        @dataclass
        class MyComponent:
            value: int
        
        print("Spawning an entity with MyComponent component in my-system")

        component_1 = asdict(MyComponent(value=0))

        # Default transform
        component_2 = {
            "translation": [0.0, 0.0, 0.0],
            "rotation":    [1.0, 0.0, 0.0, 0.0],
            "scale":       [1.0, 1.0, 1.0],
        }

        commands.spawn([
            ("python::MyComponent", json.dumps(component_1)),
            ("bevy_transform::components::transform::Transform", json.dumps(component_2)),
        ])

def q_normalize(q: List[float]) -> List[float]:
    w, x, y, z = q
    n = math.sqrt(w*w + x*x + y*y + z*z)
    return [1.0, 0.0, 0.0, 0.0] if n == 0 else [w/n, x/n, y/n, z/n]

def q_mul(a: List[float], b: List[float]) -> List[float]:
    aw, ax, ay, az = a
    bw, bx, by, bz = b
    return [
        aw*bw - ax*bx - ay*by - az*bz,
        aw*bx + ax*bw + ay*bz - az*by,
        aw*by - ax*bz + ay*bw + az*bx,
        aw*bz + ax*by - ay*bx + az*bw,
    ]

def q_from_xaxis(delta_rad: float) -> List[float]:
    h = 0.5 * delta_rad
    return [math.cos(h), math.sin(h), 0.0, 0.0]