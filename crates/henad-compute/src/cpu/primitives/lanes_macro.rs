//! `agent_lanes!`, which writes a model's `SoA` storage and its chunked step driver.

/// Declares a model's agent lanes.
///
/// A `dual` lane is double buffered, for a model whose agents read one another. Both names are
/// spelled out because a macro cannot build an identifier. A `plain` lane is written in place, and
/// every agent starts at its initial value. Every `dual` lane comes before the first `plain` lane.
///
/// ```
/// # #![deny(missing_docs)]
/// # //! A crate that documents every public item.
/// henad_compute::agent_lanes! {
///     /// Lanes of a flock.
///     pub struct BoidLanes {
///         read BoidRead;
///         chunk BoidChunk;
///         /// Position along x.
///         dual pos_x / next_pos_x: f32,
///         /// Position along y.
///         dual pos_y / next_pos_y: f32,
///         /// Velocity along x.
///         dual vel_x / next_vel_x: f32,
///         /// Palette index of each boid.
///         plain color: u8 = 0,
///     }
///     color = color;
/// }
/// # fn main() {}
/// ```
///
/// Lanes named `pos_x` and `pos_y` are required, since the engine builds the neighbour index and
/// the point view from them.
///
/// A lane's doc comment and attributes go on its field of the lanes struct, and on both fields of
/// a `dual` lane. The fields of the read view and the chunk carry generated docs.
///
/// The three types implement `Debug`. The lanes and the chunk print their agent count alone, and
/// the read view prints no field. A `#[derive(Debug)]` on the declaration conflicts with that impl.
#[macro_export]
macro_rules! agent_lanes {
    // Each lane is read as its attributes, then its keyword. A single pattern listing every lane cannot tell
    // which repetition an attribute starts.
    (
        @dual $head:tt [$($dual:tt)*]
        $(#[$attribute:meta])* dual $current:ident / $next:ident : $ty:ty, $($rest:tt)*
    ) => {
        $crate::agent_lanes! {
            @dual $head [$($dual)* { $(#[$attribute])* $current / $next : $ty }] $($rest)*
        }
    };
    (@dual $head:tt $duals:tt $($rest:tt)*) => {
        $crate::agent_lanes! { @plain $head $duals [] $($rest)* }
    };
    (
        @plain $head:tt $duals:tt [$($plain:tt)*]
        $(#[$attribute:meta])* plain $name:ident : $ty:ty = $init:expr, $($rest:tt)*
    ) => {
        $crate::agent_lanes! {
            @plain $head $duals [$($plain)* { $(#[$attribute])* $name : $ty = $init }] $($rest)*
        }
    };
    (
        @plain
        [
            $(#[$meta:meta])*
            $vis:vis struct $name:ident;
            read $read:ident;
            chunk $chunk:ident;
            $(color = $color:ident;)?
        ]
        [$({ $(#[$dmeta:meta])* $dcur:ident / $dnext:ident : $dty:ty })*]
        [$({ $(#[$pmeta:meta])* $pname:ident : $pty:ty = $pinit:expr })*]
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($(#[$dmeta])* pub $dcur: ::std::vec::Vec<$dty>, $(#[$dmeta])* pub $dnext: ::std::vec::Vec<$dty>,)*
            $($(#[$pmeta])* pub $pname: ::std::vec::Vec<$pty>,)*
        }

        /// The current side of every double buffered lane, readable by every agent.
        #[derive(::core::clone::Clone, ::core::marker::Copy)]
        $vis struct $read<'a> {
            $(
                #[doc = ::core::concat!("Current side of the `", ::core::stringify!($dcur), "` lane.")]
                pub $dcur: &'a [$dty],
            )*
            /// Keeps `'a` used when a model has no double buffered lane.
            #[doc(hidden)]
            pub _lifetime: ::std::marker::PhantomData<&'a ()>,
        }

        /// The slice of each writable lane that one chunk owns.
        $vis struct $chunk<'a> {
            $(
                #[doc = ::core::concat!(
                    "Slice of the `", ::core::stringify!($dcur), "` lane's next side that the chunk writes."
                )]
                pub $dcur: &'a mut [$dty],
            )*
            $(
                #[doc = ::core::concat!("Slice of the `", ::core::stringify!($pname), "` lane that the chunk owns.")]
                pub $pname: &'a mut [$pty],
            )*
        }

        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(::core::stringify!($name))
                    .field("len", &self.pos_x.len())
                    .finish_non_exhaustive()
            }
        }

        impl ::core::fmt::Debug for $read<'_> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(::core::stringify!($read)).finish_non_exhaustive()
            }
        }

        impl ::core::fmt::Debug for $chunk<'_> {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(::core::stringify!($chunk))
                    .field("len", &self.pos_x.len())
                    .finish_non_exhaustive()
            }
        }

        impl $name {
            /// Runs `kernel(global_index, local_index, read, chunk, rng)` over every agent,
            /// merging the returned tally in chunk order.
            ///
            /// Chunked and seeded here so a kernel never sees the parallelism. The seed comes from
            /// the chunk index, so which agent sees which stream does not depend on scheduling.
            pub fn run_pass<K, T>(&mut self, chunk_size: usize, seed: u64, tick: u64, kernel: K) -> T
            where
                K: ::core::ops::Fn(usize, usize, $read<'_>, &mut $chunk<'_>, &mut u64) -> T
                    + ::core::marker::Send
                    + ::core::marker::Sync,
                T: $crate::__macro_support::ChunkTally,
            {
                let chunk_size = chunk_size.max(1);
                let Self { $($dcur, $dnext,)* $($pname,)* } = self;

                let read = $read {
                    $($dcur,)*
                    _lifetime: ::std::marker::PhantomData,
                };

                // One view per chunk, zipped here rather than through a nested rayon zip. The Vec
                // holds one entry per chunk, not per agent.
                $(let mut $dnext = $dnext.chunks_mut(chunk_size);)*
                $(let mut $pname = $pname.chunks_mut(chunk_size);)*
                let mut views: ::std::vec::Vec<$chunk<'_>> = ::std::iter::from_fn(|| {
                    ::core::option::Option::Some($chunk {
                        $($dcur: $dnext.next()?,)*
                        $($pname: $pname.next()?,)*
                    })
                })
                .collect();

                let run = |c: usize, view: &mut $chunk<'_>| {
                    let base = c * chunk_size;
                    let mut rng = $crate::__macro_support::chunk_seed(seed, tick, c);
                    let mut acc = <T as ::core::default::Default>::default();
                    for k in 0..view.pos_x.len() {
                        acc = <T as $crate::__macro_support::ChunkTally>::merge(
                            acc,
                            kernel(base + k, k, read, view, &mut rng),
                        );
                    }
                    acc
                };

                let per_chunk: ::std::vec::Vec<T> = {
                    use $crate::__macro_support::rayon::prelude::*;
                    views.par_iter_mut().enumerate().map(|(c, v)| run(c, v)).collect()
                };

                per_chunk
                    .into_iter()
                    .fold(<T as ::core::default::Default>::default(), <T as $crate::__macro_support::ChunkTally>::merge)
            }
        }

        impl $crate::__macro_support::AgentLanes for $name {
            const LANES: &'static [$crate::__macro_support::LaneSpec] = &[
                $($crate::__macro_support::LaneSpec {
                    name: ::std::stringify!($dcur),
                    ty: ::std::stringify!($dty),
                    double_buffered: true,
                },)*
                $($crate::__macro_support::LaneSpec {
                    name: ::std::stringify!($pname),
                    ty: ::std::stringify!($pty),
                    double_buffered: false,
                },)*
            ];

            fn alloc(n: usize) -> Self {
                Self {
                    $($dcur: ::std::vec![<$dty as ::core::default::Default>::default(); n],
                      $dnext: ::std::vec![<$dty as ::core::default::Default>::default(); n],)*
                    $($pname: ::std::vec![$pinit; n],)*
                }
            }

            fn len(&self) -> usize {
                self.pos_x.len()
            }

            fn swap(&mut self) {
                $(::std::mem::swap(&mut self.$dcur, &mut self.$dnext);)*
            }

            fn heap_bytes(&self) -> usize {
                0 $(+ self.$dcur.capacity() * 2 * ::std::mem::size_of::<$dty>())*
                  $(+ self.$pname.capacity() * ::std::mem::size_of::<$pty>())*
            }

            fn positions(&self) -> (&[f32], &[f32]) {
                (&self.pos_x, &self.pos_y)
            }

            fn positions_mut(&mut self) -> (&mut [f32], &mut [f32]) {
                let Self { pos_x, pos_y, .. } = self;
                (pos_x, pos_y)
            }

            fn grow(&mut self, n: usize) {
                if n <= self.len() {
                    return;
                }
                $(self.$dcur.resize(n, <$dty as ::core::default::Default>::default());
                  self.$dnext.resize(n, <$dty as ::core::default::Default>::default());)*
                $(self.$pname.resize(n, $pinit);)*
            }

            $(fn colors(&self) -> ::core::option::Option<&[u8]> {
                ::core::option::Option::Some(&self.$color)
            })?
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            read $read:ident;
            chunk $chunk:ident;
            $($lanes:tt)*
        }
        $(color = $color:ident;)?
    ) => {
        $crate::agent_lanes! {
            @dual
            [
                $(#[$meta])*
                $vis struct $name;
                read $read;
                chunk $chunk;
                $(color = $color;)?
            ]
            []
            $($lanes)*
        }
    };
}
