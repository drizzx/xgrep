//! Cell comments — parses `xl/comments*.xml` entries via the shared byte
//! scanner in `xml_scan`. Format is stable across calamine versions; we
//! parse it ourselves to stay pinned to a small public surface (zip + regex
//! for pattern matching only).

use std::ops::ControlFlow;

use crate::error::SearchError;
use crate::reader::xml_scan;
use crate::reader::zip_index::ZipIndex;

pub fn extract(index: &mut ZipIndex) -> Result<Vec<(String, String, String)>, SearchError> {
    let sheets: Vec<(String, String)> = index
        .sheets()
        .iter()
        .map(|s| (s.name.clone(), s.xml_path.clone()))
        .collect();
    let mut out = Vec::new();

    for (sheet_name, sheet_xml) in sheets {
        let rels_path = sheet_xml.replacen("worksheets/", "worksheets/_rels/", 1) + ".rels";
        let Some(rels) = index.read_to_string(&rels_path)? else {
            continue;
        };

        // Find the first Relationship whose Target contains "comments" (the
        // sheet's comments xml). Multiple Relationship elements can exist; we
        // only care about the comments one. Rels elements are self-closing
        // (<Relationship ... />) so we use for_each_self_closing_tag.
        let mut comments_target: Option<String> = None;
        xml_scan::for_each_self_closing_tag(rels.as_bytes(), "Relationship", |attrs| {
            if let Some(target) = xml_scan::attr(attrs, "Target") {
                let target_str = std::str::from_utf8(target).unwrap_or("");
                if target_str.contains("comments") {
                    comments_target = Some(target_str.to_string());
                    return ControlFlow::Break(());
                }
            }
            ControlFlow::Continue(())
        });
        let Some(target) = comments_target else { continue; };

        let comments_path = if let Some(stripped) = target.strip_prefix("../") {
            format!("xl/{stripped}")
        } else {
            format!("xl/worksheets/{target}")
        };
        let Some(xml) = index.read_to_string(&comments_path)? else {
            continue;
        };

        xml_scan::for_each_tag(xml.as_bytes(), "comment", |attrs, body| {
            let cell = match xml_scan::attr(attrs, "ref") {
                Some(r) => match std::str::from_utf8(r) {
                    Ok(s) => s.to_string(),
                    Err(_) => return ControlFlow::Continue(()),
                },
                None => return ControlFlow::Continue(()),
            };
            let mut text = String::new();
            xml_scan::for_each_tag(body, "t", |_t_attrs, t_body| {
                text.push_str(&xml_scan::xml_unescape(t_body));
                ControlFlow::Continue(())
            });
            if !text.is_empty() {
                out.push((sheet_name.clone(), cell, text));
            }
            ControlFlow::Continue(())
        });
    }
    Ok(out)
}
