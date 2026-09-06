"""Boids on a continuous torus, updated synchronously.

Henad's rule rather than Reynolds's, and rather than the one Mesa's own `boid_flockers` example
implements: separation accumulates the offsets to everything inside the protected range, alignment
and cohesion average over everything inside the visual range, and the resulting velocity is clamped
into a speed band. Velocity is the displacement, so a tick is one unit of time.

Mesa's continuous space supplies the neighbour query and the shortest offset round the torus. The
radius query is a candidate set only: it is inclusive at the boundary where Henad's test is strict,
so both range tests are applied exactly afterwards.
"""

import numpy as np
from mesa import Model
from mesa.experimental.continuous_space import ContinuousSpace, ContinuousSpaceAgent


class Boid(ContinuousSpaceAgent):
    def __init__(self, model, space, position, velocity):
        super().__init__(space, model)
        self.position = np.asarray(position, dtype=float)
        self.velocity = np.asarray(velocity, dtype=float)
        self._next_position = self.position
        self._next_velocity = self.velocity

    def determine_state(self):
        model = self.model
        neighbors, _ = self.get_neighbors_in_radius(radius=model.visual_range)
        velocity = self.velocity
        close = np.zeros(2)
        summed_offset = np.zeros(2)
        summed_velocity = np.zeros(2)
        seen = 0

        if neighbors:
            deltas = self.space.calculate_difference_vector(self.position, agents=neighbors)
            distances_sq = np.einsum("ij,ij->i", deltas, deltas)
            for neighbor, delta, distance_sq in zip(neighbors, deltas, distances_sq):
                if distance_sq < model.protected_sq:
                    close -= delta
                if distance_sq < model.visual_sq:
                    summed_velocity += neighbor.velocity
                    summed_offset += delta
                    seen += 1

        new_velocity = velocity + close * model.separation
        if seen:
            new_velocity = new_velocity + (summed_velocity / seen - velocity) * model.alignment
            new_velocity = new_velocity + (summed_offset / seen) * model.cohesion

        speed = float(np.hypot(*new_velocity))
        if speed > 0.0:
            if speed > model.max_speed:
                new_velocity = new_velocity / speed * model.max_speed
            elif speed < model.min_speed:
                new_velocity = new_velocity / speed * model.min_speed
        else:
            new_velocity = np.array([model.min_speed, 0.0])

        self._next_velocity = new_velocity
        self._next_position = (self.position + new_velocity) % model.world

    def assume_state(self):
        self.velocity = self._next_velocity
        self.position = self._next_position


class Boids(Model):
    def __init__(
        self,
        num_agents=50_000,
        world_width=1000.0,
        world_height=1000.0,
        visual_range=50.0,
        protected_range=8.0,
        separation=0.05,
        alignment=0.05,
        cohesion=0.0005,
        max_speed=15.0,
        min_speed=3.0,
        agents=None,
        rng=None,
    ):
        """`agents` places an exact `(x, y, vx, vy)` list, for a gate scenario; otherwise the
        population is scattered at random with a fixed speed and a random heading, as Henad does."""
        super().__init__(rng=rng)
        self.world = np.array([world_width, world_height])
        self.visual_range = visual_range
        self.visual_sq = visual_range * visual_range
        self.protected_sq = protected_range * protected_range
        self.separation = separation
        self.alignment = alignment
        self.cohesion = cohesion
        self.max_speed = max_speed
        self.min_speed = min_speed

        if agents is None:
            speed = 0.5 * (min_speed + max_speed)
            agents = []
            for _ in range(num_agents):
                angle = self.random.random() * 2.0 * np.pi
                agents.append(
                    (
                        self.random.random() * world_width,
                        self.random.random() * world_height,
                        np.cos(angle) * speed,
                        np.sin(angle) * speed,
                    )
                )

        self.space = ContinuousSpace(
            [[0, world_width], [0, world_height]], torus=True, random=self.random, n_agents=len(agents)
        )
        for x, y, vx, vy in agents:
            Boid(self, self.space, (x, y), (vx, vy))

    def step(self):
        self.agents.do("determine_state")
        self.agents.do("assume_state")

    def population(self):
        return len(self.agents)

    def rows(self):
        """`x y vx vy` per agent, in creation order, as the fixture format wants."""
        return [(a.position[0], a.position[1], a.velocity[0], a.velocity[1]) for a in self.agents]
