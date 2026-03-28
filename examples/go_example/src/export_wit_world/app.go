package export_wit_world

import (
	"encoding/json"
	"fmt"
	"math"
	"wit_component/wasvy_ecs_app"
	. "wit_component/wit_world"

	witTypes "go.bytecodealliance.org/pkg/wit/types"
)

const (
	// Spin speed
	DELTA = 0.015
)

// Setup initializes the app with systems
func Setup(app *App) {
	// Create spin-cube system
	spinCube := wasvy_ecs_app.MakeSystem("spin-cube")
	spinCube.AddQuery([]wasvy_ecs_app.QueryFor{
		wasvy_ecs_app.MakeQueryForMut("bevy_transform::components::transform::Transform"),
		wasvy_ecs_app.MakeQueryForWith("host_example::MyMarker"),
	})

	// Create my-system
	mySystem := wasvy_ecs_app.MakeSystem("my-system")
	mySystem.AddCommands()
	mySystem.AddQuery([]wasvy_ecs_app.QueryFor{
		wasvy_ecs_app.MakeQueryForWith("go::MyComponent"),
	})

	// Add systems to update schedule
	app.AddSystems(wasvy_ecs_app.MakeScheduleUpdate(), []*wasvy_ecs_app.System{mySystem, spinCube})
}

// SpinCube advances rotation about the x-axis for the single component returned per iter()
func SpinCube(query *Query) {
	for {
		queryResult := query.Iter()
		if queryResult.IsNone() {
			break
		}

		component := queryResult.Some().Component(0)

		var transform map[string]interface{}
		if err := json.Unmarshal([]byte(component.Get()), &transform); err != nil {
			continue
		}

		rotation := transform["rotation"].([]interface{})
		q := qNormalize([]float64{
			rotation[0].(float64),
			rotation[1].(float64),
			rotation[2].(float64),
			rotation[3].(float64),
		})

		// Use the current quaternion (the "previous x rotation") and advance about X:
		dq := qFromXaxis(DELTA)
		qNext := qNormalize(qMul(q, dq))

		transform["rotation"] = qNext
		jsonBytes, _ := json.Marshal(transform)
		component.Set(string(jsonBytes))
	}
}

// MySystem spawns entities with MyComponent component
func MySystem(commands *Commands, query *Query) {
	// Count how many entities we've spawned
	count := 0
	for {
		components := query.Iter()
		if components.IsNone() {
			break
		}
		count++
	}

	// Avoid spawning more than 10
	if count >= 10 {
		return
	}

	fmt.Println("Spawning an entity with MyComponent component in my-system")

	// MyComponent with value
	component1 := map[string]int{
		"value": 0,
	}
	component1JSON, _ := json.Marshal(component1)

	// Default transform
	component2 := map[string]interface{}{
		"translation": []float64{0.0, 0.0, 0.0},
		"rotation":    []float64{1.0, 0.0, 0.0, 0.0},
		"scale":       []float64{1.0, 1.0, 1.0},
	}
	component2JSON, _ := json.Marshal(component2)

	// Spawn entity with components
	bundle := []witTypes.Tuple2[string, string]{
		{F0: "go::MyComponent", F1: string(component1JSON)},
		{F0: "bevy_transform::components::transform::Transform", F1: string(component2JSON)},
	}
	commands.Spawn(bundle)
}

// qNormalize normalizes a quaternion
func qNormalize(q []float64) []float64 {
	w, x, y, z := q[0], q[1], q[2], q[3]
	n := math.Sqrt(w*w + x*x + y*y + z*z)
	if n == 0 {
		return []float64{1.0, 0.0, 0.0, 0.0}
	}
	return []float64{w / n, x / n, y / n, z / n}
}

// qMul multiplies two quaternions
func qMul(a, b []float64) []float64 {
	aw, ax, ay, az := a[0], a[1], a[2], a[3]
	bw, bx, by, bz := b[0], b[1], b[2], b[3]
	return []float64{
		aw*bw - ax*bx - ay*by - az*bz,
		aw*bx + ax*bw + ay*bz - az*by,
		aw*by - ax*bz + ay*bw + az*bx,
		aw*bz + ax*by - ay*bx + az*bw,
	}
}

// qFromXaxis creates a quaternion from rotation about x-axis
func qFromXaxis(deltaRad float64) []float64 {
	h := 0.5 * deltaRad
	return []float64{math.Cos(h), math.Sin(h), 0.0, 0.0}
}
