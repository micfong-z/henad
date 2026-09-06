"""Boids on a continuous torus, updated synchronously.

Henad's rule rather than Reynolds's, and rather than the one Agents.jl's own flocking example
implements: separation accumulates the offsets to everything inside the protected range, alignment
and cohesion average over everything inside the visual range, and the resulting velocity is clamped
into a speed band. Velocity is the displacement, so a tick is `move_agent!` with `dt` of 1.

`ContinuousSpace` supplies the neighbour query and the shortest offset round the torus. That query
is a candidate set only. Henad's two range tests are strict, and the search has a blind spot of its
own, so the loop asks for a wider radius and then filters exactly. See `query_range`.
"""
module Boids

using Agents
using Random: Xoshiro

@agent struct Boid(ContinuousAgent{2, Float64})
    nvel::SVector{2, Float64}
end

struct Rules
    query_range::Float64
    visual_sq::Float64
    protected_sq::Float64
    separation::Float64
    alignment::Float64
    cohesion::Float64
    max_speed::Float64
    min_speed::Float64
end

function tick!(model)
    r = abmproperties(model)
    for a in allagents(model)
        close_x = 0.0
        close_y = 0.0
        summed_vx = 0.0
        summed_vy = 0.0
        summed_dx = 0.0
        summed_dy = 0.0
        seen = 0
        for id in nearby_ids(a, model, r.query_range)
            other = model[id]
            d = get_direction(a.pos, other.pos, model)
            distance_sq = d[1] * d[1] + d[2] * d[2]
            if distance_sq < r.protected_sq
                close_x -= d[1]
                close_y -= d[2]
            end
            if distance_sq < r.visual_sq
                summed_vx += other.vel[1]
                summed_vy += other.vel[2]
                summed_dx += d[1]
                summed_dy += d[2]
                seen += 1
            end
        end

        vx = a.vel[1] + close_x * r.separation
        vy = a.vel[2] + close_y * r.separation
        if seen > 0
            vx += (summed_vx / seen - a.vel[1]) * r.alignment + (summed_dx / seen) * r.cohesion
            vy += (summed_vy / seen - a.vel[2]) * r.alignment + (summed_dy / seen) * r.cohesion
        end

        speed = hypot(vx, vy)
        if speed == 0.0
            vx, vy = r.min_speed, 0.0
        elseif speed > r.max_speed
            vx, vy = vx / speed * r.max_speed, vy / speed * r.max_speed
        elseif speed < r.min_speed
            vx, vy = vx / speed * r.min_speed, vy / speed * r.min_speed
        end
        a.nvel = SVector(vx, vy)
    end

    for a in allagents(model)
        a.vel = a.nvel
        # A periodic `move_agent!` normalizes with `mod`, which is Henad's `(pos + v) mod world`.
        move_agent!(a, model, 1.0)
    end
    return
end

"""Radius to ask the neighbour grid for.

The approximate search widens its cell radius by how far the searching agent sits from its own
cell centre, but not by how far a neighbour sits from its. It can therefore drop an agent that is
genuinely in range. `search = :exact` filters the same candidate set and drops it too. Half a cell
diagonal covers the gap, and the exact tests inside the loop discard the surplus."""
query_range(visual_range, spacing) = visual_range + spacing * sqrt(2.0) / 2.0

"""Neighbour-grid cell size near `target`, which `ContinuousSpace` needs the extent to divide
exactly. Searched outwards from the ideal count rather than assumed: a world sized to hold a
population at constant density has an irrational side, and `side / n` does not always round-trip."""
function grid_spacing(width, height, target)
    side = min(width, height)
    want = max(1, round(Int, side / target))
    for offset in 0:want, cells in (want + offset, want - offset)
        cells < 1 && continue
        spacing = side / cells
        if width / spacing == floor(width / spacing) && height / spacing == floor(height / spacing)
            return spacing
        end
    end
    # Reachable only for a rectangular world whose sides share no such divisor. Saying so beats
    # returning a spacing that was never checked and letting ContinuousSpace abort on it.
    error("no neighbour-grid spacing near $target divides a $(width) by $(height) world exactly")
end

"""`agents` places an exact `(x, y, vx, vy)` list, for a gate scenario, otherwise the population is
scattered at random with a fixed speed and a random heading, as Henad does."""
function build(;
        num_agents = 50_000, world_width = 1000.0, world_height = 1000.0,
        visual_range = 50.0, protected_range = 8.0,
        separation = 0.05, alignment = 0.05, cohesion = 0.0005,
        max_speed = 15.0, min_speed = 3.0,
        agents = nothing, seed = 1,
    )
    # Agents.jl says to benchmark the neighbour grid's cell size rather than guess one. A sweep
    # over the choices bottoms out here, well under the visual range and well under the ratio its
    # flocking example uses.
    spacing = grid_spacing(world_width, world_height, visual_range / 6.0)
    rules = Rules(
        query_range(visual_range, spacing), visual_range * visual_range,
        protected_range * protected_range,
        separation, alignment, cohesion, max_speed, min_speed,
    )
    space = ContinuousSpace((world_width, world_height); periodic = true, spacing)
    model = StandardABM(
        Boid, space;
        model_step! = tick!, container = Vector, rng = Xoshiro(seed), properties = rules,
    )

    if agents === nothing
        rng = abmrng(model)
        speed = 0.5 * (min_speed + max_speed)
        agents = Vector{NTuple{4, Float64}}(undef, num_agents)
        for i in 1:num_agents
            angle = rand(rng) * 2.0 * pi
            agents[i] = (
                rand(rng) * world_width, rand(rng) * world_height,
                cos(angle) * speed, sin(angle) * speed,
            )
        end
    end
    zero2 = SVector(0.0, 0.0)
    for (x, y, vx, vy) in agents
        add_agent!(SVector(x, y), Boid, model; vel = SVector(vx, vy), nvel = zero2)
    end
    return model
end

"""`x y vx vy` per agent, in creation order, as the fixture format wants."""
rows(model) = [(a.pos[1], a.pos[2], a.vel[1], a.vel[2]) for a in allagents(model)]

end
