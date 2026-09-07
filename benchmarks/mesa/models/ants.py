"""Ant foraging on a bounded lattice over two pheromone layers.

Ants lay a trail for the trip they are making and follow the trail for the trip they are not, so a
colony that finds food leaves a path back to it. Written from Henad's declaration, which differs
from the MASON and krABMaga models it descends from: deposits combine with `max` rather than
last-writer-wins, and the whole field is read before any of it is written.

Mesa supplies the lattice and the two property layers; the passes are ordinary agent loops.
"""

import math

import numpy as np
from mesa import Model
from mesa.discrete_space import CellAgent, OrthogonalMooreGrid

EMPTY, OBSTACLE, FOOD, HOME = 0, 1, 2, 3
TO_FOOD, TO_HOME = "to_food", "to_home"
LOW_PHEROMONE = 1e-14
NO_STEP = 255

# `dx` outer, `dy` inner. A tie between two equally good neighbours is broken from the visit order,
# so this cannot be reordered without changing where the ants go.
MOORE_COLUMN_MAJOR = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]


def encode_step(dx, dy):
    return (dx + 1) * 3 + (dy + 1)


def decode_step(code):
    return code // 3 - 1, code % 3 - 1


def build_sites(width, height):
    """Nest, food source and the two obstacle blobs, placed proportionally.

    At 200 by 200 this is where the model Henad descends from hard-codes them.
    """
    sites = np.zeros((width, height), dtype=np.uint8)
    size = 0.407 * (200.0 / width)

    def blob(x, y, cx, cy):
        a = ((x - cx) + (y - cy)) * size
        b = ((x - cx) - (y - cy)) * size
        return a * a / 36.0 + b * b / 1024.0 <= 1.0

    for x in range(width):
        for y in range(height):
            if blob(x, y, 0.500 * width, 0.725 * height) or blob(x, y, 0.450 * width, 0.275 * height):
                sites[x, y] = OBSTACLE
    # After the blobs, so neither site is buried.
    sites[int(0.125 * width), int(0.125 * height)] = FOOD
    sites[int(0.875 * width), int(0.875 * height)] = HOME
    return sites


class Ant(CellAgent):
    def __init__(self, model, cell, has_food=0, reward=0.0, last_step=NO_STEP):
        super().__init__(model)
        self.cell = cell
        self.has_food = has_food
        self.reward = reward
        self.last_step = last_step

    def deposit(self):
        """The value this ant lays, largest over its own cell and the eight around it.

        Floored at what the cell already holds, which is why combining with `max` reproduces the
        plain overwrite of the model this descends from.
        """
        model = self.model
        x, y = self.cell.coordinate
        field = model.trail(TO_FOOD if self.has_food else TO_HOME)
        reward = self.reward
        here = field[x, y]
        best = max(here, here * model.cutdown + reward)
        for dx, dy in MOORE_COLUMN_MAJOR:
            nx, ny = x + dx, y + dy
            if not (0 <= nx < model.width and 0 <= ny < model.height):
                continue
            cut = model.diagonal if dx and dy else model.cutdown
            best = max(best, field[nx, ny] * cut + reward)
        model.record_deposit(x, y, TO_FOOD if self.has_food else TO_HOME, best)

    def advect(self):
        model = self.model
        x, y = self.cell.coordinate
        # Ants follow the trip they are not currently making.
        trail = model.trail(TO_HOME if self.has_food else TO_FOOD)

        best = -1.0
        target = (x, y)
        # 2 rather than 1 is the reference's off-by-one, which gives the first neighbour visited
        # twice the chance of the rest. Reproduced so the ports stay the same simulation.
        count = 2
        for dx, dy in MOORE_COLUMN_MAJOR:
            nx, ny = x + dx, y + dy
            if not model.passable(nx, ny):
                continue
            value = trail[nx, ny]
            if value > best:
                count = 2
            if value > best or (value == best and self.random.random() < 1.0 / count):
                best, target = value, (nx, ny)
            count += 1

        if best == 0.0 and self.last_step != NO_STEP:
            if self.random.random() < model.momentum:
                dx, dy = decode_step(self.last_step)
                if model.passable(x + dx, y + dy):
                    target = (x + dx, y + dy)
        elif self.random.random() < model.random_action:
            dx = self.random.randint(-1, 1)
            dy = self.random.randint(-1, 1)
            if (dx or dy) and model.passable(x + dx, y + dy):
                target = (x + dx, y + dy)

        self.last_step = encode_step(target[0] - x, target[1] - y)
        # The deposit pass spent whatever the ant was carrying; only a site grants more.
        self.reward = 0.0
        site = model.sites[target]
        if site == HOME and self.has_food:
            self.reward, self.has_food = model.reward, 0
            model.deliveries += 1
        elif site == FOOD and not self.has_food:
            self.reward, self.has_food = model.reward, 1
        self.cell = model.grid[target]


class Ants(Model):
    def __init__(
        self,
        num_agents=2000,
        world_width=200.0,
        world_height=200.0,
        update_cutdown=0.9,
        reward=1.0,
        momentum=0.8,
        random_action=0.1,
        evaporation=0.999,
        agents=None,
        field=None,
        rng=None,
    ):
        """`agents` and `field` fix the starting state for a gate scenario; otherwise every ant
        starts on the nest holding one reward, as Henad does."""
        super().__init__(rng=rng)
        self.width = max(int(world_width), 1)
        self.height = max(int(world_height), 1)
        self.cutdown = update_cutdown
        self.diagonal = update_cutdown**math.sqrt(2.0)
        self.reward = reward
        self.momentum = momentum
        self.random_action = random_action
        self.evaporation = evaporation
        self.deliveries = 0

        self.grid = OrthogonalMooreGrid((self.width, self.height), capacity=None, random=self.random, torus=False)
        self.sites = build_sites(self.width, self.height)
        self.layers = {
            name: self.grid.create_property_layer(name, default_value=0.0) for name in (TO_FOOD, TO_HOME)
        }
        if field is not None:
            for name, values in field.items():
                self.layers[name].data[:] = values
        self._deposits = {name: {} for name in (TO_FOOD, TO_HOME)}

        if agents is None:
            nest = (int(0.875 * self.width), int(0.875 * self.height))
            agents = [(nest[0], nest[1], NO_STEP, 0, reward)] * num_agents
        for x, y, last_step, has_food, carried in agents:
            Ant(self, self.grid[(x, y)], has_food=has_food, reward=carried, last_step=last_step)

    def trail(self, name):
        return self.layers[name].data

    def passable(self, x, y):
        return 0 <= x < self.width and 0 <= y < self.height and self.sites[x, y] != OBSTACLE

    def record_deposit(self, x, y, name, value):
        lane = self._deposits[name]
        key = (x, y)
        if value > lane.get(key, 0.0):
            lane[key] = value

    def step(self):
        for lane in self._deposits.values():
            lane.clear()
        self.agents.do("deposit")
        self.agents.do("advect")
        for name, layer in self.layers.items():
            data = layer.data
            for (x, y), value in self._deposits[name].items():
                if value > data[x, y]:
                    data[x, y] = value
            data *= self.evaporation
            data[data < LOW_PHEROMONE] = 0.0

    def population(self):
        return len(self.agents)

    def rows(self):
        """`x y last_step has_food reward` per agent, in creation order."""
        return [(*a.cell.coordinate, a.last_step, a.has_food, a.reward) for a in self.agents]
