// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! State of the Disk usage page: the scan, where the map is open, the
//! cleanup findings and what the user ticked.
//!
//! Scans, Docker queries and cleanups run on tokio's blocking pool and come
//! back as messages, so the window keeps drawing. The scan's tree stays in
//! memory until the window closes.

use crate::app::Message;
use crate::treemap;
use cosmic::app::Task;
use masthead_disk::docker::{Df, Docker};
use masthead_disk::findings::{self, Action, Group, GroupId, Home};
use masthead_disk::mounts::{self, Space};
use masthead_disk::{Kind, Progress, Scan, scan, time};
use masthead_disk::{child, clean};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Map,
    Cleanup,
}

/// Carries a finished scan through a message. Messages must be `Clone`; the
/// tree is taken out once.
#[derive(Debug, Clone)]
pub struct Handoff(Arc<Mutex<Option<Result<Scan, String>>>>);

type Outcome = Result<Option<u64>, String>;

#[derive(Debug, Clone)]
pub enum DiskMsg {
    Scan,
    Stop,
    /// Redraws the scan progress.
    Tick,
    Scanned(Handoff),
    /// The engine's usage and where it keeps its data. `resort` puts the
    /// groups back in size order; without it they keep their places.
    Docker {
        df: Result<Df, String>,
        root: Option<PathBuf>,
        resort: bool,
    },
    Tab(Tab),
    /// Opens the node at this index path.
    Go(Vec<usize>),
    ShowInFiles(PathBuf),
    AskTrash(Vec<usize>),
    Toggle(Action, bool),
    SelectStale(GroupId),
    ClearGroup(GroupId),
    Expand(GroupId),
    AskClean(GroupId),
    Confirm,
    Dismiss,
    Trashed(PathBuf, Result<(), String>),
    /// One cleanup item finished: its action, listed size and outcome.
    Cleaned(Action, u64, Outcome),
}

/// What the confirmation dialog asks about.
#[derive(Debug, Clone)]
pub enum Confirm {
    Trash { path: PathBuf, name: String, size: u64, dir: bool },
    Clean { group: GroupId, items: Vec<(Action, String, u64)> },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DockerState {
    #[default]
    NotAsked,
    Loading,
    Ready,
    /// No Docker or Podman socket.
    Missing,
    Failed(String),
}

/// A confirmed cleanup. Items run one at a time, so each leaves the list as
/// soon as it is done.
#[derive(Debug, Default)]
pub struct Cleaning {
    queue: VecDeque<(Action, u64)>,
    pub total: usize,
    freed: u64,
    trashed: u64,
    trashed_count: usize,
    failed: Vec<String>,
    docker_changed: bool,
}

impl Cleaning {
    /// The 1-based number of the item running now.
    pub fn current(&self) -> usize {
        self.total - self.queue.len()
    }
}

#[derive(Debug, Default)]
pub struct Disk {
    pub tab: Tab,
    /// Set while a scan runs.
    pub progress: Option<Arc<Progress>>,
    pub scan: Option<Scan>,
    pub scanned_at: i64,
    /// The folder the map shows, as child indices from the root.
    pub path: Vec<usize>,
    /// Folders the Cleanup tab lists, tinted on the map.
    pub cache_paths: HashSet<PathBuf>,
    pub groups: Vec<Group>,
    pub docker_groups: Vec<Group>,
    pub docker_state: DockerState,
    pub docker: Option<Docker>,
    docker_root: Option<PathBuf>,
    pub selected: HashSet<Action>,
    pub expanded: HashSet<GroupId>,
    /// Group order on the Cleanup tab, largest first when the findings
    /// arrived. Groups keep their place while their items are cleaned up.
    order: Vec<GroupId>,
    pub confirm: Option<Confirm>,
    /// Set while a cleanup or a move to the Trash runs.
    pub busy: bool,
    pub cleaning: Option<Cleaning>,
    /// Filesystems that hold a cleanup item, picked when the findings
    /// arrive so a row stays while its items are cleaned up.
    space_mounts: Vec<PathBuf>,
    /// Size and use of each of [`Self::space_mounts`], refreshed whenever
    /// something is deleted.
    pub space: Vec<(PathBuf, Space)>,
    pub message: Option<Result<String, String>>,
    pub map_cache: treemap::Cache,
    pub home: Option<Home>,
}

impl Disk {
    pub fn scanning(&self) -> bool {
        self.progress.is_some()
    }

    /// Every group, in [`Self::order`]. Groups that appeared since, such as
    /// the Trash after moving files to it, come last.
    pub fn all_groups(&self) -> impl Iterator<Item = &Group> {
        let mut all: Vec<&Group> = self.docker_groups.iter().chain(&self.groups).collect();
        all.sort_by_key(|g| (self.order.iter().position(|o| *o == g.id).unwrap_or(usize::MAX), std::cmp::Reverse(g.size())));
        all.into_iter()
    }

    /// Fixes the group order, largest first, and the filesystems the space
    /// rows show, from the findings as they are now.
    fn arrange(&mut self) {
        let local = mounts::local();
        let mut all: Vec<&Group> = self.docker_groups.iter().chain(&self.groups).collect();
        all.sort_by_key(|g| std::cmp::Reverse(g.size()));
        let order = all.iter().map(|g| g.id).collect();
        let held: HashSet<&Path> = all
            .iter()
            .flat_map(|g| &g.items)
            .filter_map(|i| self.path_of(&i.action))
            .filter_map(|p| mounts::holding(&local, &p))
            .map(|m| m.path.as_path())
            .collect();
        self.space_mounts = local.iter().filter(|m| held.contains(m.path.as_path())).map(|m| m.path.clone()).collect();
        self.order = order;
        self.refresh_space();
    }

    /// Where the space `a` reclaims is.
    fn path_of(&self, a: &Action) -> Option<PathBuf> {
        let home = self.home.as_ref();
        match a {
            Action::DeleteCache(p) | Action::MoveToTrash(p) => Some(p.clone()),
            Action::EmptyTrash => home.map(|h| h.trash.clone()),
            Action::RustupUninstall(name) => home.map(|h| h.rustup.join("toolchains").join(name)),
            Action::GoCleanModcache => home.map(|h| h.home.join("go/pkg/mod")),
            Action::DockerImage { .. } | Action::DockerContainer(_) | Action::DockerVolume(_) | Action::DockerBuildCache(_) => {
                self.docker_root.clone()
            }
        }
    }

    /// Reads the size and use of the filesystems that hold cleanup items.
    pub fn refresh_space(&mut self) {
        self.space = self.space_mounts.iter().filter_map(|m| mounts::space(m).filter(|s| s.total > 0).map(|s| (m.clone(), s))).collect();
    }

    /// True if the map may offer to move `p` to the Trash: inside the home
    /// folder, and not the home folder itself.
    pub fn trashable(&self, p: &Path) -> bool {
        self.home.as_ref().is_some_and(|h| p.starts_with(&h.home) && p != h.home)
    }

    pub fn reclaimable(&self) -> u64 {
        self.all_groups().map(Group::size).sum()
    }

    /// Starts a scan unless one is running, and asks the container engine
    /// for its usage alongside.
    pub fn start(&mut self) -> Task<Message> {
        if self.scanning() {
            return Task::none();
        }
        if self.home.is_none() {
            self.home = Home::from_env();
        }
        let progress = Arc::new(Progress::default());
        self.progress = Some(progress.clone());
        self.message = None;
        self.refresh_space();
        let opts = scan::Options { downloads: self.home.as_ref().map(|h| h.downloads.clone()) };
        let scan_task = cosmic::task::future(async move {
            let s = tokio::task::spawn_blocking(move || match std::env::current_exe() {
                // In a child process, so the walk's memory leaves with it.
                Ok(exe) => child::run(&exe, &opts, &progress),
                Err(_) => Ok(masthead_disk::scan(&[], &opts, &progress)),
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
            Message::Disk(DiskMsg::Scanned(Handoff(Arc::new(Mutex::new(Some(s))))))
        });
        Task::batch([scan_task, self.load_docker(true)])
    }

    fn load_docker(&mut self, resort: bool) -> Task<Message> {
        self.docker = Docker::find();
        let Some(d) = self.docker.clone() else {
            self.docker_state = DockerState::Missing;
            self.docker_groups.clear();
            return Task::none();
        };
        self.docker_state = DockerState::Loading;
        cosmic::task::future(async move {
            let (df, root) =
                tokio::task::spawn_blocking(move || (d.df(), d.root_dir().ok())).await.unwrap_or_else(|e| (Err(e.to_string()), None));
            Message::Disk(DiskMsg::Docker { df, root, resort })
        })
    }

    #[allow(clippy::too_many_lines)] // one arm per message
    pub fn update(&mut self, msg: DiskMsg) -> Task<Message> {
        match msg {
            DiskMsg::Scan => return self.start(),
            DiskMsg::Stop => {
                if let Some(p) = &self.progress {
                    p.cancel.store(true, Relaxed);
                }
            }
            DiskMsg::Tick => {}
            DiskMsg::Scanned(h) => {
                self.progress = None;
                self.refresh_space();
                let s = h.0.lock().ok().and_then(|mut s| s.take());
                match s {
                    Some(Ok(s)) if !s.cancelled => self.set_scan(s),
                    Some(Err(e)) => self.message = Some(Err(e)),
                    _ => self.message = Some(Err("Scan stopped.".into())),
                }
            }
            DiskMsg::Docker { df, root, resort } => {
                self.docker_root = root;
                let engine = self.docker.as_ref().map_or("Docker", |d| d.engine);
                match df {
                    Ok(df) => {
                        self.docker_groups = findings::from_docker(&df, engine, time::now());
                        self.docker_state = DockerState::Ready;
                    }
                    Err(e) => {
                        self.docker_groups.clear();
                        self.docker_state = DockerState::Failed(e);
                    }
                }
                if resort {
                    self.arrange();
                }
                self.prune_selection();
            }
            DiskMsg::Tab(t) => self.tab = t,
            DiskMsg::Go(p) => {
                if self.scan.as_ref().and_then(|s| s.tree.at(&p)).is_some_and(masthead_disk::Node::is_dir) {
                    self.path = p;
                    self.map_cache.clear();
                }
            }
            DiskMsg::ShowInFiles(p) => {
                if let Err(e) = clean::show_in_files(&p) {
                    self.message = Some(Err(e));
                }
            }
            DiskMsg::AskTrash(p) => {
                let Some(s) = &self.scan else { return Task::none() };
                let (Some(n), Some(fs)) = (s.tree.at(&p), s.tree.fs_path(&p)) else { return Task::none() };
                if !matches!(n.kind, Kind::Dir | Kind::File) {
                    return Task::none();
                }
                if !self.trashable(&fs) {
                    return Task::none();
                }
                self.confirm = Some(Confirm::Trash { path: fs, name: n.name.to_string(), size: n.size, dir: n.is_dir() });
            }
            DiskMsg::Toggle(a, on) => {
                if on {
                    self.selected.insert(a);
                } else {
                    self.selected.remove(&a);
                }
            }
            DiskMsg::SelectStale(g) => {
                let stale: Vec<Action> =
                    self.all_groups().filter(|x| x.id == g).flat_map(|x| &x.items).filter(|i| i.stale).map(|i| i.action.clone()).collect();
                self.selected.extend(stale);
            }
            DiskMsg::ClearGroup(g) => {
                let ids: Vec<Action> = self.all_groups().filter(|x| x.id == g).flat_map(|x| &x.items).map(|i| i.action.clone()).collect();
                for a in ids {
                    self.selected.remove(&a);
                }
            }
            DiskMsg::Expand(g) => {
                if !self.expanded.remove(&g) {
                    self.expanded.insert(g);
                }
            }
            DiskMsg::AskClean(g) => {
                let items: Vec<(Action, String, u64)> = self
                    .all_groups()
                    .filter(|x| x.id == g)
                    .flat_map(|x| &x.items)
                    .filter(|i| self.selected.contains(&i.action))
                    .map(|i| (i.action.clone(), i.title.clone(), i.size))
                    .collect();
                if !items.is_empty() {
                    self.confirm = Some(Confirm::Clean { group: g, items });
                }
            }
            DiskMsg::Dismiss => self.confirm = None,
            DiskMsg::Confirm => return self.run_confirmed(),
            DiskMsg::Trashed(p, r) => {
                self.busy = false;
                self.refresh_space();
                match r {
                    Ok(()) => {
                        let (size, files) = self.forget(&p);
                        findings::add_to_trash(&mut self.groups, size, files);
                        self.message = Some(Ok(format!(
                            "Moved {} ({}) to the Trash. Empty the Trash to free the space.",
                            p.display(),
                            masthead_core::format::bytes(size)
                        )));
                    }
                    Err(e) => self.message = Some(Err(format!("Could not move {} to the Trash: {e}", p.display()))),
                }
            }
            DiskMsg::Cleaned(a, size, r) => return self.on_cleaned(a, size, r),
        }
        Task::none()
    }

    fn set_scan(&mut self, s: Scan) {
        let now = time::now();
        self.groups = self.home.as_ref().map(|h| findings::from_scan(&s, h, now)).unwrap_or_default();
        self.cache_paths = self
            .groups
            .iter()
            .flat_map(|g| &g.items)
            .filter_map(|i| match &i.action {
                Action::DeleteCache(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        // Open the largest filesystem; the root above it is one block.
        self.path = if s.tree.children.is_empty() { Vec::new() } else { vec![0] };
        self.scan = Some(s);
        self.scanned_at = now;
        self.map_cache.clear();
        self.arrange();
        self.prune_selection();
    }

    /// Drops ticks for items no longer listed.
    fn prune_selection(&mut self) {
        let listed: HashSet<&Action> = self.all_groups().flat_map(|g| &g.items).map(|i| &i.action).collect();
        let keep: HashSet<Action> = self.selected.iter().filter(|a| listed.contains(a)).cloned().collect();
        self.selected = keep;
    }

    fn run_confirmed(&mut self) -> Task<Message> {
        let Some(c) = self.confirm.take() else { return Task::none() };
        self.busy = true;
        self.message = None;
        match c {
            Confirm::Trash { path, .. } => cosmic::task::future(async move {
                let p = path.clone();
                let r = tokio::task::spawn_blocking(move || clean::trash(&p)).await.unwrap_or_else(|e| Err(e.to_string()));
                Message::Disk(DiskMsg::Trashed(path, r))
            }),
            Confirm::Clean { items, .. } => {
                if self.home.is_none() {
                    self.busy = false;
                    return Task::none();
                }
                // Pruning all unused build cache covers the 30-day prune.
                let all_cache = items.iter().any(|(a, _, _)| *a == Action::DockerBuildCache(None));
                let queue: VecDeque<_> = items
                    .into_iter()
                    .filter(|(a, _, _)| !(all_cache && matches!(a, Action::DockerBuildCache(Some(_)))))
                    .map(|(a, _, size)| (a, size))
                    .collect();
                self.cleaning = Some(Cleaning { total: queue.len(), queue, ..Cleaning::default() });
                self.clean_next()
            }
        }
    }

    /// Runs the next queued cleanup item, or finishes when none is left.
    fn clean_next(&mut self) -> Task<Message> {
        let next = self.cleaning.as_mut().and_then(|c| c.queue.pop_front());
        let (Some((a, size)), Some(home)) = (next, self.home.clone()) else { return self.finish_cleaning() };
        let docker = self.docker.clone();
        cosmic::task::future(async move {
            let action = a.clone();
            let r = tokio::task::spawn_blocking(move || clean::run(&action, &home, docker.as_ref()))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
            Message::Disk(DiskMsg::Cleaned(a, size, r))
        })
    }

    /// Takes a finished item off the list and starts the next.
    fn on_cleaned(&mut self, a: Action, size: u64, r: Outcome) -> Task<Message> {
        let Some(mut c) = self.cleaning.take() else { return Task::none() };
        match r {
            Ok(reported) => {
                match &a {
                    Action::MoveToTrash(p) => {
                        let (s, f) = self.forget(p);
                        c.trashed += size.max(s);
                        c.trashed_count += 1;
                        findings::add_to_trash(&mut self.groups, size.max(s), f.max(1));
                    }
                    Action::DockerImage { .. } | Action::DockerContainer(_) | Action::DockerVolume(_) | Action::DockerBuildCache(_) => {
                        c.freed += reported.unwrap_or(size);
                        c.docker_changed = true;
                    }
                    Action::DeleteCache(_) | Action::EmptyTrash | Action::RustupUninstall(_) | Action::GoCleanModcache => {
                        c.freed += size;
                        if let Some(p) = self.path_of(&a) {
                            self.forget(&p);
                        }
                    }
                }
                let mut done = vec![a];
                if matches!(done[0], Action::DockerBuildCache(None)) {
                    done.push(Action::DockerBuildCache(Some(30 * 24)));
                }
                for d in &done {
                    self.selected.remove(d);
                }
                findings::without(&mut self.groups, &done);
                findings::without(&mut self.docker_groups, &done);
                self.refresh_space();
            }
            Err(e) => c.failed.push(e),
        }
        self.cleaning = Some(c);
        self.clean_next()
    }

    /// Reports what the cleanup did.
    fn finish_cleaning(&mut self) -> Task<Message> {
        self.busy = false;
        let Some(c) = self.cleaning.take() else { return Task::none() };
        let mut parts = Vec::new();
        if c.freed > 0 || c.trashed_count == 0 {
            parts.push(format!("Freed about {}.", masthead_core::format::bytes(c.freed)));
        }
        if c.trashed_count > 0 {
            parts.push(format!(
                "Moved {} to the Trash ({}). Empty the Trash to free the space.",
                if c.trashed_count == 1 { "1 item".to_owned() } else { format!("{} items", c.trashed_count) },
                masthead_core::format::bytes(c.trashed)
            ));
        }
        match (c.failed.len(), c.failed.first()) {
            (1, Some(first)) => parts.push(format!("One item was not removed: {first}")),
            (n, Some(first)) => parts.push(format!("{n} items were not removed. The first: {first}")),
            _ => {}
        }
        let msg = parts.join(" ");
        self.message = Some(if c.failed.is_empty() { Ok(msg) } else { Err(msg) });
        // Sizes of the rest shift as shared layers become unshared.
        if c.docker_changed { self.load_docker(false) } else { Task::none() }
    }

    /// Removes the node at filesystem path `p` from the tree after it was
    /// deleted, keeping the map on the same folder where it still exists.
    /// Returns the size and file count it had.
    fn forget(&mut self, p: &Path) -> (u64, u64) {
        let Some(s) = &mut self.scan else { return (0, 0) };
        let Some(at) = s.tree.find(p) else { return (0, 0) };
        let had = s.tree.at(&at).map_or((0, 0), |n| (n.size, n.files));
        let current = s.tree.fs_path(&self.path);
        let Some(parent) = s.tree.remove(&at) else { return (0, 0) };
        self.path = match current {
            Some(c) if c.starts_with(p) => parent,
            Some(c) => s.tree.find(&c).unwrap_or(parent),
            None => self.path.clone(),
        };
        self.cache_paths.remove(p);
        self.map_cache.clear();
        had
    }
}
