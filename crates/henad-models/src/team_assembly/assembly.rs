//! Team assembly, the global pass of Team Assembly, and the initial teams.

use std::f32::consts::TAU;

use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::network_model::Nodes;
use henad_core::authoring::primitives::rng::{next_float, next_index};
use henad_core::network::Network;

use crate::team_assembly::ring::{due_key, retirement_key};
use crate::team_assembly::{
    INCUMBENT_INCUMBENT, NEWCOMER_INCUMBENT, NEWCOMER_NEWCOMER, REPEAT, TeamAssembly, TeamAux, TeamParams,
};

/// Number of uniform draws from the live nodes before the ones outside the team are listed instead.
const IDLE_TRIES: u32 = 64;

/// Radius of the polygon a team is placed on, as a fraction of the world's shorter side.
///
/// NetLogo's setup places the first team 1.75 patches from the origin in a world 101 patches wide.
const PLACE_RADIUS: f32 = 1.75 / 101.0;

/// Joins the initial nodes into teams of `team_size`, each a clique on a small polygon.
///
/// NetLogo's setup builds one team of `team_size` nodes, and its members count as incumbents,
/// so their links are incumbent-incumbent. Here the engine's node count is the initial population,
/// grouped into teams the same way, and the teams sit on a grid of cells across the world.
/// A node count equal to `team_size` gives NetLogo's setup.
/// Note that with fewer nodes than `team_size`, the first teams take newcomers once the setup nodes are used up.
pub(super) fn setup(nodes: &mut Nodes<'_, TeamAssembly>, extent: Extent, team_size: u32) {
    let n = nodes.graph.slot_count() as u32;
    let size = team_size.max(1);
    let teams = n.div_ceil(size);
    let cols = (teams as f32).sqrt().ceil().max(1.0) as u32;
    let rows = teams.div_ceil(cols).max(1);
    let (cell_w, cell_h) = (extent.w / cols as f32, extent.h / rows as f32);
    let radius = place_radius(extent, n as usize);

    for team in 0..teams {
        let members = team * size..(team * size + size).min(n);
        let center = (
            ((team % cols) as f32 + 0.5) * cell_w,
            ((team / cols) as f32 + 0.5) * cell_h,
        );
        for (k, i) in members.clone().enumerate() {
            let (x, y) = on_polygon(center, radius, k, members.len(), extent);
            nodes.lanes.pos_x[i as usize] = x;
            nodes.lanes.pos_y[i as usize] = y;
        }
        for a in members.clone() {
            for b in a + 1..members.end {
                nodes.graph.add_edge(a, b, INCUMBENT_INCUMBENT);
            }
        }
    }
}

// --8<-- [start:assemble]
/// Assembles one team, links its members and retires the nodes that have been idle for too long.
///
/// This follows NetLogo's `go`. Each member is a newcomer with probability `1 - p`.
/// Otherwise it is an incumbent. With probability `q` it is a previous collaborator of the team so far,
/// if there is one, and any incumbent outside the team if not.
pub(super) fn assemble(
    nodes: &mut Nodes<'_, TeamAssembly>,
    params: &TeamParams,
    extent: Extent,
    rng: &mut u64,
    tick: u64,
) {
    let now = tick + 1;
    // Listed here rather than in setup, so nodes a `from_graph` seed spawned or retired are counted too.
    if !nodes.aux.live_listed {
        nodes.aux.live.rebuild(nodes.graph);
        nodes.aux.live_listed = true;
    }
    debug_assert_eq!(
        nodes.aux.live.as_slice().len(),
        nodes.graph.node_count(),
        "the live list has drifted from the graph"
    );
    {
        let TeamAux { ring, live, .. } = &mut *nodes.aux;
        ring.prepare(params.max_downtime, live.as_slice(), &nodes.lanes.team_tick);
    }

    let mut team = std::mem::take(&mut nodes.aux.team);
    team.clear();
    nodes.aux.candidates.clear();
    nodes.aux.scanned = 0;
    for _ in 0..params.team_size {
        let incumbent = if next_float(rng, 1.0) >= params.incumbent_chance {
            None
        } else if next_float(rng, 1.0) < params.collaborator_chance && collect_collaborators(nodes, &team, now) {
            let candidates = &nodes.aux.candidates;
            Some(candidates[next_index(rng, candidates.len() as u32) as usize])
        } else {
            idle_incumbent(nodes, now, rng)
        };

        // With no incumbent outside the team, NetLogo stops with an error. A newcomer joins instead.
        let member = if let Some(i) = incumbent {
            nodes.aux.ring.remove(i);
            i
        } else {
            let i = nodes.spawn();
            nodes.lanes.spawn_tick[i as usize] = now;
            nodes.aux.live.insert(i);
            i
        };
        nodes.lanes.team_tick[member as usize] = now;
        nodes.aux.ring.insert(member, now);
        team.push(member);
    }

    place_newcomers(nodes, &team, extent, now);
    tie(nodes.graph, &nodes.lanes.spawn_tick, &team, now);
    retire_idle(nodes, params.max_downtime, now);
    nodes.aux.team = team;
}
// --8<-- [end:assemble]

/// Lists every node outside the team that is linked to a member, and returns whether there are any.
///
/// The list is built up over the tick. Only the rows of members added since the last call are read,
/// and nodes that have since joined the team are dropped. A node's `mark` holds the tick it was listed at,
/// so it is listed once however many members it is linked to.
fn collect_collaborators(nodes: &mut Nodes<'_, TeamAssembly>, team: &[u32], now: u64) -> bool {
    let Nodes { lanes, graph, aux } = nodes;
    for &i in &team[aux.scanned..] {
        for &j in graph.in_neighbors(i) {
            let j_ = j as usize;
            if lanes.team_tick[j_] != now && lanes.mark[j_] != now {
                lanes.mark[j_] = now;
                aux.candidates.push(j);
            }
        }
    }
    aux.scanned = team.len();
    aux.candidates.retain(|&j| lanes.team_tick[j as usize] != now);
    !aux.candidates.is_empty()
}

/// Picks a live node outside the team uniformly at random, or returns `None` if there is none.
///
/// Live nodes are drawn uniformly until one is outside the team.
/// If every draw lands in the team, the nodes outside it are listed and one is drawn from the list.
/// Either way every node outside the team is equally likely.
fn idle_incumbent(nodes: &mut Nodes<'_, TeamAssembly>, now: u64, rng: &mut u64) -> Option<u32> {
    let TeamAux { live, eligible, .. } = &mut *nodes.aux;
    let team_tick = &nodes.lanes.team_tick;
    let live = live.as_slice();
    let n = live.len() as u32;
    if n == 0 {
        return None;
    }
    for _ in 0..IDLE_TRIES {
        let i = live[next_index(rng, n) as usize];
        if team_tick[i as usize] != now {
            return Some(i);
        }
    }
    eligible.clear();
    eligible.extend(live.iter().copied().filter(|&i| team_tick[i as usize] != now));
    let n = eligible.len() as u32;
    (n > 0).then(|| eligible[next_index(rng, n) as usize])
}

/// Places the team's newcomers on a small polygon around its first incumbent, or around the world's center if it has
/// none.
///
/// NetLogo creates newcomers at the origin and leaves the rest to its layout. Positions are only used for drawing.
fn place_newcomers(nodes: &mut Nodes<'_, TeamAssembly>, team: &[u32], extent: Extent, now: u64) {
    let lanes = &mut *nodes.lanes;
    let anchor = team
        .iter()
        .find(|&&i| lanes.spawn_tick[i as usize] != now)
        .map_or((0.5 * extent.w, 0.5 * extent.h), |&i| {
            (lanes.pos_x[i as usize], lanes.pos_y[i as usize])
        });
    let radius = place_radius(extent, nodes.graph.node_count());
    for (k, &i) in team.iter().enumerate() {
        if lanes.spawn_tick[i as usize] == now {
            let (x, y) = on_polygon(anchor, radius, k, team.len(), extent);
            lanes.pos_x[i as usize] = x;
            lanes.pos_y[i as usize] = y;
        }
    }
}

/// Links every pair of team members.
///
/// A pair that is already linked has collaborated before, so its link turns red. A new link is coloured by how many of
/// its ends are incumbents, as NetLogo's `color-collaborations` does.
fn tie(graph: &mut Network, spawn_tick: &[u64], team: &[u32], now: u64) {
    for (k, &a) in team.iter().enumerate() {
        for &b in &team[k + 1..] {
            if let Some(e) = graph.edge_between(a, b) {
                graph.set_edge_color(e, REPEAT);
            } else {
                let incumbents = [a, b].iter().filter(|&&i| spawn_tick[i as usize] != now).count();
                let color = [NEWCOMER_NEWCOMER, NEWCOMER_INCUMBENT, INCUMBENT_INCUMBENT][incumbents];
                graph.add_edge(a, b, color);
            }
        }
    }
}

/// Retires the nodes that have been idle for more than `max_downtime` ticks, in index order.
fn retire_idle(nodes: &mut Nodes<'_, TeamAssembly>, max_downtime: u32, now: u64) {
    let Some(due) = due_key(now, max_downtime) else {
        return;
    };
    let mut retiring = std::mem::take(&mut nodes.aux.retiring);
    retiring.clear();
    nodes.aux.ring.drain_through(due, &mut retiring);
    retiring.sort_unstable();
    for &i in &retiring {
        debug_assert!(
            now - retirement_key(nodes.lanes.team_tick[i as usize]) > u64::from(max_downtime),
            "node {i} was drained before it was due"
        );
        nodes.aux.live.remove(i);
        nodes.retire(i);
    }
    nodes.aux.retiring = retiring;
}

/// Returns the radius of the polygon a team is placed on.
///
/// This is NetLogo's radius scaled to the world, and at most half the mean spacing between nodes.
fn place_radius(extent: Extent, nodes: usize) -> f32 {
    let spacing = (extent.w * extent.h / nodes.max(1) as f32).sqrt();
    (PLACE_RADIUS * extent.w.min(extent.h)).min(0.5 * spacing)
}

/// Returns corner `k` of a regular polygon with `sides` corners around `center`, kept inside the world.
fn on_polygon(center: (f32, f32), radius: f32, k: usize, sides: usize, extent: Extent) -> (f32, f32) {
    let angle = TAU * k as f32 / sides.max(1) as f32;
    (
        (center.0 + radius * angle.sin()).clamp(0.0, extent.w),
        (center.1 - radius * angle.cos()).clamp(0.0, extent.h),
    )
}

#[cfg(test)]
mod tests {
    use super::{collect_collaborators, idle_incumbent};
    use crate::team_assembly::{TeamAssembly, TeamAux, TeamLanes};
    use henad_core::authoring::model::agent_model::AgentLanes as _;
    use henad_core::authoring::model::network_model::Nodes;
    use henad_core::network::Network;

    const NOW: u64 = 5;

    /// Lanes, a graph and model state for `n` live nodes, with `members` already in this tick's team.
    fn world(n: usize, members: &[u32]) -> (TeamLanes, Network, TeamAux) {
        let mut lanes = TeamLanes::alloc(n);
        for &i in members {
            lanes.team_tick[i as usize] = NOW;
        }
        let mut aux = TeamAux::default();
        for i in 0..n as u32 {
            aux.live.insert(i);
        }
        (lanes, Network::new(n, false), aux)
    }

    /// A node linked to several members is listed once, as NetLogo's `one-of turtles with [...]` counts it once.
    #[test]
    fn a_collaborator_linked_to_several_members_is_listed_once() {
        let (a, b, x, y, z) = (0, 1, 2, 3, 4);
        let (mut lanes, mut graph, mut aux) = world(5, &[a, b]);
        graph.add_edge(a, x, 0);
        graph.add_edge(b, x, 0);
        graph.add_edge(a, y, 0);
        graph.add_edge(a, b, 0);
        let mut nodes = Nodes::<TeamAssembly> {
            lanes: &mut lanes,
            graph: &mut graph,
            aux: &mut aux,
        };

        assert!(collect_collaborators(&mut nodes, &[a, b], NOW));
        let mut listed = nodes.aux.candidates.clone();
        listed.sort_unstable();
        assert_eq!(listed, [x, y], "a member was listed, or x was listed twice");

        // y joins, and z is linked to it. The list drops y, keeps x, and gains z.
        nodes.graph.add_edge(y, z, 0);
        nodes.lanes.team_tick[y as usize] = NOW;
        assert!(collect_collaborators(&mut nodes, &[a, b, y], NOW));
        assert_eq!(nodes.aux.candidates, [x, z]);
    }

    /// Every node outside the team is drawn about equally often, and no member ever is.
    #[test]
    fn an_idle_incumbent_is_drawn_uniformly_from_outside_the_team() {
        let members = [1, 4, 6];
        let (mut lanes, mut graph, mut aux) = world(12, &members);
        let mut nodes = Nodes::<TeamAssembly> {
            lanes: &mut lanes,
            graph: &mut graph,
            aux: &mut aux,
        };
        let mut rng = 0x1D1E_5EED_u64;
        let mut counts = [0u32; 12];
        const DRAWS: u32 = 90_000;
        for _ in 0..DRAWS {
            let i = idle_incumbent(&mut nodes, NOW, &mut rng).expect("nine nodes are outside the team");
            counts[i as usize] += 1;
        }
        for m in members {
            assert_eq!(counts[m as usize], 0, "member {m} was drawn");
        }
        // Nine equally likely outcomes. A 5 sigma band on each count.
        let expected = f64::from(DRAWS) / 9.0;
        let sigma = (expected * (1.0 - 1.0 / 9.0)).sqrt();
        for (i, &c) in counts.iter().enumerate() {
            if !members.contains(&(i as u32)) {
                assert!(
                    (f64::from(c) - expected).abs() < 5.0 * sigma,
                    "node {i} drawn {c} times, expected {expected}"
                );
            }
        }
    }

    /// With almost every live node in the team, the draws nearly always miss and the listing takes over.
    /// It still draws every node outside the team about equally often.
    #[test]
    fn the_listing_draws_uniformly_from_the_few_nodes_outside_the_team() {
        const OUTSIDE: [u32; 4] = [7, 512, 1_234, 1_999];
        let members: Vec<u32> = (0..2_000).filter(|i| !OUTSIDE.contains(i)).collect();
        let (mut lanes, mut graph, mut aux) = world(2_000, &members);
        let mut nodes = Nodes::<TeamAssembly> {
            lanes: &mut lanes,
            graph: &mut graph,
            aux: &mut aux,
        };
        let mut rng = 0x5CA7_u64;
        let mut counts = [0u32; 4];
        const DRAWS: u32 = 4_000;
        for _ in 0..DRAWS {
            let i = idle_incumbent(&mut nodes, NOW, &mut rng).expect("four nodes are outside the team");
            let at = OUTSIDE.iter().position(|&o| o == i).expect("a member was drawn");
            counts[at] += 1;
        }
        assert_eq!(
            nodes.aux.eligible.len(),
            4,
            "the listing never ran, so this proved little"
        );
        let expected = f64::from(DRAWS) / 4.0;
        let sigma = (expected * 0.75).sqrt();
        for (&i, &c) in OUTSIDE.iter().zip(&counts) {
            assert!(
                (f64::from(c) - expected).abs() < 5.0 * sigma,
                "node {i} drawn {c} times, expected {expected}"
            );
        }

        for i in OUTSIDE {
            nodes.lanes.team_tick[i as usize] = NOW;
        }
        assert_eq!(idle_incumbent(&mut nodes, NOW, &mut rng), None);
    }
}
