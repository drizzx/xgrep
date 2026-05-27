//! File-level parallel search. Each xlsx file is processed by a single thread
//! (xlsx zip + shared-strings make in-file parallelism a net loss); rayon's
//! work-stealing pool distributes files across threads.

use std::path::PathBuf;

use rayon::iter::{IntoParallelIterator, ParallelIterator};
use rayon::ThreadPoolBuilder;

use crate::matcher::Pattern;
use crate::reader::ReaderOptions;
use crate::{search_file, FileBlock};

pub fn run_search(
    paths: Vec<PathBuf>,
    pattern: &Pattern,
    reader_opts: &ReaderOptions,
    invert: bool,
    threads: usize,
) -> Vec<FileBlock> {
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .expect("rayon pool");
    pool.install(|| {
        paths
            .into_par_iter()
            .map(|p| search_file(&p, pattern, reader_opts, invert))
            .collect()
    })
}
