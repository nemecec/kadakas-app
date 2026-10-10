//! Code pages of old DXF and shapefile text.

use encoding_rs::Encoding;

/// The encoding of a Windows or East Asian code page number (1250, 932, ...).
pub fn for_code_page(n: u32) -> Option<&'static Encoding> {
    let label = match n {
        874 => "windows-874".to_string(),
        932 => "shift_jis".to_string(),
        936 => "gbk".to_string(),
        949 => "euc-kr".to_string(),
        950 => "big5".to_string(),
        1250..=1258 => format!("windows-{n}"),
        65001 => "utf-8".to_string(),
        28591 => "iso-8859-1".to_string(),
        _ => return None,
    };
    Encoding::for_label(label.as_bytes())
}

/// The encoding that a `.cpg` file names: `UTF-8`, `1257`, `ANSI 1257`, `CP1257`, `windows-1257`, `ISO-8859-4`.
pub fn from_cpg(text: &str) -> Option<&'static Encoding> {
    let t = text.trim().trim_matches('\0').trim();
    let lower = t.to_ascii_lowercase();
    let digits = lower
        .trim_start_matches("ansi")
        .trim_start_matches("cp")
        .trim_start_matches("windows")
        .trim_start_matches(['-', '_', ' ']);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        return for_code_page(digits.parse().ok()?);
    }
    Encoding::for_label(t.as_bytes())
}

/// The encoding of a DXF `$DWGCODEPAGE` value such as `ANSI_1257`.
pub fn from_dwg_code_page(value: &str) -> Option<&'static Encoding> {
    let lower = value.trim().to_ascii_lowercase();
    let n = lower.strip_prefix("ansi_")?;
    for_code_page(n.parse().ok()?)
}

/// The encoding for the language driver byte of a dbf header.
pub fn from_dbf_language_driver(id: u8) -> Option<&'static Encoding> {
    let n = match id {
        0x03 => 1252,
        0x4D => 936,
        0x4E => 949,
        0x4F => 950,
        0x50 => 874,
        0x7C => 1256,
        0x7D => 1255,
        0xC8 => 1250,
        0xC9 => 1251,
        0xCA => 1254,
        0xCB => 1253,
        0xCC => 1257,
        _ => return None,
    };
    for_code_page(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_numbers() {
        assert_eq!(from_cpg("UTF-8\n").unwrap().name(), "UTF-8");
        assert_eq!(from_cpg("1257").unwrap().name(), "windows-1257");
        assert_eq!(from_cpg("ANSI 1257").unwrap().name(), "windows-1257");
        assert_eq!(from_cpg("CP1251").unwrap().name(), "windows-1251");
        assert_eq!(from_cpg("ISO-8859-4").unwrap().name(), "ISO-8859-4");
        assert_eq!(from_cpg("932").unwrap().name(), "Shift_JIS");
        assert!(from_cpg("nonsense").is_none());
        assert!(from_cpg("").is_none());
        assert!(from_cpg("99999").is_none());
        assert_eq!(
            from_dwg_code_page("ANSI_1250").unwrap().name(),
            "windows-1250"
        );
        assert!(from_dwg_code_page("DOS437").is_none());
        assert!(from_dwg_code_page("ANSI_x").is_none());
        assert_eq!(
            from_dbf_language_driver(0xCC).unwrap().name(),
            "windows-1257"
        );
        assert!(from_dbf_language_driver(0x01).is_none());
    }
}
