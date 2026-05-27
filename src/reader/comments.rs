//! Cell comments — parses `xl/comments*.xml` entries via direct XML scanning
//! (calamine's comments API moves across versions; doing it ourselves keeps
//! us pinned to the small public surface of `zip` + `regex`).

use crate::error::SearchError;
use crate::reader::zip_index::ZipIndex;

pub fn extract(index: &mut ZipIndex) -> Result<Vec<(String, String, String)>, SearchError> {
    let sheets: Vec<(String, String)> = index
        .sheets()
        .iter()
        .map(|s| (s.name.clone(), s.xml_path.clone()))
        .collect();
    let mut out = Vec::new();
    let re_comment = regex::Regex::new(
        r#"<comment[^>]*ref="([^"]+)"[^>]*>([\s\S]*?)</comment>"#
    ).unwrap();
    let re_t = regex::Regex::new(r#"<t[^>]*>([\s\S]*?)</t>"#).unwrap();
    let re_target = regex::Regex::new(r#"Target="([^"]*comments[^"]+\.xml)""#).unwrap();

    for (sheet_name, sheet_xml) in sheets {
        let rels_path = sheet_xml.replacen("worksheets/", "worksheets/_rels/", 1) + ".rels";
        let Some(rels) = index.read_to_string(&rels_path)? else { continue; };
        let Some(cap) = re_target.captures(&rels) else { continue; };
        let target = cap[1].to_string();
        let comments_path = if let Some(stripped) = target.strip_prefix("../") {
            format!("xl/{stripped}")
        } else {
            format!("xl/worksheets/{target}")
        };
        let Some(xml) = index.read_to_string(&comments_path)? else { continue; };

        for cap in re_comment.captures_iter(&xml) {
            let cell = cap[1].to_string();
            let body = &cap[2];
            let text: String = re_t
                .captures_iter(body)
                .map(|c| xml_unescape(&c[1]))
                .collect::<Vec<_>>()
                .join("");
            if !text.is_empty() {
                out.push((sheet_name.clone(), cell, text));
            }
        }
    }
    Ok(out)
}

pub fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
