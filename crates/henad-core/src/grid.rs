//! The double-buffered [`Grid2D`] that holds a grid model's cells.

use std::mem;

/// A double-buffered 2D grid of cells, stored row-major.
///
/// A step reads [`Self::current`] and writes [`Self::next_mut`], then [`Self::swap`] makes the next
/// side current.
pub struct Grid2D<T: Copy + Default> {
    width: u32,
    height: u32,
    current: Vec<T>,
    next: Vec<T>,
}

/// Prints the grid's size, not its cells.
impl<T: Copy + Default> std::fmt::Debug for Grid2D<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grid2D")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl<T: Copy + Default> Grid2D<T> {
    /// Creates a `width` by `height` grid with every cell at `T::default()` on both sides.
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize) * (height as usize);
        Self {
            width,
            height,
            current: vec![T::default(); len],
            next: vec![T::default(); len],
        }
    }

    /// Width in cells.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Heap memory held by both sides, in bytes.
    pub fn heap_bytes(&self) -> usize {
        (self.current.capacity() + self.next.capacity()) * mem::size_of::<T>()
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.current.len()
    }

    /// Returns whether the grid has no cells.
    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// Cells of the current side.
    pub fn current(&self) -> &[T] {
        &self.current
    }

    /// Returns the current side mutably, for initialisation.
    ///
    /// A step writes through [`Self::next_mut`].
    pub fn current_mut(&mut self) -> &mut [T] {
        &mut self.current
    }

    /// Returns the next side for a step to write.
    pub fn next_mut(&mut self) -> &mut [T] {
        &mut self.next
    }

    /// Returns the current side to read and the next side to write, as one split borrow.
    pub fn current_and_next_mut(&mut self) -> (&[T], &mut [T]) {
        (&self.current, &mut self.next)
    }

    /// Makes the next side current by swapping the two buffers, copying no cells.
    pub fn swap(&mut self) {
        mem::swap(&mut self.current, &mut self.next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_new_and_size() {
        let grid: Grid2D<u8> = Grid2D::new(10, 20);
        assert_eq!(grid.width(), 10);
        assert_eq!(grid.height(), 20);
        assert_eq!(grid.len(), 200);
        assert!(!grid.is_empty());
        assert!(grid.current().iter().all(|&v| v == 0));
    }

    #[test]
    fn grid_swap() {
        let mut grid: Grid2D<u8> = Grid2D::new(3, 3);
        grid.next_mut()[0] = 42;
        assert_eq!(grid.current()[0], 0);
        grid.swap();
        assert_eq!(grid.current()[0], 42);
    }

    #[test]
    fn current_and_next_mut_split_borrow() {
        let mut grid: Grid2D<u8> = Grid2D::new(3, 3);
        grid.current_mut()[4] = 10;
        let (cur, nxt) = grid.current_and_next_mut();
        nxt[4] = cur[4] + 1;
        assert_eq!(grid.next_mut()[4], 11);
    }
}
