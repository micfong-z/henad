"""SIR on a Moore torus, updated synchronously.

A susceptible cell with `k` infected neighbours catches it with probability `1 - (1 - beta)^k`,
an infected cell recovers with probability `gamma`, and recovery is permanent. Same two-pass shape
as Game of Life, so no cell sees a neighbour that has already changed this tick.
"""

from mesa import Model
from mesa.discrete_space import FixedAgent, OrthogonalMooreGrid

SUSCEPTIBLE = 0
INFECTED = 1
RECOVERED = 2


class Patch(FixedAgent):
    def __init__(self, model, cell, state=SUSCEPTIBLE):
        super().__init__(model)
        self.cell = cell
        self.state = state
        self._next_state = state

    def determine_state(self):
        model = self.model
        if self.state == SUSCEPTIBLE:
            infected = sum(n.state == INFECTED for n in self.cell.neighborhood.agents)
            catches = infected > 0 and self.random.random() < 1.0 - (1.0 - model.infection_rate) ** infected
            self._next_state = INFECTED if catches else SUSCEPTIBLE
        elif self.state == INFECTED:
            self._next_state = RECOVERED if self.random.random() < model.recovery_rate else INFECTED
        else:
            self._next_state = RECOVERED

    def assume_state(self):
        self.state = self._next_state


class Sir(Model):
    def __init__(
        self,
        width=1024,
        height=1024,
        infection_rate=0.3,
        recovery_rate=0.05,
        initial_infected_pct=0.01,
        rng=None,
    ):
        super().__init__(rng=rng)
        self.infection_rate = infection_rate
        self.recovery_rate = recovery_rate
        self.grid = OrthogonalMooreGrid((width, height), capacity=1, random=self.random, torus=True)
        for cell in self.grid.all_cells:
            state = INFECTED if self.random.random() < initial_infected_pct else SUSCEPTIBLE
            Patch(self, cell, state)

    def step(self):
        self.agents.do("determine_state")
        self.agents.do("assume_state")

    def population(self):
        return self.grid.width * self.grid.height

    def counts(self):
        totals = [0, 0, 0]
        for agent in self.agents:
            totals[agent.state] += 1
        return totals
