//! Static search configuration. CLI parsing lives in main.rs; this module just
//! owns the data shape and small derived defaults.

use bitflags::bitflags;

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
pub enum Layer {
    Display,
    Cached,
    Formula,
    Comment,
}

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
pub enum OutputMode {
    Pretty,
    Json,
    CountOnly,
    FilesOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
