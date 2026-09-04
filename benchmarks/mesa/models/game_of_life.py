"""Conway's Game of Life, B3/S23 on a Moore torus, updated synchronously.

Follows the shape of Mesa's own `conways_game_of_life` example: one cell agent per site, and a
step in two passes so a cell never sees a neighbour that has already moved on.
"""

from mesa import Model
from mesa.discrete_space import FixedAgent, OrthogonalMooreGrid

DEAD = 0
ALIVE = 1


class Cell(FixedAgent):
    def __init__(self, model, cell, state=DEAD):
        super().__init__(model)
        self.cell = cell
        self.state = state
        self._next_state = state

    def determine_state(self):
        alive = sum(neighbor.state for neighbor in self.cell.neighborhood.agents)
        if self.state == ALIVE:
            self._next_state = ALIVE if alive in (2, 3) else DEAD
        else:
            self._next_state = ALIVE if alive == 3 else DEAD

    def assume_state(self):
        self.state = self._next_state


class GameOfLife(Model):
    def __init__(self, width=1024, height=1024, density=0.3, live=None, rng=None):
        """`live` places an exact set of `(x, y)` cells, for a gate scenario; otherwise `density`
        fills at random."""
        super().__init__(rng=rng)
        self.grid = OrthogonalMooreGrid((width, height), capacity=1, random=self.random, torus=True)
        live = set(live) if live is not None else None
        for cell in self.grid.all_cells:
            if live is None:
                state = ALIVE if self.random.random() < density else DEAD
            else:
                state = ALIVE if cell.coordinate in live else DEAD
            Cell(self, cell, state)

    def step(self):
        self.agents.do("determine_state")
        self.agents.do("assume_state")

    def population(self):
        return self.grid.width * self.grid.height

    def bitmap(self):
        """Rows ascending in y, each row ascending in x, as the fixture format wants."""
        by_coordinate = {agent.cell.coordinate: agent.state for agent in self.agents}
        return [
            "".join(str(by_coordinate[(x, y)]) for x in range(self.grid.width))
            for y in range(self.grid.height)
        ]
