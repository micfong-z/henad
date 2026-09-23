//! Final-state output. A grid is written as one line of cell indices per row, a point cloud as `x,y` CSV,
//! and network edges as `src,dst,color` CSV.
//!
//! A file can hold every section, so a composite model writes both its field and its agents.

use std::io::{self, Write};

/// Write the grid section: a `# grid WxH` marker, then one line of comma-separated cell indices
/// per row.
///
/// # Errors
/// If writing fails.
pub fn write_grid<W: Write>(out: &mut W, width: u32, height: u32, cells: &[u8]) -> io::Result<()> {
    writeln!(out, "# grid {width}x{height}")?;
    for row in cells.chunks(width as usize) {
        let line = row.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",");
        writeln!(out, "{line}")?;
    }
    Ok(())
}

/// Row index that [`point_rows`] gives a point left out of the point section.
pub const NO_ROW: u32 = u32::MAX;

/// Returns whether a point has a finite position.
///
/// A retired node's position is `NaN`, so the point section leaves it out.
fn is_placed(x: f32, y: f32) -> bool {
    x.is_finite() && y.is_finite()
}

/// Write the point section: a `# points N` marker, a header row, then one line per agent.
///
/// The `color` column appears only if the model carries the lane.
/// A point without a finite position is left out.
///
/// # Errors
/// If writing fails.
pub fn write_points<W: Write>(out: &mut W, pos_x: &[f32], pos_y: &[f32], color: Option<&[u8]>) -> io::Result<()> {
    let placed = pos_x.iter().zip(pos_y).filter(|&(&x, &y)| is_placed(x, y)).count();
    writeln!(out, "# points {placed}")?;
    if let Some(color) = color {
        writeln!(out, "x,y,color")?;
        for ((&x, &y), c) in pos_x.iter().zip(pos_y).zip(color) {
            if is_placed(x, y) {
                writeln!(out, "{x},{y},{c}")?;
            }
        }
    } else {
        writeln!(out, "x,y")?;
        for (&x, &y) in pos_x.iter().zip(pos_y) {
            if is_placed(x, y) {
                writeln!(out, "{x},{y}")?;
            }
        }
    }
    Ok(())
}

/// Returns the row each point takes in the section written by [`write_points`],
/// or [`NO_ROW`] for a point that is left out.
pub fn point_rows(pos_x: &[f32], pos_y: &[f32]) -> Vec<u32> {
    let mut next = 0;
    pos_x
        .iter()
        .zip(pos_y)
        .map(|(&x, &y)| {
            if is_placed(x, y) {
                next += 1;
                next - 1
            } else {
                NO_ROW
            }
        })
        .collect()
}

/// Write the edge section: a `# edges N` marker, a header row, then one line per edge.
///
/// Endpoints are mapped through `rows`, as returned by [`point_rows`], so they index the point section as written.
/// An edge with an endpoint left out of that section is dropped as well.
///
/// # Errors
/// If writing fails.
pub fn write_edges<W: Write>(out: &mut W, src: &[u32], dst: &[u32], color: &[u8], rows: &[u32]) -> io::Result<()> {
    let row = |node: u32| rows.get(node as usize).copied().unwrap_or(NO_ROW);
    let kept = |&(&a, &b): &(&u32, &u32)| row(a) != NO_ROW && row(b) != NO_ROW;
    let count = src.iter().zip(dst).filter(kept).count();
    writeln!(out, "# edges {count}")?;
    writeln!(out, "src,dst,color")?;
    for (i, pair) in src.iter().zip(dst).enumerate() {
        if kept(&pair) {
            let c = color.get(i).copied().unwrap_or(0);
            writeln!(out, "{},{},{c}", row(*pair.0), row(*pair.1))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NO_ROW, point_rows, write_edges, write_grid, write_points};

    fn render(f: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut buf = Vec::new();
        f(&mut buf);
        String::from_utf8(buf).expect("output should be utf8")
    }

    #[test]
    fn a_grid_writes_one_line_per_row() {
        let text = render(|out| write_grid(out, 3, 2, &[0, 1, 2, 3, 4, 5]).expect("write"));
        assert_eq!(text, "# grid 3x2\n0,1,2\n3,4,5\n");
    }

    #[test]
    fn points_gain_a_color_column_only_when_the_lane_exists() {
        let bare = render(|out| write_points(out, &[0.5, 1.5], &[2.0, 3.0], None).expect("write"));
        assert_eq!(bare, "# points 2\nx,y\n0.5,2\n1.5,3\n");

        let colored = render(|out| write_points(out, &[0.5], &[2.0], Some(&[7])).expect("write"));
        assert_eq!(colored, "# points 1\nx,y,color\n0.5,2,7\n");
    }

    #[test]
    fn edges_write_their_endpoints_and_colours() {
        let rows = [0, 1, 2, 3];
        let text = render(|out| write_edges(out, &[0, 2], &[1, 3], &[0, 5], &rows).expect("write"));
        assert_eq!(text, "# edges 2\nsrc,dst,color\n0,1,0\n2,3,5\n");
    }

    /// A model without edge colours still writes the column, so one parser can read either file.
    #[test]
    fn edges_without_colours_fall_back_to_zero() {
        let rows: Vec<u32> = (0..9).collect();
        let text = render(|out| write_edges(out, &[7], &[8], &[], &rows).expect("write"));
        assert_eq!(text, "# edges 1\nsrc,dst,color\n7,8,0\n");
    }

    /// A retired node sits at `NaN`. It is left out of the point section, and the rows after it close up.
    #[test]
    fn a_retired_node_is_left_out_and_the_rows_close_up() {
        let (xs, ys) = ([0.5, f32::NAN, 2.5], [1.0, f32::NAN, 3.0]);
        let text = render(|out| write_points(out, &xs, &ys, Some(&[1, 2, 3])).expect("write"));
        assert_eq!(text, "# points 2\nx,y,color\n0.5,1,1\n2.5,3,3\n");

        let rows = point_rows(&xs, &ys);
        assert_eq!(rows, [0, NO_ROW, 1]);
        let text = render(|out| write_edges(out, &[0, 0], &[2, 1], &[4, 6], &rows).expect("write"));
        assert_eq!(
            text, "# edges 1\nsrc,dst,color\n0,1,4\n",
            "node 2 is row 1 now, and 1 is gone"
        );
    }
}
