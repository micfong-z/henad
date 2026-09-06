"""SIR on a Moore torus, updated synchronously.

A susceptible cell with `k` infected neighbours catches it with probability `1 - (1 - beta)^k`, an
infected cell recovers with probability `gamma`, and recovery is permanent. Same two-pass shape as
Game of Life, so no cell sees a neighbour that has already changed this tick.
"""
module Sir

using Agents
using Random: Xoshiro

const SUSCEPTIBLE = 0
const INFECTED = 1
const RECOVERED = 2

@agent struct Patch(GridAgent{2})
    state::Int
    next::Int
end

struct Rates
    infection_rate::Float64
    recovery_rate::Float64
end

function tick!(model)
    rates = abmproperties(model)
    rng = abmrng(model)
    for a in allagents(model)
        if a.state == SUSCEPTIBLE
            infected = 0
            for n in nearby_agents(a, model, 1)
                infected += n.state == INFECTED
            end
            catches = infected > 0 && rand(rng) < 1.0 - (1.0 - rates.infection_rate)^infected
            a.next = catches ? INFECTED : SUSCEPTIBLE
        elseif a.state == INFECTED
            a.next = rand(rng) < rates.recovery_rate ? RECOVERED : INFECTED
        else
            a.next = RECOVERED
        end
    end
    for a in allagents(model)
        a.state = a.next
    end
    return
end

function build(
        width, height;
        infection_rate = 0.3, recovery_rate = 0.05, initial_infected_pct = 0.01, seed = 1,
    )
    space = GridSpaceSingle((width, height); periodic = true, metric = :chebyshev)
    model = StandardABM(
        Patch, space;
        model_step! = tick!, container = Vector, rng = Xoshiro(seed),
        properties = Rates(infection_rate, recovery_rate),
    )
    rng = abmrng(model)
    for y in 1:height, x in 1:width
        state = rand(rng) < initial_infected_pct ? INFECTED : SUSCEPTIBLE
        add_agent!((x, y), Patch, model; state, next = SUSCEPTIBLE)
    end
    return model
end

"""Cells in each compartment, in the order the fixture's columns run."""
function counts(model)
    totals = [0, 0, 0]
    for a in allagents(model)
        totals[a.state + 1] += 1
    end
    return totals
end

end
