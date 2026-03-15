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

    /// Converts (x, y) to a flat index.
    #[inline]
    pub fn index(&self, x: u32, y: u32) -> usize {
        (y as usize) * (self.width as usize) + (x as usize)
    }

    /// Returns the 8 Moore neighbor indices for cell at (x, y) with toroidal wrapping.
    #[inline]
    pub fn moore_neighbors(&self, x: u32, y: u32) -> [usize; 8] {
        let w = self.width;
        let h = self.height;
        let xm = (x + w - 1) % w;
        let xp = (x + 1) % w;
        let ym = (y + h - 1) % h;
        let yp = (y + 1) % h;

        [
            self.index(xm, ym),
            self.index(x, ym),
            self.index(xp, ym),
            self.index(xm, y),
            self.index(xp, y),
            self.index(xm, yp),
            self.index(x, yp),
            self.index(xp, yp),
        ]
    }

    /// Returns the 4 Von Neumann neighbor indices for cell at (x, y) with toroidal wrapping.
    #[inline]
    pub fn von_neumann_neighbors(&self, x: u32, y: u32) -> [usize; 4] {
        let w = self.width;
        let h = self.height;
        [
            self.index(x, (y + h - 1) % h),
            self.index((x + w - 1) % w, y),
            self.index((x + 1) % w, y),
            self.index(x, (y + 1) % h),
        ]
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
    fn moore_neighbors_center() {
        let grid: Grid2D<u8> = Grid2D::new(5, 5);
        let neighbors = grid.moore_neighbors(2, 2);
        let expected = [
            grid.index(1, 1),
            grid.index(2, 1),
            grid.index(3, 1),
            grid.index(1, 2),
            grid.index(3, 2),
            grid.index(1, 3),
            grid.index(2, 3),
            grid.index(3, 3),
        ];
        assert_eq!(neighbors, expected);
    }

    #[test]
    fn moore_neighbors_wrapping() {
        let grid: Grid2D<u8> = Grid2D::new(5, 5);
        let neighbors = grid.moore_neighbors(0, 0);
        // Top-left corner should wrap around
        assert_eq!(neighbors[0], grid.index(4, 4)); // NW wraps both
        assert_eq!(neighbors[1], grid.index(0, 4)); // N wraps y
        assert_eq!(neighbors[2], grid.index(1, 4)); // NE wraps y
        assert_eq!(neighbors[3], grid.index(4, 0)); // W wraps x
        assert_eq!(neighbors[4], grid.index(1, 0)); // E
        assert_eq!(neighbors[5], grid.index(4, 1)); // SW wraps x
        assert_eq!(neighbors[6], grid.index(0, 1)); // S
        assert_eq!(neighbors[7], grid.index(1, 1)); // SE
    }

    #[test]
    fn von_neumann_neighbors_wrapping() {
        let grid: Grid2D<u8> = Grid2D::new(4, 4);
        let neighbors = grid.von_neumann_neighbors(0, 0);
        assert_eq!(neighbors[0], grid.index(0, 3)); // N wraps
        assert_eq!(neighbors[1], grid.index(3, 0)); // W wraps
        assert_eq!(neighbors[2], grid.index(1, 0)); // E
        assert_eq!(neighbors[3], grid.index(0, 1)); // S
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
