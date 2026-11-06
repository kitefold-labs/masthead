// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Running a scan in a child process.
//!
//! The walk allocates and frees far more than the tree it keeps, and glibc
//! holds on to freed memory, so a scan run in the window would leave it
//! about 60 MB larger for as long as it stays open. A child process returns
//! all of it when it exits. The child reports progress on stderr and sends
//! the tree on stdout as a flat pre-order list, which needs no recursion to
//! read back however deep the folders go.

use crate::scan::{CacheDir, CacheKind, Download, Options, Progress, Scan};
use crate::tree::{Kind, Node};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::Duration;

/// The argument that makes a program call [`serve`].
pub const ARG: &str = "--disk-scan";

/// One node: name, kind, size, files, newest, number of children.
#[derive(Debug, Serialize, Deserialize)]
struct Flat(Box<str>, u8, u64, u64, i64, u32);

#[derive(Debug, Serialize, Deserialize)]
struct Report {
    nodes: Vec<Flat>,
    caches: Vec<(PathBuf, u64, i64, bool)>,
    downloads: Vec<(PathBuf, u64, i64)>,
    elapsed_ms: u64,
}

/// The child side: scans, printing progress to stderr, then writes the
/// report to stdout. `downloads` is the folder to look for old downloads in.
pub fn serve(downloads: Option<PathBuf>) -> std::io::Result<()> {
    let progress = Arc::new(Progress::default());
    let done = Arc::new(AtomicBool::new(false));
    let reporter = {
        let (progress, done) = (progress.clone(), done.clone());
        std::thread::spawn(move || {
            let mut err = std::io::stderr().lock();
            while !done.load(Relaxed) {
                let current = progress.current.lock().map(|c| c.replace(['\t', '\n'], " ")).unwrap_or_default();
                let line = format!(
                    "{}\t{}\t{}\t{}\t{current}\n",
                    progress.files.load(Relaxed),
                    progress.dirs.load(Relaxed),
                    progress.bytes.load(Relaxed),
                    progress.denied.load(Relaxed)
                );
                if err.write_all(line.as_bytes()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    };
    let s = crate::scan(&[], &Options { downloads }, &progress);
    done.store(true, Relaxed);
    reporter.join().ok();
    let report = Report {
        nodes: flatten(&s.tree),
        caches: s.caches.into_iter().map(|c| (c.path, c.size, c.newest, c.kind == CacheKind::NodeModules)).collect(),
        downloads: s.downloads.into_iter().map(|d| (d.path, d.size, d.mtime)).collect(),
        elapsed_ms: s.elapsed.as_millis() as u64,
    };
    let mut out = BufWriter::new(std::io::stdout().lock());
    serde_json::to_writer(&mut out, &report)?;
    out.flush()
}

/// The parent side: runs `exe` with [`ARG`], mirrors its progress into
/// `progress`, and kills it if `progress.cancel` is set. Blocks until done.
pub fn run(exe: &Path, opts: &Options, progress: &Arc<Progress>) -> Result<Scan, String> {
    let mut cmd = Command::new(exe);
    cmd.arg(ARG).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(d) = &opts.downloads {
        cmd.arg(d);
    }
    let mut child = cmd.spawn().map_err(|e| format!("Could not start the scan: {e}"))?;
    let pid = i32::try_from(child.id()).ok().and_then(rustix::process::Pid::from_raw);
    let finished = Arc::new(AtomicBool::new(false));

    let stderr = child.stderr.take().ok_or("no stderr")?;
    let reader = {
        let progress = progress.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let mut f = line.splitn(5, '\t');
                let mut num = || f.next().and_then(|v| v.parse::<u64>().ok());
                let (Some(files), Some(dirs), Some(bytes), Some(denied)) = (num(), num(), num(), num()) else { continue };
                progress.files.store(files, Relaxed);
                progress.dirs.store(dirs, Relaxed);
                progress.bytes.store(bytes, Relaxed);
                progress.denied.store(denied, Relaxed);
                if let (Some(c), Ok(mut cur)) = (f.next(), progress.current.lock()) {
                    c.clone_into(&mut cur);
                }
            }
        })
    };
    let watcher = {
        let (progress, finished) = (progress.clone(), finished.clone());
        std::thread::spawn(move || {
            while !finished.load(Relaxed) {
                if progress.cancel.load(Relaxed) {
                    if let Some(pid) = pid {
                        rustix::process::kill_process(pid, rustix::process::Signal::KILL).ok();
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let report: Result<Report, _> = serde_json::from_reader(BufReader::new(stdout));
    finished.store(true, Relaxed);
    let status = child.wait();
    reader.join().ok();
    watcher.join().ok();
    let cancelled = progress.cancel.load(Relaxed);
    if cancelled {
        return Ok(Scan {
            tree: Node::new("This computer", Kind::Computer),
            caches: Vec::new(),
            downloads: Vec::new(),
            elapsed: Duration::ZERO,
            cancelled,
        });
    }
    let report = report.map_err(|e| format!("The scan failed ({}): {e}", status.map_or_else(|e| e.to_string(), |s| s.to_string())))?;
    let tree = unflatten(report.nodes).ok_or("The scan sent an incomplete tree.")?;
    Ok(Scan {
        tree,
        caches: report
            .caches
            .into_iter()
            .map(|(path, size, newest, nm)| CacheDir {
                path,
                size,
                newest,
                kind: if nm { CacheKind::NodeModules } else { CacheKind::Tagged },
            })
            .collect(),
        downloads: report.downloads.into_iter().map(|(path, size, mtime)| Download { path, size, mtime }).collect(),
        elapsed: Duration::from_millis(report.elapsed_ms),
        cancelled: false,
    })
}

fn kind_code(k: Kind) -> u8 {
    match k {
        Kind::Computer => 0,
        Kind::Mount => 1,
        Kind::Dir => 2,
        Kind::File => 3,
        Kind::Other => 4,
        Kind::Unscanned => 5,
    }
}

fn kind_from(c: u8) -> Option<Kind> {
    Some(match c {
        0 => Kind::Computer,
        1 => Kind::Mount,
        2 => Kind::Dir,
        3 => Kind::File,
        4 => Kind::Other,
        5 => Kind::Unscanned,
        _ => return None,
    })
}

fn flatten(root: &Node) -> Vec<Flat> {
    let mut out = Vec::with_capacity(root.count());
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        out.push(Flat(n.name.clone(), kind_code(n.kind), n.size, n.files, n.newest, n.children.len() as u32));
        stack.extend(n.children.iter().rev());
    }
    out
}

fn unflatten(flat: Vec<Flat>) -> Option<Node> {
    // Each open node with the number of children it still waits for.
    let mut stack: Vec<(Node, u32)> = Vec::new();
    let mut root = None;
    for Flat(name, kind, size, files, newest, kids) in flat {
        let node = Node { name, kind: kind_from(kind)?, size, files, newest, children: Vec::with_capacity(kids as usize) };
        stack.push((node, kids));
        // Close every node whose children are all in.
        while let Some((_, 0)) = stack.last() {
            let (done, _) = stack.pop()?;
            match stack.last_mut() {
                Some((parent, left)) => {
                    parent.children.push(done);
                    *left -= 1;
                }
                None => root = Some(done),
            }
        }
    }
    if stack.is_empty() { root } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_round_trip() {
        let mut deep = Node::file("leaf", 1, 1);
        for i in 0..500 {
            let mut d = Node::new(format!("d{i}"), Kind::Dir);
            d.children.push(deep);
            d.sum_children(0);
            deep = d;
        }
        let mut root = Node::new("This computer", Kind::Computer);
        root.children = vec![deep, Node::file("f", 9, 2), Node::new("x", Kind::Other)];
        root.sum_children(0);
        let flat = flatten(&root);
        assert_eq!(flat.len(), root.count());
        let json = serde_json::to_vec(&flat).unwrap();
        let back = unflatten(serde_json::from_slice(&json).unwrap()).unwrap();
        assert_eq!(back, root);
        assert_eq!(unflatten(flatten(&root).into_iter().take(10).collect()), None);
    }
}
