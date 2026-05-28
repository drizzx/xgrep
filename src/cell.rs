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

/// Inverse of the row-component of `to_a1`. Extracts the row number from an
/// A1-style cell address. Returns `None` if the address is malformed:
/// - empty string
/// - no leading letters
/// - no trailing digits
/// - digit prefix is "0" (row 0 is not a valid A1 row)
pub fn row_from_a1(cell: &str) -> Option<u32> {
    let split = cell.find(|c: char| c.is_ascii_digit())?;
    if split == 0 {
        return None; // no leading letters
    }
    let digits = &cell[split..];
    let row: u32 = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some(row)
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

    #[test]
    fn row_from_a1_basic() {
        assert_eq!(row_from_a1("A1").unwrap(), 1);
        assert_eq!(row_from_a1("B3").unwrap(), 3);
        assert_eq!(row_from_a1("Z99").unwrap(), 99);
    }

    #[test]
    fn row_from_a1_multi_letter_column() {
        assert_eq!(row_from_a1("AA1").unwrap(), 1);
        assert_eq!(row_from_a1("ZZ42").unwrap(), 42);
        assert_eq!(row_from_a1("AAA1000").unwrap(), 1000);
    }

    #[test]
    fn row_from_a1_invalid_returns_none() {
        assert!(row_from_a1("").is_none());
        assert!(row_from_a1("A").is_none());          // no digits
        assert!(row_from_a1("123").is_none());        // no letters
        assert!(row_from_a1("A0").is_none());         // row 0 not valid
    }
}
