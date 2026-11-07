// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! The size tree a scan produces.
//!
//! Children are sorted largest first. Small items are folded into one
//! [`Kind::Other`] node per directory, so the tree stays a few hundred
//! thousand nodes on a multi-terabyte disk.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The tree's root, holding one [`Kind::Mount`] per filesystem.
    Computer,
    /// A filesystem's mount point. Its name is the absolute path.
    Mount,
    Dir,
    File,
    /// Items too small to list, folded together.
    Other,
    /// Space a filesystem reports as used that the scan could not see:
    /// directories the user cannot read, and filesystem metadata.
    Unscanned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: Box<str>,
    pub kind: Kind,
    /// Allocated bytes, including everything below.
    pub size: u64,
    /// Files below, including folded ones.
    pub files: u64,
    /// Newest modification time below, in seconds since the epoch.
    pub newest: i64,
    /// Largest first.
    pub children: Vec<Node>,
}

impl Node {
    pub fn new(name: impl Into<Box<str>>, kind: Kind) -> Self {
        Self { name: name.into(), kind, size: 0, files: 0, newest: 0, children: Vec::new() }
    }

    pub fn file(name: impl Into<Box<str>>, size: u64, mtime: i64) -> Self {
        Self { name: name.into(), kind: Kind::File, size, files: 1, newest: mtime, children: Vec::new() }
    }

    /// True for nodes a user can open: directories and mounts.
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Computer | Kind::Mount | Kind::Dir)
    }

    /// Sets size, file count and newest time from the children, and sorts them.
    pub fn sum_children(&mut self, own: u64) {
        self.children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
        self.size = own + self.children.iter().map(|c| c.size).sum::<u64>();
        self.files = self.children.iter().map(|c| c.files).sum();
        self.newest = self.children.iter().map(|c| c.newest).max().unwrap_or(self.newest).max(self.newest);
    }

    /// The node at `path`, a list of child indices from here.
    pub fn at(&self, path: &[usize]) -> Option<&Node> {
        path.iter().try_fold(self, |n, &i| n.children.get(i))
    }

    /// The filesystem path of the node at `path`, if it is on disk.
    pub fn fs_path(&self, path: &[usize]) -> Option<PathBuf> {
        let mut out: Option<PathBuf> = None;
        let mut n = self;
        for &i in path {
            n = n.children.get(i)?;
            match n.kind {
                Kind::Mount => out = Some(PathBuf::from(&*n.name)),
                Kind::Dir | Kind::File => out = Some(out?.join(&*n.name)),
                _ => return None,
            }
        }
        out
    }

    /// The index path of `target`, an absolute filesystem path, if the scan
    /// kept it as its own node. The longest mount point that contains it wins.
    pub fn find(&self, target: &std::path::Path) -> Option<Vec<usize>> {
        let (mi, rest) = self
            .children
            .iter()
            .enumerate()
            .filter(|(_, m)| m.kind == Kind::Mount)
            .filter_map(|(i, m)| target.strip_prefix(&*m.name).ok().map(|r| (i, r, m.name.len())))
            .max_by_key(|(_, _, len)| *len)
            .map(|(i, r, _)| (i, r))?;
        let mut path = vec![mi];
        let mut n = &self.children[mi];
        for part in rest.components() {
            let name = part.as_os_str().to_str()?;
            let i = n.children.iter().position(|c| matches!(c.kind, Kind::Dir | Kind::File) && &*c.name == name)?;
            path.push(i);
            n = &n.children[i];
        }
        Some(path)
    }

    /// Removes the node at `path` after it was deleted from disk, and
    /// subtracts its size from every ancestor. Returns the parent's path,
    /// which can change because siblings are re-sorted by their new sizes.
    pub fn remove(&mut self, path: &[usize]) -> Option<Vec<usize>> {
        let (&last, parents) = path.split_last()?;
        let removed = {
            let parent = self.at_mut(parents)?;
            if last >= parent.children.len() {
                return None;
            }
            parent.children.remove(last)
        };
        // Walk down again, fixing sizes and re-sorting, tracking new indices.
        let mut new_path = Vec::with_capacity(parents.len());
        let mut n = self;
        n.size = n.size.saturating_sub(removed.size);
        n.files = n.files.saturating_sub(removed.files);
        for &i in parents {
            let name = n.children[i].name.clone();
            let kind = n.children[i].kind;
            {
                let c = &mut n.children[i];
                c.size = c.size.saturating_sub(removed.size);
                c.files = c.files.saturating_sub(removed.files);
            }
            n.children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
            let j = n.children.iter().position(|c| c.name == name && c.kind == kind)?;
            new_path.push(j);
            n = &mut n.children[j];
        }
        Some(new_path)
    }

    fn at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        path.iter().try_fold(self, |n, &i| n.children.get_mut(i))
    }

    /// Folds children beyond the first `keep`, and any smaller than `min`,
    /// into one [`Kind::Other`] node. Expects children sorted largest first.
    pub fn fold(&mut self, keep: usize, min: u64, mut other: Node) {
        let cut = self.children.iter().take(keep).take_while(|c| c.size >= min).count();
        for c in self.children.drain(cut..) {
            other.size += c.size;
            other.files += c.files;
            other.newest = other.newest.max(c.newest);
        }
        if other.size > 0 || other.files > 0 {
            other.name = match other.files {
                1 => "1 smaller item".into(),
                n => format!("{n} smaller items").into(),
            };
            let at = self.children.partition_point(|c| c.size >= other.size);
            self.children.insert(at, other);
        }
    }

    /// Number of nodes in this tree, for memory accounting in tests.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(Node::count).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, children: Vec<Node>) -> Node {
        let mut n = Node::new(name, Kind::Dir);
        n.children = children;
        n.sum_children(0);
        n
    }

    fn sample() -> Node {
        let mut m = Node::new("/", Kind::Mount);
        m.children = vec![
            dir("home", vec![Node::file("a.iso", 500, 10), dir("src", vec![Node::file("x", 300, 20), Node::file("y", 100, 5)])]),
            Node::file("swap", 200, 1),
        ];
        m.sum_children(0);
        let mut root = Node::new("This computer", Kind::Computer);
        root.children = vec![m];
        root.sum_children(0);
        root
    }

    #[test]
    fn sizes_paths_and_lookup() {
        let t = sample();
        assert_eq!(t.size, 1100);
        assert_eq!(t.files, 4);
        assert_eq!(t.newest, 20);
        let home = t.find(std::path::Path::new("/home/src/x")).unwrap();
        assert_eq!(home, vec![0, 0, 1, 0]);
        assert_eq!(t.fs_path(&home).unwrap(), PathBuf::from("/home/src/x"));
        assert_eq!(t.at(&[0, 0, 1]).unwrap().name.as_ref(), "src");
        assert_eq!(t.find(std::path::Path::new("/nope")), None);
    }

    #[test]
    fn remove_resorts_and_returns_the_new_parent_path() {
        let mut t = sample();
        // Remove a.iso (500): home drops to 400, below... still above swap (200).
        let parent = t.remove(&[0, 0, 0]).unwrap();
        assert_eq!(parent, vec![0, 0]);
        assert_eq!(t.size, 600);
        assert_eq!(t.at(&[0, 0]).unwrap().size, 400);
        // Remove src (400): home drops to 0 and sorts after swap.
        let parent = t.remove(&[0, 0, 0]).unwrap();
        assert_eq!(parent, vec![0, 1]);
        assert_eq!(t.at(&[0, 1]).unwrap().name.as_ref(), "home");
        assert_eq!(t.size, 200);
    }

    #[test]
    fn fold_keeps_the_largest() {
        let mut d = dir("d", (1..=6).map(|i| Node::file(format!("f{i}"), i * 10, i64::from(i as u8))).collect());
        d.fold(3, 35, Node::new("", Kind::Other));
        let names: Vec<_> = d.children.iter().map(|c| c.name.to_string()).collect();
        // The folded node sorts by its size like any other.
        assert_eq!(names, ["f6", "3 smaller items", "f5", "f4"]);
        assert_eq!(d.children[1].size, 60);
        assert_eq!(d.children[1].newest, 3);
    }
}
