//! Single-open wrapper over `zip::ZipArchive`. Reads workbook.xml + the
//! workbook rels exactly once, exposes them as parsed data, and lets callers
//! pull arbitrary entry bodies on demand.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::error::SearchError;

#[derive(Debug, Clone)]
pub struct SheetEntry {
    pub name: String,
    /// e.g. "xl/worksheets/sheet1.xml"
    pub xml_path: String,
}

pub struct ZipIndex {
    archive: ZipArchive<File>,
    path: PathBuf,
    sheets: Vec<SheetEntry>,
}

impl ZipIndex {
    pub fn open(path: &Path) -> Result<Self, SearchError> {
        let file = File::open(path).map_err(SearchError::Io)?;
        let mut archive =
            ZipArchive::new(file).map_err(|e| SearchError::Parse(format!("zip: {e}")))?;
        let sheets = parse_sheets(&mut archive)?;
        Ok(Self {
            archive,
            path: path.to_path_buf(),
            sheets,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn sheets(&self) -> &[SheetEntry] {
        &self.sheets
    }

    /// Read a zip entry into a `String`. Returns Ok(None) if the entry does
    /// not exist (commonly the case for optional files like sharedStrings).
    pub fn read_to_string(&mut self, entry: &str) -> Result<Option<String>, SearchError> {
        match self.archive.by_name(entry) {
            Ok(mut f) => {
                let mut s = String::new();
                f.read_to_string(&mut s).map_err(SearchError::Io)?;
                Ok(Some(s))
            }
            Err(zip::result::ZipError::FileNotFound) => Ok(None),
            Err(e) => Err(SearchError::Parse(format!("zip entry {entry}: {e}"))),
        }
    }

    /// Same as `read_to_string` but returns raw bytes. Used by the sheet-xml
    /// fast-path (regex on bytes, no need to decode UTF-8 first since sheet
    /// xml is always UTF-8 and regex's `unicode(true)` works on `&str`).
    pub fn read_to_vec(&mut self, entry: &str) -> Result<Option<Vec<u8>>, SearchError> {
        match self.archive.by_name(entry) {
            Ok(mut f) => {
                let mut buf = Vec::new();
                f.read_to_end(&mut buf).map_err(SearchError::Io)?;
                Ok(Some(buf))
            }
            Err(zip::result::ZipError::FileNotFound) => Ok(None),
            Err(e) => Err(SearchError::Parse(format!("zip entry {entry}: {e}"))),
        }
    }
}

fn parse_sheets(archive: &mut ZipArchive<File>) -> Result<Vec<SheetEntry>, SearchError> {
    let mut workbook = String::new();
    archive
        .by_name("xl/workbook.xml")
        .map_err(|e| SearchError::Parse(format!("workbook.xml: {e}")))?
        .read_to_string(&mut workbook)
        .map_err(SearchError::Io)?;
    let re_sheet = regex::Regex::new(r#"<sheet[^>]*name="([^"]+)"[^>]*r:id="(rId\d+)""#).unwrap();
    let mut rids: Vec<(String, String)> = re_sheet
        .captures_iter(&workbook)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    if rids.is_empty() {
        // Order-of-attributes variant.
        let re_alt = regex::Regex::new(r#"<sheet[^>]*r:id="(rId\d+)"[^>]*name="([^"]+)""#).unwrap();
        rids = re_alt
            .captures_iter(&workbook)
            .map(|c| (c[2].to_string(), c[1].to_string()))
            .collect();
    }

    let mut rels = String::new();
    if let Ok(mut f) = archive.by_name("xl/_rels/workbook.xml.rels") {
        f.read_to_string(&mut rels).map_err(SearchError::Io)?;
    }
    let re_rel =
        regex::Regex::new(r#"<Relationship[^>]*Id="(rId\d+)"[^>]*Target="([^"]+)""#).unwrap();
    let mut rid_to_target: HashMap<String, String> = HashMap::new();
    for cap in re_rel.captures_iter(&rels) {
        rid_to_target.insert(cap[1].to_string(), cap[2].to_string());
    }

    Ok(rids
        .into_iter()
        .filter_map(|(name, rid)| {
            rid_to_target.get(&rid).map(|t| SheetEntry {
                name,
                xml_path: format!("xl/{}", t.trim_start_matches('/')),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::Workbook;
    use tempfile::TempDir;

    fn tiny_workbook(path: &std::path::Path) {
        let mut wb = Workbook::new();
        let s = wb.add_worksheet().set_name("Alpha").unwrap();
        s.write_string(0, 0, "x").unwrap();
        let s2 = wb.add_worksheet().set_name("Beta").unwrap();
        s2.write_string(0, 0, "y").unwrap();
        wb.save(path).unwrap();
    }

    #[test]
    fn open_lists_sheets_in_workbook_order() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("tiny.xlsx");
        tiny_workbook(&p);
        let idx = ZipIndex::open(&p).unwrap();
        let names: Vec<_> = idx.sheets().iter().map(|s| s.name.clone()).collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn open_resolves_rid_to_sheet_xml_path() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("tiny.xlsx");
        tiny_workbook(&p);
        let idx = ZipIndex::open(&p).unwrap();
        for s in idx.sheets() {
            assert!(s.xml_path.starts_with("xl/worksheets/"));
            assert!(s.xml_path.ends_with(".xml"));
        }
    }

    #[test]
    fn read_to_string_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("tiny.xlsx");
        tiny_workbook(&p);
        let mut idx = ZipIndex::open(&p).unwrap();
        assert!(idx
            .read_to_string("xl/no-such-entry.xml")
            .unwrap()
            .is_none());
    }
}
