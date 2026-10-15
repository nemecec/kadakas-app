//! The decision to open a big `.kad` file (DOC-19), without egui.
//!
//! A limit on the data of a person must ask the person and not decide alone. So the desktop app has
//! no fixed cap for the files that a person opens. Above [`ASK_ABOVE_BYTES`] it asks, with an
//! estimate of the memory that the file needs. It refuses only when the estimate is more than
//! the physical memory of the computer. When the memory is unknown it never refuses (the browser
//! build always has unknown memory, so it only asks). Agents cannot be asked: they have the hard
//! limit `SessionLimits::document_file_bytes`.
//!
//! [`decide`] is a pure function. Every way into a file of the person (File > Open, the library
//! browser, the recovery of an autosave, the browser's file picker) calls it with the size of the
//! file and `kadakas_sys::sysinfo::physical_memory()` (the app does this in
//! `file_io::open_decision`), and shows [`Estimate::ask_text`] or [`Estimate::refuse_text`].
//! The sizes in the texts are decimal: 1 MB is 1 000 000 bytes and 1 GB is 1 000 000 000 bytes.

use kadakas_core::open_estimate::needed_bytes;
use kadakas_l10n::tr;

/// Files above this size need a question first: 500 MB.
pub const ASK_ABOVE_BYTES: u64 = 500_000_000;

/// The numbers behind the question or the refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Estimate {
    /// The size of the file.
    pub file_bytes: u64,
    /// The memory that the file needs to open.
    pub needed_bytes: u64,
    /// The physical memory of the computer, if it is known.
    pub memory_bytes: Option<u64>,
}

/// What to do with a file that a person wants to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenDecision {
    /// Open it without a question.
    Open,
    /// Ask first: "This file is N MB and needs about M GB of memory to open. Open it?"
    Ask(Estimate),
    /// The estimate is more than the memory of the computer. Do not open the file.
    Refuse(Estimate),
}

/// Decides how to open a file of `file_bytes` whose text is all loaded into memory (the plain JSON
/// form). `physical_memory` is the memory of the computer
/// (`kadakas_sys::sysinfo::physical_memory`). Unknown memory never refuses.
pub fn decide(file_bytes: u64, physical_memory: Option<u64>) -> OpenDecision {
    decide_sized(file_bytes, file_bytes, physical_memory)
}

/// Like [`decide`], for a file that loads only `resident_bytes` of JSON text into memory. A
/// container (DOC-20) keeps its pictures in the file (DOC-21): what it loads is the drawing and the
/// history, uncompressed, which the manifest names (`kadakas_file::native::Inspect::resident_bytes`).
/// The question comes when that is above [`ASK_ABOVE_BYTES`], and the estimate of the memory is
/// made from it. The question still names the size of the file.
pub fn decide_sized(
    file_bytes: u64,
    resident_bytes: u64,
    physical_memory: Option<u64>,
) -> OpenDecision {
    if resident_bytes <= ASK_ABOVE_BYTES {
        return OpenDecision::Open;
    }
    let est = Estimate {
        file_bytes,
        needed_bytes: needed_bytes(resident_bytes),
        memory_bytes: physical_memory,
    };
    match physical_memory {
        Some(m) if est.needed_bytes > m => OpenDecision::Refuse(est),
        _ => OpenDecision::Ask(est),
    }
}

fn megabytes(bytes: u64) -> String {
    kadakas_units::current().number_text((bytes as f64 / 1_000_000.0).round(), 0)
}

fn gigabytes(bytes: u64) -> String {
    kadakas_units::current().number_text(bytes as f64 / 1_000_000_000.0, 1)
}

impl Estimate {
    /// The question before a big file opens (the dialog has Open and Cancel).
    pub fn ask_text(&self) -> String {
        tr!(
            "uic-open-limit-ask",
            size = megabytes(self.file_bytes),
            memory = gigabytes(self.needed_bytes)
        )
    }

    /// The refusal of a file that the computer cannot hold.
    pub fn refuse_text(&self) -> String {
        tr!(
            "uic-open-limit-refuse",
            size = megabytes(self.file_bytes),
            memory = gigabytes(self.needed_bytes),
            installed = gigabytes(self.memory_bytes.unwrap_or(0))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000 * MB;

    #[test]
    fn small_files_open_without_a_question() {
        assert_eq!(decide(0, None), OpenDecision::Open);
        assert_eq!(decide(ASK_ABOVE_BYTES, Some(GB)), OpenDecision::Open);
        // Even a computer with very little memory is not asked about a small file.
        assert_eq!(decide(10 * MB, Some(MB)), OpenDecision::Open);
    }

    #[test]
    fn a_file_above_the_threshold_asks() {
        let size = ASK_ABOVE_BYTES + 1;
        match decide(size, Some(16 * GB)) {
            OpenDecision::Ask(e) => {
                assert_eq!(e.file_bytes, size);
                assert_eq!(e.needed_bytes, needed_bytes(size));
                assert_eq!(e.memory_bytes, Some(16 * GB));
            }
            d => panic!("{d:?}"),
        }
        // The file from the measurements: 1.08 GB asks on a computer with 8 GB.
        assert!(matches!(
            decide(1_083_000_000, Some(8 * GB)),
            OpenDecision::Ask(_)
        ));
    }

    #[test]
    fn a_container_asks_by_what_it_loads_and_not_by_its_size_on_disk() {
        // A file of 900 MB that is mostly pictures loads 100 MB: no question.
        assert_eq!(
            decide_sized(900 * MB, 100 * MB, Some(2 * GB)),
            OpenDecision::Open
        );
        // A small file that expands to 2 GB of drawing asks (and the estimate follows the 2 GB).
        match decide_sized(100 * MB, 2 * GB, Some(16 * GB)) {
            OpenDecision::Ask(e) => {
                assert_eq!(e.file_bytes, 100 * MB);
                assert_eq!(e.needed_bytes, needed_bytes(2 * GB));
            }
            d => panic!("{d:?}"),
        }
        assert!(matches!(
            decide_sized(100 * MB, 2 * GB, Some(3 * GB)),
            OpenDecision::Refuse(_)
        ));
        assert_eq!(decide(10 * MB, None), decide_sized(10 * MB, 10 * MB, None));
    }

    #[test]
    fn unknown_memory_never_refuses() {
        for size in [ASK_ABOVE_BYTES + 1, 100 * GB, u64::MAX] {
            assert!(matches!(decide(size, None), OpenDecision::Ask(_)), "{size}");
        }
    }

    #[test]
    fn a_file_is_refused_only_when_the_estimate_is_more_than_the_memory() {
        let size = 10 * GB;
        let need = needed_bytes(size);
        assert!(matches!(decide(size, Some(need)), OpenDecision::Ask(_)));
        assert!(matches!(decide(size, Some(need + 1)), OpenDecision::Ask(_)));
        match decide(size, Some(need - 1)) {
            OpenDecision::Refuse(e) => assert_eq!(e.memory_bytes, Some(need - 1)),
            d => panic!("{d:?}"),
        }
        // 8 GB of memory cannot hold a file of 5 GB (estimate 8.5 GB), but holds one of 4 GB.
        assert!(matches!(
            decide(5 * GB, Some(8 * GB)),
            OpenDecision::Refuse(_)
        ));
        assert!(matches!(decide(4 * GB, Some(8 * GB)), OpenDecision::Ask(_)));
    }

    #[test]
    fn the_texts_name_the_sizes() {
        let OpenDecision::Ask(e) = decide(GB, None) else {
            panic!("asks")
        };
        let t = e.ask_text();
        assert!(t.contains("1000") && t.contains("1.7"), "{t}");
        assert!(t.contains("Open it?"), "{t}");
        let OpenDecision::Refuse(e) = decide(10 * GB, Some(4 * GB)) else {
            panic!("refuses")
        };
        let t = e.refuse_text();
        assert!(
            t.contains("10000") && t.contains("17.0") && t.contains("4.0"),
            "{t}"
        );
    }

    #[test]
    fn the_texts_exist_in_estonian() {
        let OpenDecision::Ask(e) = decide(GB, None) else {
            panic!("asks")
        };
        let (ask, refuse) = kadakas_l10n::with_language("et", || (e.ask_text(), e.refuse_text()));
        assert!(ask.contains("Kas avada see?"), "{ask}");
        assert!(refuse.contains("mälu"), "{refuse}");
    }
}
