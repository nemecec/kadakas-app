//! Export of calculated worksheets to CSV and XLSX.
//!
//! The XLSX writer is small and has no dependencies: it writes values (numbers, text, booleans),
//! not formulas, with the decimals of the cell format. The ZIP container is "stored" (no
//! compression). Excel, LibreOffice and Numbers open the files.

use crate::compute::Computed;
use crate::formula::CellRef;
use crate::value::Value;

/// The sheet as CSV text (RFC 4180, lines end with `\n`). Numbers use the decimals of their
/// cell format and no unit. Text that starts with `=`, `+`, `-` or `@` gets a leading `'`,
/// so that a spreadsheet program does not run it as a formula.
pub fn csv(c: &Computed) -> String {
    let mut out = String::new();
    for r in 1..=c.rows {
        let line: Vec<String> = (1..=c.cols)
            .map(|col| {
                let text = c.display_plain(r, col);
                if matches!(c.value(r, col), Value::Text(_)) {
                    cadektor_core::csv::text_cell(&text)
                } else {
                    cadektor_core::csv::quote(&text)
                }
            })
            .collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

// --- XLSX ---

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    !data.iter().fold(!0u32, |c, &b| {
        table[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8)
    })
}

/// A ZIP file with stored (uncompressed) entries.
fn zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        // Local header.
        le32(&mut out, 0x0403_4b50);
        le16(&mut out, 20); // version needed
        le16(&mut out, 0x0800); // flags: UTF-8 names
        le16(&mut out, 0); // stored
        le16(&mut out, 0); // time
        le16(&mut out, 0x21); // date: 1980-01-01
        le32(&mut out, crc);
        le32(&mut out, data.len() as u32);
        le32(&mut out, data.len() as u32);
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        // Central directory entry.
        le32(&mut central, 0x0201_4b50);
        le16(&mut central, 20); // made by
        le16(&mut central, 20); // needed
        le16(&mut central, 0x0800);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0x21);
        le32(&mut central, crc);
        le32(&mut central, data.len() as u32);
        le32(&mut central, data.len() as u32);
        le16(&mut central, name.len() as u16);
        le16(&mut central, 0); // extra
        le16(&mut central, 0); // comment
        le16(&mut central, 0); // disk
        le16(&mut central, 0); // internal attributes
        le32(&mut central, 0); // external attributes
        le32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    le32(&mut out, 0x0605_4b50);
    le16(&mut out, 0);
    le16(&mut out, 0);
    le16(&mut out, files.len() as u16);
    le16(&mut out, files.len() as u16);
    le32(&mut out, cd_size);
    le32(&mut out, cd_offset);
    le16(&mut out, 0);
    out
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Characters that XML 1.0 does not allow: controls other than tab, LF and CR, and the
            // two non-characters U+FFFE and U+FFFF.
            c if ((c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r'))
                || matches!(c, '\u{fffe}' | '\u{ffff}') => {}
            c => out.push(c),
        }
    }
    out
}

/// The sheet name for the workbook: no `[]:*?/\`, at most 31 characters, not empty.
pub fn sheet_name(name: &str) -> String {
    let n: String = name
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\') && !c.is_control())
        .take(31)
        .collect();
    let n = n.trim().trim_matches('\'').to_string();
    if n.is_empty() { "Sheet1".into() } else { n }
}

/// The largest number of decimals that the XLSX styles cover.
const STYLE_DECIMALS: u8 = 6;

fn styles_xml() -> String {
    let mut fmts = String::new();
    let mut xfs =
        String::from("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>");
    for d in 0..=STYLE_DECIMALS {
        let code = if d == 0 {
            "0".to_string()
        } else {
            format!("0.{}", "0".repeat(usize::from(d)))
        };
        fmts.push_str(&format!(
            "<numFmt numFmtId=\"{}\" formatCode=\"{code}\"/>",
            164 + u16::from(d)
        ));
        xfs.push_str(&format!("<xf numFmtId=\"{}\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/>", 164 + u16::from(d)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><numFmts count=\"{}\">{fmts}</numFmts><fonts count=\"1\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"{}\">{xfs}</cellXfs></styleSheet>",
        STYLE_DECIMALS + 1,
        STYLE_DECIMALS + 2
    )
}

fn sheet_xml(c: &Computed) -> String {
    let mut rows = String::new();
    for r in 1..=c.rows {
        let mut cells = String::new();
        for col in 1..=c.cols {
            let v = c.value(r, col);
            let at = CellRef { row: r, col };
            let body = match &v {
                Value::Empty => continue,
                Value::Number(n) => {
                    let style = c
                        .format(r, col)
                        .decimals
                        .filter(|d| *d <= STYLE_DECIMALS)
                        .map_or(0, |d| 1 + usize::from(d));
                    let s = if style > 0 {
                        format!(" s=\"{style}\"")
                    } else {
                        String::new()
                    };
                    format!("<c r=\"{at}\"{s}><v>{n}</v></c>")
                }
                Value::Bool(b) => format!("<c r=\"{at}\" t=\"b\"><v>{}</v></c>", u8::from(*b)),
                Value::Text(t) => format!(
                    "<c r=\"{at}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                    xml_escape(t)
                ),
                Value::Error(e) => format!(
                    "<c r=\"{at}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                    e.code()
                ),
            };
            cells.push_str(&body);
        }
        if !cells.is_empty() {
            rows.push_str(&format!("<row r=\"{r}\">{cells}</row>"));
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{rows}</sheetData></worksheet>"
    )
}

/// The sheet as an XLSX file (bytes). `name` is the sheet name in the workbook.
pub fn xlsx(c: &Computed, name: &str) -> Vec<u8> {
    let ct = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>";
    let workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>",
        xml_escape(&sheet_name(name))
    );
    let wb_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";
    zip(&[
        ("[Content_Types].xml", ct.as_bytes().to_vec()),
        ("_rels/.rels", rels.as_bytes().to_vec()),
        ("xl/workbook.xml", workbook.into_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes().to_vec()),
        ("xl/styles.xml", styles_xml().into_bytes()),
        ("xl/worksheets/sheet1.xml", sheet_xml(c).into_bytes()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compute::compute;
    use crate::def::{CellFormat, WorksheetDef};
    use cadektor_core::model::Document;

    fn computed() -> Computed {
        let mut w = WorksheetDef::default();
        w.set_cell("A1", "Name", None).unwrap();
        w.set_cell("B1", "Value, with comma", None).unwrap();
        w.set_cell("A2", "He said \"hi\"", None).unwrap();
        w.set_cell(
            "B2",
            "=1/3",
            Some(CellFormat {
                decimals: Some(2),
                unit: "m".into(),
            }),
        )
        .unwrap();
        w.set_cell("A3", "'=cmd|' /C calc'!A0", None).unwrap();
        w.set_cell("B3", "true", None).unwrap();
        w.set_cell("C3", "-5", None).unwrap();
        w.set_cell("A4", "line\nbreak <&>", None).unwrap();
        w.set_cell("B4", "=1/0", None).unwrap();
        compute(&w, &Document::default())
    }

    #[test]
    fn csv_quotes_and_defuses() {
        let text = csv(&computed());
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines[0], "Name,\"Value, with comma\",");
        assert_eq!(lines[1], "\"He said \"\"hi\"\"\",0.33,");
        assert_eq!(
            lines[2], "'=cmd|' /C calc'!A0,TRUE,-5",
            "text that starts with = is defused. A number stays a number"
        );
        assert_eq!(lines[3], "\"line");
        assert_eq!(lines[4], "break <&>\",#DIV/0!,");
        assert_eq!(
            csv(&compute(&WorksheetDef::default(), &Document::default())),
            ""
        );
    }

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        // Read the central directory.
        let eocd = bytes.len() - 22;
        assert_eq!(&bytes[eocd..eocd + 4], &[0x50, 0x4b, 0x05, 0x06]);
        let n = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
        let mut pos = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        let mut out = vec![];
        for _ in 0..n {
            assert_eq!(&bytes[pos..pos + 4], &[0x50, 0x4b, 0x01, 0x02]);
            let crc = u32::from_le_bytes(bytes[pos + 16..pos + 20].try_into().unwrap());
            let size = u32::from_le_bytes(bytes[pos + 24..pos + 28].try_into().unwrap()) as usize;
            let nlen = u16::from_le_bytes([bytes[pos + 28], bytes[pos + 29]]) as usize;
            let off = u32::from_le_bytes(bytes[pos + 42..pos + 46].try_into().unwrap()) as usize;
            let name = String::from_utf8(bytes[pos + 46..pos + 46 + nlen].to_vec()).unwrap();
            // The local header holds the same name and then the data.
            assert_eq!(&bytes[off..off + 4], &[0x50, 0x4b, 0x03, 0x04]);
            let lnlen = u16::from_le_bytes([bytes[off + 26], bytes[off + 27]]) as usize;
            let start = off + 30 + lnlen;
            let data = bytes[start..start + size].to_vec();
            assert_eq!(crc32(&data), crc, "{name}");
            out.push((name, data));
            pos += 46 + nlen;
        }
        out
    }

    #[test]
    fn xlsx_has_the_parts_and_the_values() {
        let bytes = xlsx(&computed(), "Plant list: [1]");
        let parts = entries(&bytes);
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "[Content_Types].xml",
                "_rels/.rels",
                "xl/workbook.xml",
                "xl/_rels/workbook.xml.rels",
                "xl/styles.xml",
                "xl/worksheets/sheet1.xml"
            ]
        );
        let text = |n: &str| {
            String::from_utf8(parts.iter().find(|(m, _)| m == n).unwrap().1.clone()).unwrap()
        };
        assert!(text("xl/workbook.xml").contains("name=\"Plant list 1\""));
        let sheet = text("xl/worksheets/sheet1.xml");
        assert!(sheet.contains(
            "<c r=\"A1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Name</t></is></c>"
        ));
        assert!(
            sheet.contains("<c r=\"B2\" s=\"3\"><v>0.3333333333333333</v></c>"),
            "{sheet}"
        );
        assert!(sheet.contains("<c r=\"B3\" t=\"b\"><v>1</v></c>"));
        assert!(sheet.contains("<c r=\"C3\"><v>-5</v></c>"));
        assert!(sheet.contains("line\nbreak &lt;&amp;&gt;"));
        assert!(sheet.contains("<t>#DIV/0!</t>"));
        assert!(
            sheet.contains("<t xml:space=\"preserve\">=cmd|"),
            "text is not changed in XLSX, a text cell is not a formula"
        );
        assert!(!sheet.contains("<c r=\"C1\""), "empty cells are left out");
        for part in [
            "xl/styles.xml",
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/_rels/workbook.xml.rels",
        ] {
            assert!(text(part).starts_with("<?xml"), "{part}");
        }
        assert!(text("xl/styles.xml").contains("formatCode=\"0.00\""));
    }

    #[test]
    fn xlsx_of_an_empty_sheet_and_odd_names() {
        let bytes = xlsx(&Computed::default(), "///");
        let parts = entries(&bytes);
        assert!(String::from_utf8_lossy(&parts[2].1).contains("name=\"Sheet1\""));
        assert!(String::from_utf8_lossy(&parts[5].1).contains("<sheetData></sheetData>"));
        assert_eq!(sheet_name(&"x".repeat(50)).len(), 31);
        assert_eq!(sheet_name("  'a'  "), "a");
        assert_eq!(sheet_name("a\u{7}b"), "ab");
        assert_eq!(xml_escape("a\u{1}\"b\t"), "a&quot;b\t");
        assert_eq!(
            xml_escape("a\u{fffe}b\u{ffff}c\u{b}d\u{c}e\nf\rg"),
            "abcde\nf\rg"
        );
        assert_eq!(
            xml_escape("\u{fffd}\u{10ffff}"),
            "\u{fffd}\u{10ffff}",
            "other characters stay"
        );
    }

    #[test]
    fn xlsx_text_keeps_a_leading_minus_or_equals() {
        let mut w = WorksheetDef::default();
        w.set_cell("A1", "'-5 cm", None).unwrap();
        w.set_cell("A2", "'+x", None).unwrap();
        w.set_cell("A3", "'@home", None).unwrap();
        let c = compute(&w, &Document::default());
        let sheet = sheet_xml(&c);
        assert!(
            sheet.contains(">-5 cm</t>")
                && sheet.contains(">+x</t>")
                && sheet.contains(">@home</t>"),
            "{sheet}"
        );
        assert!(!sheet.contains("'-5"));
        // CSV still guards against formulas.
        assert!(csv(&c).starts_with("'-5 cm\n"));
    }

    #[test]
    fn many_decimals_use_the_plain_style() {
        let mut w = WorksheetDef::default();
        w.set_cell(
            "A1",
            "1.5",
            Some(CellFormat {
                decimals: Some(9),
                unit: String::new(),
            }),
        )
        .unwrap();
        w.set_cell(
            "A2",
            "2.5",
            Some(CellFormat {
                decimals: Some(0),
                unit: String::new(),
            }),
        )
        .unwrap();
        let sheet = sheet_xml(&compute(&w, &Document::default()));
        assert!(sheet.contains("<c r=\"A1\"><v>1.5</v></c>"));
        assert!(sheet.contains("<c r=\"A2\" s=\"1\"><v>2.5</v></c>"));
    }
}
