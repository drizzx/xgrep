//! A1 notation utilities. Rows/columns are 0-indexed internally.

/// Convert (row, col) 0-indexed to Excel A1 notation, e.g. (0,0) -> "A1", (2,1) -> "B3".
pub fn to_a1(row: u32, col: u32) -> String {
    let mut col_str = String::new();
    let mut c = col + 1; // A1 columns are 1-indexed
    while c > 0 {
        let rem = (c - 1) % 26;
        col_str.insert(0, (b'A' + rem as u8) as char);
        c = (c - 1) / 26;
    }
    format!("{}{}", col_str, row + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_origin() {
        assert_eq!(to_a1(0, 0), "A1");
    }

    #[test]
    fn a1_b3() {
        assert_eq!(to_a1(2, 1), "B3");
    }

    #[test]
    fn a1_column_boundary_z_to_aa() {
        assert_eq!(to_a1(0, 25), "Z1");
        assert_eq!(to_a1(0, 26), "AA1");
        assert_eq!(to_a1(0, 27), "AB1");
    }

    #[test]
    fn a1_double_letter_boundary() {
        assert_eq!(to_a1(0, 701), "ZZ1");
        assert_eq!(to_a1(0, 702), "AAA1");
    }

    #[test]
    fn a1_large_row() {
        assert_eq!(to_a1(1_048_575, 0), "A1048576");
    }
}
