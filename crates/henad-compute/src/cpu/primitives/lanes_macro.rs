//! `agent_lanes!`, which writes a model's `SoA` storage and its chunked step driver.

/// Declares a model's agent lanes.
///
/// A `dual` lane is double buffered, for a model whose agents read one another. Both names are
/// spelled out because a macro cannot build an identifier. A `plain` lane is written in place.
///
/// ```ignore
/// agent_lanes! {
///     pub struct BoidLanes {
///         read BoidRead;
///         chunk BoidChunk;
///         dual pos_x / next_pos_x: f32,
///         dual vel_x / next_vel_x: f32,
///         plain color: u8,
///     }
///     color = color;
/// }
/// ```
///
/// Lanes named `pos_x` and `pos_y` are required, since the engine builds the neighbour index and
/// the point view from them.
#[macro_export]
macro_rules! agent_lanes {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            read $read:ident;
            chunk $chunk:ident;
            $($(#[$dmeta:meta])* dual $dcur:ident / $dnext:ident : $dty:ty,)*
            $($(#[$pmeta:meta])* plain $pname:ident : $pty:ty = $pinit:expr,)*
        }
        $(color = $color:ident;)?
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($(#[$dmeta])* pub $dcur: ::std::vec::Vec<$dty>, pub $dnext: ::std::vec::Vec<$dty>,)*
            $($(#[$pmeta])* pub $pname: ::std::vec::Vec<$pty>,)*
        }

        /// The current side of every double buffered lane, readable by every agent.
        #[derive(::core::clone::Clone, ::core::marker::Copy)]
        $vis struct $read<'a> {
            $(pub $dcur: &'a [$dty],)*
            /// Keeps `'a` used when a model has no double buffered lane.
            #[doc(hidden)]
            pub _lifetime: ::std::marker::PhantomData<&'a ()>,
        }

        /// The slice of each writable lane that one chunk owns.
        $vis struct $chunk<'a> {
            $(pub $dcur: &'a mut [$dty],)*
            $(pub $pname: &'a mut [$pty],)*
        }

        impl $name {
            /// Runs `kernel(global_index, local_index, read, chunk, rng)` over every agent,
            /// merging the returned tally in chunk order.
            ///
            /// Chunked and seeded here so a kernel never sees the parallelism. The seed comes from
            /// the chunk index, so which agent sees which stream does not depend on scheduling.
            pub fn run_pass<K, T>(&mut self, chunk_size: usize, seed: u64, tick: u64, kernel: K) -> T
            where
                K: ::core::ops::Fn(usize, usize, $read<'_>, &mut $chunk<'_>, &mut u64) -> T + ::core::marker::Send + ::core::marker::Sync,
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
}
