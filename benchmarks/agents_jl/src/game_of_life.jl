"""Conway's Game of Life, B3/S23 on a Moore torus, updated synchronously.

One cell agent per site on a `GridSpaceSingle`, which is the shape Agents.jl's own cellular
automaton examples use. The tick is a `model_step!` in two passes, so a cell never sees a neighbour
that has already moved on. Agents.jl calls that its advanced stepping form, and giving no
`agent_step!` is what keeps the scheduler out of the loop.
"""
module GameOfLife

using Agents
using Random: Xoshiro

const DEAD = 0
const ALIVE = 1

@agent struct Cell(GridAgent{2})
    state::Int
    next::Int
end

function tick!(model)
    for a in allagents(model)
        alive = 0
        for n in nearby_agents(a, model, 1)
            alive += n.state
        end
        a.next = if a.state == ALIVE
            (alive == 2 || alive == 3) ? ALIVE : DEAD
        else
            alive == 3 ? ALIVE : DEAD
        end
    end
    for a in allagents(model)
        a.state = a.next
    end
    return
end

"""`live` places an exact set of `(x, y)` cells, for a gate scenario, otherwise `density` fills at
random. Agents.jl counts positions from 1 where Henad counts from 0, so the offsets shift by one."""
function build(width, height; density = 0.3, live = nothing, seed = 1)
    space = GridSpaceSingle((width, height); periodic = true, metric = :chebyshev)
    model = StandardABM(Cell, space; model_step! = tick!, container = Vector, rng = Xoshiro(seed))
    alive = live === nothing ? nothing : Set(live)
    rng = abmrng(model)
    for y in 1:height, x in 1:width
        state = if alive === nothing
            rand(rng) < density ? ALIVE : DEAD
        else
            (x - 1, y - 1) in alive ? ALIVE : DEAD
        end
        add_agent!((x, y), Cell, model; state, next = DEAD)
    end
    return model
end

"""Rows ascending in y, each row ascending in x, as the fixture format wants."""
function bitmap(model)
    width, height = spacesize(model)
    grid = zeros(Int, width, height)
    for a in allagents(model)
        grid[a.pos[1], a.pos[2]] = a.state
    end
    return [join(grid[x, y] for x in 1:width) for y in 1:height]
end

end
