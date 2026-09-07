"""Ant foraging on a bounded lattice over two pheromone layers.

Ants lay a trail for the trip they are making and follow the trail for the trip they are not, so a
colony that finds food leaves a path back to it. Written from Henad's declaration, which differs
from the MASON and krABMaga models it descends from: deposits combine with `max` rather than
last-writer-wins, and the whole field is read before any of it is written.

`GridSpace` supplies the lattice and carries the ants. The two layers are plain matrices in the
model properties, and both passes are ordinary agent loops. The advect scan walks the eight
neighbours by hand: `nearby_positions` visits them in Agents.jl's own offset order, and the rule
here specifies column-major, which the library does not promise.
"""
module Ants

using Agents
using Random: Xoshiro

const EMPTY = 0x00
const OBSTACLE = 0x01
const FOOD = 0x02
const HOME = 0x03
const LOW_PHEROMONE = 1.0e-14
const NO_STEP = 255

# `dx` outer, `dy` inner. A tie between two equally good neighbours is broken from the visit order,
# so this cannot be reordered without changing where the ants go.
const MOORE_COLUMN_MAJOR = (
    (-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1),
)

encode_step(dx, dy) = (dx + 1) * 3 + (dy + 1)
decode_step(code) = (code ÷ 3 - 1, code % 3 - 1)

@agent struct Ant(GridAgent{2})
    has_food::Int
    reward::Float64
    last_step::Int
end

"""One pheromone layer: the field itself, the deposits landing in it this tick, and the cells those
deposits touched. Collecting the touched cells keeps the merge proportional to the ants rather than
to the lattice."""
struct Layer
    field::Matrix{Float64}
    deposits::Matrix{Float64}
    touched::Vector{Int}
end

Layer(width, height) = Layer(zeros(width, height), zeros(width, height), Int[])

mutable struct Colony
    const width::Int
    const height::Int
    const cutdown::Float64
    # Cutdown raised to the diagonal distance, since those neighbours are further away.
    const diagonal::Float64
    const reward::Float64
    const momentum::Float64
    const random_action::Float64
    const evaporation::Float64
    const sites::Matrix{UInt8}
    const to_food::Layer
    const to_home::Layer
    deliveries::Int
end

passable(c::Colony, x, y) =
    1 <= x <= c.width && 1 <= y <= c.height && c.sites[x, y] != OBSTACLE

"""Nest, food source and the two obstacle blobs, placed proportionally.

At 200 by 200 this is where the model Henad descends from hard-codes them. The formula counts from
0 where the lattice is indexed from 1."""
function build_sites(width, height)
    sites = fill(EMPTY, width, height)
    scale = 0.407 * (200.0 / width)
    blob(x, y, cx, cy) = begin
        a = ((x - cx) + (y - cy)) * scale
        b = ((x - cx) - (y - cy)) * scale
        a * a / 36.0 + b * b / 1024.0 <= 1.0
    end
    for y in 0:(height - 1), x in 0:(width - 1)
        if blob(x, y, 0.5 * width, 0.725 * height) || blob(x, y, 0.45 * width, 0.275 * height)
            sites[x + 1, y + 1] = OBSTACLE
        end
    end
    # After the blobs, so neither site is buried.
    sites[floor(Int, 0.125 * width) + 1, floor(Int, 0.125 * height) + 1] = FOOD
    sites[floor(Int, 0.875 * width) + 1, floor(Int, 0.875 * height) + 1] = HOME
    return sites
end

"""The value an ant lays, largest over its own cell and the eight around it.

Floored at what the cell already holds, which is why combining with `max` reproduces the plain
overwrite of the model this descends from."""
function deposit_pass!(model, c::Colony)
    for a in allagents(model)
        x, y = a.pos
        layer = a.has_food == 1 ? c.to_food : c.to_home
        field = layer.field
        reward = a.reward
        here = field[x, y]
        best = max(here, here * c.cutdown + reward)
        for (dx, dy) in MOORE_COLUMN_MAJOR
            nx, ny = x + dx, y + dy
            (1 <= nx <= c.width && 1 <= ny <= c.height) || continue
            cut = (dx != 0 && dy != 0) ? c.diagonal : c.cutdown
            best = max(best, field[nx, ny] * cut + reward)
        end
        cell = (y - 1) * c.width + x
        if best > layer.deposits[cell]
            layer.deposits[cell] = best
        end
        push!(layer.touched, cell)
    end
    return
end

function advect_pass!(model, c::Colony)
    rng = abmrng(model)
    for a in allagents(model)
        x, y = a.pos
        # Ants follow the trip they are not currently making.
        trail = (a.has_food == 1 ? c.to_home : c.to_food).field

        best = -1.0
        tx, ty = x, y
        # 2 rather than 1 is the reference's off-by-one, which gives the first neighbour visited
        # twice the chance of the rest. Reproduced so the ports stay the same simulation.
        visits = 2
        for (dx, dy) in MOORE_COLUMN_MAJOR
            nx, ny = x + dx, y + dy
            passable(c, nx, ny) || continue
            m = trail[nx, ny]
            if m > best
                visits = 2
            end
            if m > best || (m == best && rand(rng) < 1.0 / visits)
                best = m
                tx, ty = nx, ny
            end
            visits += 1
        end

        if best == 0.0 && a.last_step != NO_STEP
            if rand(rng) < c.momentum
                dx, dy = decode_step(a.last_step)
                if passable(c, x + dx, y + dy)
                    tx, ty = x + dx, y + dy
                end
            end
        elseif rand(rng) < c.random_action
            dx = rand(rng, -1:1)
            dy = rand(rng, -1:1)
            if (dx != 0 || dy != 0) && passable(c, x + dx, y + dy)
                tx, ty = x + dx, y + dy
            end
        end

        a.last_step = encode_step(tx - x, ty - y)
        # The deposit pass spent whatever the ant was carrying, only a site grants more.
        a.reward = 0.0
        site = c.sites[tx, ty]
        if site == HOME && a.has_food == 1
            a.reward, a.has_food = c.reward, 0
            c.deliveries += 1
        elseif site == FOOD && a.has_food == 0
            a.reward, a.has_food = c.reward, 1
        end
        move_agent!(a, (tx, ty), model)
    end
    return
end

function settle!(layer::Layer, evaporation)
    for cell in layer.touched
        value = layer.deposits[cell]
        if value > layer.field[cell]
            layer.field[cell] = value
        end
        layer.deposits[cell] = 0.0
    end
    empty!(layer.touched)
    field = layer.field
    for cell in eachindex(field)
        value = field[cell] * evaporation
        field[cell] = value < LOW_PHEROMONE ? 0.0 : value
    end
    return
end

function tick!(model)
    c = abmproperties(model)
    deposit_pass!(model, c)
    advect_pass!(model, c)
    settle!(c.to_food, c.evaporation)
    settle!(c.to_home, c.evaporation)
    return
end

"""`agents` and `field` fix the starting state for a gate scenario, otherwise every ant starts on
the nest holding one reward, as Henad does."""
function build(;
        num_agents = 2_000, world_width = 200.0, world_height = 200.0,
        update_cutdown = 0.9, reward = 1.0, momentum = 0.8, random_action = 0.1,
        evaporation = 0.999, agents = nothing, field = nothing, seed = 1,
    )
    width = max(floor(Int, world_width), 1)
    height = max(floor(Int, world_height), 1)
    colony = Colony(
        width, height, update_cutdown, update_cutdown^sqrt(2.0), reward,
        momentum, random_action, evaporation,
        build_sites(width, height), Layer(width, height), Layer(width, height), 0,
    )
    if field !== nothing
        colony.to_food.field .= field["to_food"]
        colony.to_home.field .= field["to_home"]
    end

    space = GridSpace((width, height); periodic = false, metric = :chebyshev)
    model = StandardABM(
        Ant, space;
        model_step! = tick!, container = Vector, rng = Xoshiro(seed), properties = colony,
    )
    if agents === nothing
        nest = (floor(Int, 0.875 * width), floor(Int, 0.875 * height))
        agents = [(Float64(nest[1]), Float64(nest[2]), NO_STEP, 0, reward) for _ in 1:num_agents]
    end
    for (x, y, last_step, has_food, carried) in agents
        pos = (round(Int, x) + 1, round(Int, y) + 1)
        add_agent!(pos, Ant, model; has_food, reward = carried, last_step)
    end
    return model
end

"""`x y last_step has_food reward` per agent, in creation order, counting from 0 as Henad does."""
rows(model) = [
    (a.pos[1] - 1, a.pos[2] - 1, a.last_step, a.has_food, a.reward) for a in allagents(model)
]

function trail(model, name)
    c = abmproperties(model)
    return (name == "to_food" ? c.to_food : c.to_home).field
end

end
