//! Boids on a continuous torus, updated synchronously.
//!
//! Henad's rule rather than Reynolds's, and rather than the one krABMaga's own `flockers` example
//! implements. Separation accumulates the offsets to everything inside the protected range,
//! alignment and cohesion average over everything inside the visual range, and the resulting
//! velocity is clamped into a speed band. Velocity is the displacement, so a tick is one unit of
//! time.
//!
//! Shape follows `flockers`: one agent per bird holding its own position and velocity, reading the
//! `Field2D` read buffer and re-registering itself with `set_object_location`. Nothing an agent
//! writes is visible until `Schedule::step` swaps the buffers, which is what makes the tick
//! synchronous.

use core::fmt;
use std::any::Any;
use std::hash::{Hash, Hasher};

use krabmaga::engine::agent::Agent;
use krabmaga::engine::fields::field::Field;
use krabmaga::engine::fields::field_2d::{toroidal_transform, Field2D, Location2D};
use krabmaga::engine::location::Real2D;
use krabmaga::engine::schedule::Schedule;
use krabmaga::engine::state::State;
use krabmaga::rand::{Rng, SeedableRng};
use krabmaga::rand_pcg::Pcg64;

#[derive(Clone, Copy)]
pub struct Boid {
    pub id: u32,
    pub loc: Real2D,
    pub vel: Real2D,
}

impl Location2D<Real2D> for Boid {
    fn get_location(self) -> Real2D {
        self.loc
    }

    fn set_location(&mut self, loc: Real2D) {
        self.loc = loc;
    }
}

impl Hash for Boid {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.id.hash(hasher);
    }
}

impl Eq for Boid {}

impl PartialEq for Boid {
    fn eq(&self, other: &Boid) -> bool {
        self.id == other.id
    }
}

impl fmt::Display for Boid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} loc {}", self.id, self.loc)
    }
}

impl Agent for Boid {
    fn step(&mut self, state: &mut dyn State) {
        let model = state.as_any().downcast_ref::<Boids>().expect("boids state");

        let mut close = (0.0f32, 0.0f32);
        let mut summed_velocity = (0.0f32, 0.0f32);
        let mut summed_offset = (0.0f32, 0.0f32);
        let mut seen = 0u32;

        let (shifts, count) = model.seam_shifts(self.loc);
        for &(sx, sy) in &shifts[..count] {
            let probe = Real2D {
                x: self.loc.x + sx,
                y: self.loc.y + sy,
            };
            for other in model
                .field
                .get_neighbors_within_relax_distance(probe, model.visual_range)
            {
                if other.id == self.id {
                    continue;
                }
                // Offset from the probe, which is the wrapped offset from this boid. Only one
                // probe can land a given neighbour inside the visual range, see `seam_shifts`.
                let dx = other.loc.x - probe.x;
                let dy = other.loc.y - probe.y;
                let dist_sq = dx * dx + dy * dy;

                if dist_sq < model.protected_sq {
                    close.0 -= dx;
                    close.1 -= dy;
                }
                if dist_sq < model.visual_sq {
                    summed_velocity.0 += other.vel.x;
                    summed_velocity.1 += other.vel.y;
                    summed_offset.0 += dx;
                    summed_offset.1 += dy;
                    seen += 1;
                }
            }
        }

        let mut vx = self.vel.x + close.0 * model.separation;
        let mut vy = self.vel.y + close.1 * model.separation;
        if seen > 0 {
            let inverse = 1.0 / seen as f32;
            vx += (summed_velocity.0 * inverse - self.vel.x) * model.alignment
                + summed_offset.0 * inverse * model.cohesion;
            vy += (summed_velocity.1 * inverse - self.vel.y) * model.alignment
                + summed_offset.1 * inverse * model.cohesion;
        }

        let speed_sq = vx * vx + vy * vy;
        if speed_sq > 0.0 {
            let speed = speed_sq.sqrt();
            if speed > model.max_speed {
                vx = vx / speed * model.max_speed;
                vy = vy / speed * model.max_speed;
            } else if speed < model.min_speed {
                vx = vx / speed * model.min_speed;
                vy = vy / speed * model.min_speed;
            }
        } else {
            vx = model.min_speed;
            vy = 0.0;
        }

        self.vel = Real2D { x: vx, y: vy };
        self.loc = Real2D {
            x: toroidal_transform(self.loc.x + vx, model.world_w),
            y: toroidal_transform(self.loc.y + vy, model.world_h),
        };
        model.field.set_object_location(*self, self.loc);
    }
}

pub struct Boids {
    pub world_w: f32,
    pub world_h: f32,
    visual_range: f32,
    visual_sq: f32,
    protected_sq: f32,
    separation: f32,
    alignment: f32,
    cohesion: f32,
    max_speed: f32,
    min_speed: f32,
    field: Field2D<Boid>,
    num_agents: u32,
    /// An exact `(x, y, vx, vy)` list for a gate scenario, in place of the random scatter.
    agents: Option<Vec<(f32, f32, f32, f32)>>,
    seed: u64,
}

#[allow(clippy::too_many_arguments)]
impl Boids {
    pub fn new(
        num_agents: u32,
        world_w: f32,
        world_h: f32,
        visual_range: f32,
        protected_range: f32,
        separation: f32,
        alignment: f32,
        cohesion: f32,
        max_speed: f32,
        min_speed: f32,
        seed: u64,
    ) -> Result<Self, String> {
        if visual_range <= 0.0 {
            return Err("visual_range must be positive".to_owned());
        }
        // The seam probes below assume a neighbourhood narrower than half the world. See
        // `seam_shifts`.
        if 2.0 * visual_range > world_w.min(world_h) {
            return Err(format!(
                "visual_range {visual_range} needs a world at least {} across, got {world_w} by {world_h}",
                2.0 * visual_range
            ));
        }
        Ok(Boids {
            world_w,
            world_h,
            visual_range,
            visual_sq: visual_range * visual_range,
            protected_sq: protected_range * protected_range,
            separation,
            alignment,
            cohesion,
            max_speed,
            min_speed,
            // One bucket per visual range. Coarser and the field's own bucket scan stops short of
            // the radius, finer and it never reaches it.
            field: Field2D::new(world_w, world_h, visual_range, true),
            num_agents,
            agents: None,
            seed,
        })
    }

    pub fn with_agents(mut self, agents: &[(f32, f32, f32, f32)]) -> Self {
        self.num_agents = agents.len() as u32;
        self.agents = Some(agents.to_vec());
        self
    }

    pub fn population(&self) -> u64 {
        u64::from(self.num_agents)
    }

    /// Where to stand when asking the field for neighbours.
    ///
    /// `get_neighbors_within_*_distance` clamps its bucket scan to the field on a toroidal world,
    /// so it never visits the buckets across a seam. Asking again from the mirrored point one world
    /// away reaches them. A neighbour lands inside the visual range of exactly one probe, since the
    /// probes are a whole world apart and the range is under half of that, so nothing is counted
    /// twice and the offset from the probe is already the wrapped one.
    fn seam_shifts(&self, loc: Real2D) -> ([(f32, f32); 4], usize) {
        let mut xs = [0.0f32; 2];
        let mut nx = 1;
        if loc.x <= self.visual_range {
            xs[1] = self.world_w;
            nx = 2;
        } else if loc.x >= self.world_w - self.visual_range {
            xs[1] = -self.world_w;
            nx = 2;
        }

        let mut ys = [0.0f32; 2];
        let mut ny = 1;
        if loc.y <= self.visual_range {
            ys[1] = self.world_h;
            ny = 2;
        } else if loc.y >= self.world_h - self.visual_range {
            ys[1] = -self.world_h;
            ny = 2;
        }

        let mut shifts = [(0.0f32, 0.0f32); 4];
        let mut count = 0;
        for &x in &xs[..nx] {
            for &y in &ys[..ny] {
                shifts[count] = (x, y);
                count += 1;
            }
        }
        (shifts, count)
    }
}

impl State for Boids {
    fn init(&mut self, schedule: &mut Schedule) {
        let mut rng = Pcg64::seed_from_u64(self.seed);
        let speed = 0.5 * (self.min_speed + self.max_speed);

        for id in 0..self.num_agents {
            let (x, y, vx, vy) = match &self.agents {
                Some(agents) => agents[id as usize],
                None => {
                    // Scattered at random with a fixed speed and a random heading, as Henad does.
                    let x = rng.random::<f32>() * self.world_w;
                    let y = rng.random::<f32>() * self.world_h;
                    let angle = rng.random::<f32>() * std::f32::consts::TAU;
                    (x, y, angle.cos() * speed, angle.sin() * speed)
                }
            };
            let boid = Boid {
                id,
                loc: Real2D { x, y },
                vel: Real2D { x: vx, y: vy },
            };
            self.field.set_object_location(boid, boid.loc);
            schedule.schedule_repeating(Box::new(boid), 0.0, 0);
        }
    }

    fn update(&mut self, _step: u64) {
        self.field.lazy_update();
    }

    fn reset(&mut self) {
        self.field = Field2D::new(self.world_w, self.world_h, self.visual_range, true);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_state(&self) -> &dyn State {
        self
    }

    fn as_state_mut(&mut self) -> &mut dyn State {
        self
    }
}

/// `x y vx vy` per agent, in creation order, as the fixture format wants.
///
/// The schedule hands its agents back in hash order, hence the sort.
pub fn rows(schedule: &Schedule) -> Vec<(f32, f32, f32, f32)> {
    let mut boids: Vec<Boid> = schedule
        .get_all_events()
        .iter()
        .filter_map(|agent| agent.downcast_ref::<Boid>().copied())
        .collect();
    boids.sort_by_key(|boid| boid.id);
    boids
        .iter()
        .map(|boid| (boid.loc.x, boid.loc.y, boid.vel.x, boid.vel.y))
        .collect()
}
