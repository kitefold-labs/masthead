// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The parallel directory walk.
//!
//! Each filesystem is walked on its own device only, so `/proc`, `/sys`,
//! network mounts and container overlays are skipped. Sizes are allocated
//! blocks, like `du`, and a file with several hard links counts once. A
//! directory's small items are folded as soon as it is finished, which keeps
//! memory proportional to what is shown rather than to the file count.

use crate::mounts::{self, Mount};
use crate::tree::{Kind, Node};
use rayon::prelude::*;
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

/// Items smaller than this are folded into "N smaller items".
pub const MIN_ITEM: u64 = 4 << 20;
/// At most this many items are kept per directory; the rest are folded.
pub const MAX_ITEMS: usize = 300;
/// Cache directories smaller than this are not worth listing.
const MIN_CACHE: u64 = 32 << 20;
/// Downloads at least this big and this old are listed.
const BIG_DOWNLOAD: u64 = 100 << 20;
const OLD_DOWNLOAD: i64 = 30 * 86_400;

/// Live counters a UI polls while a scan runs.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    /// Directories the user may not read.
    pub denied: AtomicU64,
    pub cancel: AtomicBool,
    /// A directory being read, updated now and then.
    pub current: Mutex<String>,
}

/// A directory that holds regenerable data, found during the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheDir {
    pub path: PathBuf,
    pub size: u64,
    pub newest: i64,
    pub kind: CacheKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheKind {
    /// Holds a `CACHEDIR.TAG` (Cargo's `target/`, many tool caches).
    Tagged,
    /// A `node_modules` directory.
    NodeModules,
}

/// A large file under the Downloads folder that has not changed in a month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub path: PathBuf,
    pub size: u64,
    pub mtime: i64,
}

#[derive(Debug)]
pub struct Scan {
    /// [`Kind::Computer`] with one [`Kind::Mount`] per filesystem.
    pub tree: Node,
    /// Outermost cache directories, largest first.
    pub caches: Vec<CacheDir>,
    pub downloads: Vec<Download>,
    pub elapsed: Duration,
    pub cancelled: bool,
}

/// Where the scan should look for downloads.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub downloads: Option<PathBuf>,
}

struct Ctx<'a> {
    progress: &'a Progress,
    opts: &'a Options,
    now: i64,
    /// (device, inode) of files with more than one link, so each counts once.
    links: Mutex<HashSet<(u64, u64)>>,
    caches: Mutex<Vec<CacheDir>>,
    downloads: Mutex<Vec<Download>>,
    last_note: Mutex<Instant>,
    /// Mount points not to descend into; each root is walked on its own.
    mount_points: HashSet<PathBuf>,
}

/// Scans `roots`, or every local filesystem when empty.
pub fn scan(roots: &[Mount], opts: &Options, progress: &Progress) -> Scan {
    let started = Instant::now();
    let roots = if roots.is_empty() { mounts::local() } else { roots.to_vec() };
    let ctx = Ctx {
        progress,
        opts,
        now: crate::time::now(),
        links: Mutex::new(HashSet::new()),
        caches: Mutex::new(Vec::new()),
        downloads: Mutex::new(Vec::new()),
        last_note: Mutex::new(Instant::now()),
        mount_points: mounts::points(),
    };
    // Directory reads block on the disk, so more threads than cores helps on
    // SSDs; rayon's default of one per core leaves an NVMe drive idle.
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get).clamp(4, 16);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).stack_size(16 << 20).build();
    let walk_all = || roots.par_iter().map(|m| walk_mount(&ctx, m)).collect::<Vec<_>>();
    let mounts = match &pool {
        Ok(p) => p.install(walk_all),
        Err(_) => walk_all(),
    };
    let mut tree = Node::new("This computer", Kind::Computer);
    tree.children = mounts;
    tree.sum_children(0);
    let mut caches = ctx.caches.into_inner().unwrap_or_default();
    caches.sort_by_key(|x| std::cmp::Reverse(x.size));
    let mut downloads = ctx.downloads.into_inner().unwrap_or_default();
    downloads.sort_by_key(|x| std::cmp::Reverse(x.size));
    Scan { tree, caches, downloads, elapsed: started.elapsed(), cancelled: progress.cancel.load(Relaxed) }
}

fn walk_mount(ctx: &Ctx<'_>, m: &Mount) -> Node {
    let Ok(md) = std::fs::symlink_metadata(&m.path) else {
        return Node::new(m.path.to_string_lossy(), Kind::Mount);
    };
    let mut node = walk(ctx, &m.path, &OsString::new(), md.dev(), false);
    node.name = m.path.to_string_lossy().into();
    node.kind = Kind::Mount;
    // A real mount (not a directory scanned on its own) shows the space the
    // filesystem reports as used but the walk could not read.
    if !m.fstype.is_empty()
        && let Some(used) = mounts::used(&m.path)
    {
        let missing = used.saturating_sub(node.size);
        // Under 1% is filesystem metadata and rounding, not worth a block.
        if missing > used / 100 && !ctx.progress.cancel.load(Relaxed) {
            let mut u = Node::new("Not readable without root", Kind::Unscanned);
            u.size = missing;
            node.children.push(u);
            node.sum_children(md.blocks() * 512);
        }
    }
    node
}

fn walk(ctx: &Ctx<'_>, dir: &Path, name: &OsString, dev: u64, in_cache: bool) -> Node {
    let p = ctx.progress;
    let mut node = Node::new(name.to_string_lossy(), Kind::Dir);
    if p.cancel.load(Relaxed) {
        return node;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        p.denied.fetch_add(1, Relaxed);
        return node;
    };
    p.dirs.fetch_add(1, Relaxed);
    note(ctx, dir);

    let mut other = Node::new("", Kind::Other);
    let mut subdirs: Vec<(PathBuf, OsString)> = Vec::new();
    let mut tagged = false;
    let in_downloads = ctx.opts.downloads.as_deref().is_some_and(|d| dir.starts_with(d));
    for e in entries.flatten() {
        let fname = e.file_name();
        // DirEntry::metadata does not follow symlinks.
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            // Another filesystem mounted here is scanned as its own root, or
            // is virtual (/proc, /sys, overlays).
            let path = e.path();
            if md.dev() == dev && !ctx.mount_points.contains(&path) {
                subdirs.push((path, fname));
            }
            continue;
        }
        if fname == "CACHEDIR.TAG" && crate::scan::has_cache_signature(&e.path()) {
            tagged = true;
        }
        if md.nlink() > 1 && !ctx.links.lock().is_ok_and(|mut s| s.insert((md.dev(), md.ino()))) {
            continue;
        }
        let size = md.blocks() * 512;
        let mtime = md.mtime();
        p.files.fetch_add(1, Relaxed);
        p.bytes.fetch_add(size, Relaxed);
        if in_downloads
            && size >= BIG_DOWNLOAD
            && ctx.now - mtime >= OLD_DOWNLOAD
            && let Ok(mut d) = ctx.downloads.lock()
        {
            d.push(Download { path: e.path(), size, mtime });
        }
        if size >= MIN_ITEM {
            node.children.push(Node::file(fname.to_string_lossy(), size, mtime));
        } else {
            other.size += size;
            other.files += 1;
            other.newest = other.newest.max(mtime);
        }
    }

    let cache_kind = if in_cache {
        None
    } else if tagged {
        Some(CacheKind::Tagged)
    } else if *name == "node_modules" {
        Some(CacheKind::NodeModules)
    } else {
        None
    };
    let below_cache = in_cache || cache_kind.is_some();
    let subs: Vec<Node> = subdirs.into_par_iter().map(|(path, n)| walk(ctx, &path, &n, dev, below_cache)).collect();
    node.children.extend(subs);
    let own = std::fs::symlink_metadata(dir).map_or(0, |m| m.blocks() * 512);
    node.sum_children(own);
    node.size += other.size;
    node.files += other.files;
    node.newest = node.newest.max(other.newest);
    node.fold(MAX_ITEMS, MIN_ITEM, other);

    if let Some(kind) = cache_kind
        && node.size >= MIN_CACHE
        && let Ok(mut c) = ctx.caches.lock()
    {
        c.push(CacheDir { path: dir.to_path_buf(), size: node.size, newest: node.newest, kind });
    }
    node
}

/// The first line a `CACHEDIR.TAG` must start with
/// (<https://bford.info/cachedir/>).
const SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

/// True if `tag` is a cache directory tag, not just a file with that name.
pub fn has_cache_signature(tag: &Path) -> bool {
    use std::io::Read;
    let mut buf = [0u8; SIGNATURE.len()];
    std::fs::File::open(tag).and_then(|mut f| f.read_exact(&mut buf)).is_ok() && buf == SIGNATURE
}

/// Records the directory being read, at most four times a second.
fn note(ctx: &Ctx<'_>, dir: &Path) {
    if let Ok(mut last) = ctx.last_note.try_lock()
        && last.elapsed() >= Duration::from_millis(250)
    {
        *last = Instant::now();
        if let Ok(mut c) = ctx.progress.current.lock() {
            c.clear();
            c.push_str(&dir.to_string_lossy());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![1u8; len]).unwrap();
    }

    fn scan_dir(root: &Path, opts: &Options) -> Scan {
        let m = Mount::dir(root.to_path_buf());
        scan(&[m], opts, &Progress::default())
    }

    #[test]
    fn sizes_folding_caches_and_links() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        write(&r.join("big/a.bin"), 6 << 20);
        for i in 0..50 {
            write(&r.join(format!("small/f{i}")), 100_000);
        }
        fs::create_dir_all(r.join("proj/target")).unwrap();
        fs::write(r.join("proj/target/CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
        // A file with the name but not the signature marks nothing.
        write(&r.join("fake/CACHEDIR.TAG"), 43);
        write(&r.join("fake/data"), 40 << 20);
        write(&r.join("proj/target/debug/out"), 40 << 20);
        write(&r.join("proj/target/debug/nested/node_modules/x"), 33 << 20);
        write(&r.join("web/node_modules/pkg/index.js"), 34 << 20);
        fs::hard_link(r.join("big/a.bin"), r.join("big/a-link.bin")).unwrap();
        std::os::unix::fs::symlink(r.join("big"), r.join("loop")).unwrap();

        let s = scan_dir(r, &Options::default());
        let mount = &s.tree.children[0];
        assert_eq!(mount.kind, Kind::Mount);
        let at = |p: &str| s.tree.find(&r.join(p)).and_then(|i| s.tree.at(&i).cloned());

        let big = at("big").unwrap();
        // The hard link counts once.
        assert!(big.size >= 6 << 20 && big.size < 7 << 20, "{}", big.size);
        assert_eq!(big.files, 1);
        // 50 small files fold into one node.
        let small = at("small").unwrap();
        assert_eq!(small.files, 50);
        assert_eq!(small.children.len(), 1);
        assert_eq!(small.children[0].kind, Kind::Other);
        assert_eq!(&*small.children[0].name, "50 smaller items");

        // Outermost caches only: target/ and web/node_modules, not the
        // node_modules inside target/.
        let found: Vec<_> = s.caches.iter().map(|c| (c.path.strip_prefix(r).unwrap().to_owned(), c.kind)).collect();
        assert_eq!(found, [(PathBuf::from("proj/target"), CacheKind::Tagged), (PathBuf::from("web/node_modules"), CacheKind::NodeModules)]);
        // The symlink counts as a file, as in du; it is not followed.
        assert_eq!(mount.files, 1 + 50 + 3 + 1 + 1 + 2);
    }

    #[test]
    fn old_big_downloads() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        let f = r.join("Downloads/disk.iso");
        write(&f, (100 << 20) + 1);
        write(&r.join("Downloads/new.iso"), (100 << 20) + 1);
        let old = std::time::SystemTime::now() - Duration::from_hours(40 * 24);
        fs::File::options().write(true).open(&f).unwrap().set_modified(old).unwrap();
        let s = scan_dir(r, &Options { downloads: Some(r.join("Downloads")) });
        assert_eq!(s.downloads.len(), 1);
        assert_eq!(s.downloads[0].path, f);
    }

    #[test]
    fn cancel_stops_early() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("a/b/c"), 10);
        let p = Progress::default();
        p.cancel.store(true, Relaxed);
        let m = Mount::dir(tmp.path().to_path_buf());
        let s = scan(&[m], &Options::default(), &p);
        assert!(s.cancelled);
        assert_eq!(s.tree.files, 0);
    }
}
