//! Static search configuration. CLI parsing lives in main.rs; this module just
//! owns the data shape and small derived defaults.

use std::path::PathBuf;

use bitflags::bitflags;
use globset::Glob;

use crate::matcher::Pattern;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct LayerSet: u8 {
        const DISPLAY = 0b0001;
        const CACHED  = 0b0010;
        const COMMENT = 0b0100;
        const FORMULA = 0b1000;
    }
}

impl LayerSet {
    /// Default match pool per spec §3: display + cached + comments. Formula text
    /// is *not* in the default — `--formula` adds it. Hidden sheets/rows/cols are
    /// included via `include_hidden` (separate axis from layers).
    pub fn defaults() -> Self {
        LayerSet::DISPLAY | LayerSet::CACHED | LayerSet::COMMENT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer { Display, Cached, Formula, Comment }

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Layer::Display => "display",
            Layer::Cached => "cached",
            Layer::Formula => "formula",
            Layer::Comment => "comment",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode { Pretty, Json, CountOnly, FilesOnly }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice { Auto, Always, Never }

#[derive(Debug, Clone)]
pub struct Config {
    pub pattern: Pattern,
    pub paths: Vec<PathBuf>,
    pub layers: LayerSet,
    pub include_hidden: bool,
    pub sheet_glob: Option<Glob>,
    pub file_glob: Option<Glob>,
    pub threads: usize,
    pub output: OutputMode,
    pub color: ColorChoice,
    pub invert: bool,
    /// True when --layers was passed: print a `[display]` tag even on display matches.
    pub force_layer_tag: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{CaseMode, Pattern};

    #[test]
    fn default_layers_includes_display_cached_comment_not_formula() {
        let l = LayerSet::defaults();
        assert!(l.contains(LayerSet::DISPLAY));
        assert!(l.contains(LayerSet::CACHED));
        assert!(l.contains(LayerSet::COMMENT));
        assert!(!l.contains(LayerSet::FORMULA));
    }

    #[test]
    fn layer_as_str() {
        assert_eq!(Layer::Display.as_str(), "display");
        assert_eq!(Layer::Formula.as_str(), "formula");
    }

    #[test]
    fn config_round_trip() {
        let p = Pattern::compile("foo", CaseMode::Smart, false, false).unwrap();
        let cfg = Config {
            pattern: p,
            paths: vec![PathBuf::from(".")],
            layers: LayerSet::defaults(),
            include_hidden: true,
            sheet_glob: None,
            file_glob: None,
            threads: 4,
            output: OutputMode::Pretty,
            color: ColorChoice::Auto,
            invert: false,
            force_layer_tag: false,
        };
        assert_eq!(cfg.layers, LayerSet::defaults());
        assert_eq!(cfg.output, OutputMode::Pretty);
    }
}
