// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Scans every local filesystem, or the directories given, and prints the
//! largest items, the cleanup findings, time taken and peak memory.
//!
//! `cargo run --release -p masthead-disk --example disk_scan [dir...]`

use masthead_disk::{docker::Docker, findings, mounts::Mount, scan, time, tree::Node};

fn main() {
    let roots: Vec<Mount> = std::env::args().skip(1).map(|a| Mount::dir(a.into())).collect();
    let home = findings::Home::from_env().expect("HOME is set");
    let opts = scan::Options { downloads: Some(home.downloads.clone()) };
    let progress = masthead_disk::Progress::default();
    let s = masthead_disk::scan(&roots, &opts, &progress);
    println!(
        "{} files, {} dirs, {} unreadable, {:.1} s, {} nodes kept",
        progress.files.load(std::sync::atomic::Ordering::Relaxed),
        progress.dirs.load(std::sync::atomic::Ordering::Relaxed),
        progress.denied.load(std::sync::atomic::Ordering::Relaxed),
        s.elapsed.as_secs_f64(),
        s.tree.count()
    );
    print_tree(&s.tree, 0, 3);
    let now = time::now();
    let mut groups = findings::from_scan(&s, &home, now);
    if let Some(d) = Docker::find() {
        match d.df() {
            Ok(df) => groups.splice(0..0, findings::from_docker(&df, d.engine, now)).for_each(drop),
            Err(e) => println!("{}: {e}", d.engine),
        }
    }
    for g in &groups {
        println!("\n{} — {:.1} GB in {} items", g.title, g.size() as f64 / 1e9, g.items.len());
        for i in g.items.iter().take(5) {
            println!("  {:>8.2} GB {} {} — {}", i.size as f64 / 1e9, if i.stale { "stale" } else { "     " }, i.title, i.detail);
        }
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for l in status.lines().filter(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS")) {
        println!("{l}");
    }
}

fn print_tree(n: &Node, depth: usize, max: usize) {
    println!("{:indent$}{:>9.2} GB  {}", "", n.size as f64 / 1e9, n.name, indent = depth * 2);
    if depth < max {
        for c in n.children.iter().take(4) {
            print_tree(c, depth + 1, max);
        }
    }
}
