// Conway's Game of Life. A live cell with two or three live neighbours stays alive, and a dead cell with exactly
// three comes alive.
fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {
    let alive = moore_count(ALIVE);
    if (alive == 3u || (cell == ALIVE && alive == 2u)) {
        return ALIVE;
    }
    return DEAD;
}
