//! Final-state output. A grid as one line of cell indices per row, a point cloud as `x,y` CSV.
//!
//! A file can hold both sections, so a composite model writes its field and its agents.

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

/// Write the point section: a `# points N` marker, a header row, then one line per agent.
///
/// The `color` column appears only where the model carries the lane.
///
/// # Errors
/// If writing fails.
pub fn write_points<W: Write>(out: &mut W, pos_x: &[f32], pos_y: &[f32], color: Option<&[u8]>) -> io::Result<()> {
    writeln!(out, "# points {}", pos_x.len())?;
    if let Some(color) = color {
        writeln!(out, "x,y,color")?;
        for ((x, y), c) in pos_x.iter().zip(pos_y).zip(color) {
            writeln!(out, "{x},{y},{c}")?;
        }
    } else {
        writeln!(out, "x,y")?;
        for (x, y) in pos_x.iter().zip(pos_y) {
            writeln!(out, "{x},{y}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{write_grid, write_points};

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
}
