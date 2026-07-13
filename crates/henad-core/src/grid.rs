use std::mem;

/// A double-buffered 2D grid with toroidal wrapping.
pub struct Grid2D<T: Copy + Default> {
    width: u32,
    height: u32,
    current: Vec<T>,
    next: Vec<T>,
}

impl<T: Copy + Default> Grid2D<T> {
    /// Creates a new grid filled with the default value.
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize) * (height as usize);
        Self {
            width,
            height,
            current: vec![T::default(); len],
            next: vec![T::default(); len],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Heap bytes used by both buffers.
    pub fn heap_bytes(&self) -> usize {
        (self.current.capacity() + self.next.capacity()) * mem::size_of::<T>()
    }

    pub fn len(&self) -> usize {
        self.current.len()
    }

    pub fn is_empty(&self) -> bool {
        self.current.is_empty()
    }

    /// Returns a read-only slice of the current buffer.
    pub fn current(&self) -> &[T] {
        &self.current
    }

    /// Returns a mutable slice of the current buffer (for initialization).
    pub fn current_mut(&mut self) -> &mut [T] {
        &mut self.current
    }

    /// Returns a mutable slice of the next buffer.
    pub fn next_mut(&mut self) -> &mut [T] {
        &mut self.next
    }

    /// Returns both current (read) and next (write) buffers via split borrows.
    pub fn current_and_next_mut(&mut self) -> (&[T], &mut [T]) {
        (&self.current, &mut self.next)
    }

    /// Swaps the current and next buffers (pointer swap, O(1)).
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
