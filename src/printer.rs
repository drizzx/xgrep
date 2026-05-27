//! Output sinks. Each call to `print_block` writes one file's events to the
//! provided writer atomically — callers should hold a single writer lock to
//! guarantee no other thread interleaves.

use std::io::{self, Write};

use crate::config::{ColorChoice, OutputMode};
use crate::{FileBlock, MatchEvent};

pub fn print_block(
    block: &FileBlock,
    out: &mut impl Write,
    mode: OutputMode,
    color: ColorChoice,
    force_layer_tag: bool,
) -> io::Result<()> {
    match mode {
        OutputMode::Pretty => print_pretty(block, out, color, force_layer_tag),
        OutputMode::Json => print_json(block, out),
        OutputMode::CountOnly => print_count(block, out),
        OutputMode::FilesOnly => print_files_with_matches(block, out),
    }
}

fn print_pretty(
    block: &FileBlock,
    out: &mut impl Write,
    _color: ColorChoice,  // wired up in T18
    force_layer_tag: bool,
) -> io::Result<()> {
    let has_match = block.events.iter().any(|e| matches!(e, MatchEvent::Match { .. }));
    if !has_match { return Ok(()); }

    for ev in &block.events {
        match ev {
            MatchEvent::FileBegin { path } => {
                writeln!(out, "{}", path.display())?;
            }
            MatchEvent::Match { sheet, cell, layer, text, submatches, .. } => {
                let offset = submatches.first().map(|s| s.start + 1).unwrap_or(1);
                let tag = if layer != "display" || force_layer_tag {
                    format!(" [{}]", layer)
                } else {
                    String::new()
                };
                writeln!(out, "  {sheet}!{cell}:{offset}: {text}{tag}")?;
            }
            MatchEvent::FileEnd { .. } => {}
            MatchEvent::Error { message, .. } => {
                writeln!(out, "  ERROR: {message}")?;
            }
        }
    }
    Ok(())
}

fn print_json(block: &FileBlock, out: &mut impl Write) -> io::Result<()> {
    for ev in &block.events {
        serde_json::to_writer(&mut *out, ev).map_err(io_other)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

fn print_count(block: &FileBlock, out: &mut impl Write) -> io::Result<()> {
    let mut count = 0u64;
    let mut path = None;
    for ev in &block.events {
        match ev {
            MatchEvent::FileBegin { path: p } => path = Some(p.clone()),
            MatchEvent::Match { .. } => count += 1,
            _ => {}
        }
    }
    if let (Some(p), n) = (path, count) {
        if n > 0 {
            writeln!(out, "{}:{}", p.display(), n)?;
        }
    }
    Ok(())
}

fn print_files_with_matches(block: &FileBlock, out: &mut impl Write) -> io::Result<()> {
    let has_match = block.events.iter().any(|e| matches!(e, MatchEvent::Match { .. }));
    if !has_match { return Ok(()); }
    if let Some(MatchEvent::FileBegin { path }) = block.events.first() {
        writeln!(out, "{}", path.display())?;
    }
    Ok(())
}

fn io_other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
