//! The memory that a `.kad` file needs to open (DOC-19), shared by every program that opens the
//! files of a person: the desktop app asks and refuses with it (`kadakas_ui_core::open_limit`),
//! and the local commands of the CLI refuse with it.
//!
//! A loaded drawing needs 1.1 to 1.7 times the file size in memory (`docs/doc-load-benchmark.md`).
//! The estimate takes the top of the range.

const FACTOR_NUM: u128 = 17;
const FACTOR_DEN: u128 = 10;

/// The memory that a file of `file_bytes` needs to open, in bytes (1.7 times the size).
pub fn needed_bytes(file_bytes: u64) -> u64 {
    let n = u128::from(file_bytes) * FACTOR_NUM / FACTOR_DEN;
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// The largest file whose estimate fits in `memory` bytes. `None`: the memory is unknown, so no
/// file is too big.
pub fn largest_file_for(memory: Option<u64>) -> u64 {
    match memory {
        None => u64::MAX,
        Some(m) => {
            let n = u128::from(m) * FACTOR_DEN / FACTOR_NUM;
            u64::try_from(n).unwrap_or(u64::MAX)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_is_one_point_seven_times_the_file() {
        assert_eq!(needed_bytes(0), 0);
        assert_eq!(needed_bytes(1000), 1700);
        assert_eq!(needed_bytes(u64::MAX), u64::MAX);
    }

    #[test]
    fn the_largest_file_is_the_one_whose_estimate_fits() {
        assert_eq!(largest_file_for(None), u64::MAX);
        assert_eq!(largest_file_for(Some(1700)), 1000);
        for memory in [1u64, 1700, 12_345_678, 8_000_000_000] {
            let file = largest_file_for(Some(memory));
            assert!(needed_bytes(file) <= memory, "{memory}");
            // The next sizes do not fit (the estimate rounds down, so one more byte may still fit).
            assert!(needed_bytes(file + 2) > memory, "{memory}");
        }
        assert_eq!(
            largest_file_for(Some(u64::MAX)),
            (u128::from(u64::MAX) * 10 / 17) as u64
        );
    }
}
