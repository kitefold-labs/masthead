// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Space that can be reclaimed, grouped for the Cleanup tab.
//!
//! Every item names the action that reclaims it, and that action is always
//! the owning tool's or a check-then-delete of regenerable data: nothing here
//! proposes removing a user's own files except old downloads, which go to the
//! Trash.

use crate::docker::Df;
use crate::scan::{CacheKind, Scan};
use crate::time::{self, DAY};
use crate::tree::{Kind, Node};
use std::fmt::Write as _;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Items untouched for this long are marked stale.
pub const STALE: i64 = 30 * DAY;
/// Smaller items are not worth a row.
const MIN_SIZE: u64 = 32 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Action {
    /// Delete a directory of regenerable data: a build output, `node_modules`
    /// or a cache. [`crate::clean`] re-checks that it still is one.
    DeleteCache(PathBuf),
    GoCleanModcache,
    RustupUninstall(String),
    EmptyTrash,
    MoveToTrash(PathBuf),
    DockerImage {
        id: String,
        tags: Vec<String>,
    },
    DockerContainer(String),
    DockerVolume(String),
    /// Prune build cache unused for this many hours, or all unused cache.
    DockerBuildCache(Option<u32>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    pub detail: String,
    pub size: u64,
    pub stale: bool,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupId {
    DockerImages,
    DockerBuildCache,
    DockerContainers,
    DockerVolumes,
    BuildOutputs,
    AppCaches,
    Toolchains,
    Trash,
    Downloads,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub title: &'static str,
    /// What the items are and what removing them does.
    pub about: String,
    pub items: Vec<Item>,
}

impl Group {
    pub fn size(&self) -> u64 {
        self.items.iter().map(|i| i.size).sum()
    }
}

/// The user's directories the finders look in.
#[derive(Debug, Clone)]
pub struct Home {
    pub home: PathBuf,
    pub cache: PathBuf,
    pub downloads: PathBuf,
    pub trash: PathBuf,
    pub cargo: PathBuf,
    pub rustup: PathBuf,
}

impl Home {
    pub fn from_env() -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let data = std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from);
        Some(Self {
            cache: std::env::var_os("XDG_CACHE_HOME").map_or_else(|| home.join(".cache"), PathBuf::from),
            downloads: user_dir(&home, "XDG_DOWNLOAD_DIR").unwrap_or_else(|| home.join("Downloads")),
            trash: data.join("Trash"),
            cargo: std::env::var_os("CARGO_HOME").map_or_else(|| home.join(".cargo"), PathBuf::from),
            rustup: std::env::var_os("RUSTUP_HOME").map_or_else(|| home.join(".rustup"), PathBuf::from),
            home,
        })
    }

    /// `~/x` for paths under home.
    pub fn short(&self, p: &Path) -> String {
        match p.strip_prefix(&self.home) {
            Ok(r) => format!("~/{}", r.display()),
            Err(_) => p.display().to_string(),
        }
    }

    /// Caches outside `~/.cache` that tools keep in fixed places, with a
    /// description. All are re-downloaded when needed.
    pub fn known_caches(&self) -> Vec<(PathBuf, &'static str)> {
        vec![
            (self.cargo.join("registry/cache"), "Downloaded crate archives (Cargo)"),
            (self.cargo.join("registry/src"), "Unpacked crate sources (Cargo)"),
            (self.cargo.join("git/checkouts"), "Git dependency checkouts (Cargo)"),
            (self.home.join(".npm/_cacache"), "Package cache (npm)"),
            (self.home.join(".gradle/caches"), "Build cache (Gradle)"),
            (self.home.join(".local/share/pnpm/store"), "Package store (pnpm)"),
        ]
    }
}

/// A folder from `~/.config/user-dirs.dirs`.
fn user_dir(home: &Path, key: &str) -> Option<PathBuf> {
    let s = std::fs::read_to_string(home.join(".config/user-dirs.dirs")).ok()?;
    let line = s.lines().find(|l| l.starts_with(key))?;
    let v = line.split_once('=')?.1.trim().trim_matches('"');
    Some(PathBuf::from(v.replace("$HOME", &home.to_string_lossy())))
}

/// Groups from a scan, in display order. Empty groups are left out.
#[allow(clippy::too_many_lines)] // one block per group
pub fn from_scan(scan: &Scan, home: &Home, now: i64) -> Vec<Group> {
    let me = rustix::process::getuid().as_raw();
    let owned = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.uid() == me && m.is_dir());
    let size_of = |p: &Path| scan.tree.find(p).and_then(|i| scan.tree.at(&i)).filter(|n| n.kind == Kind::Dir);
    let mut groups = Vec::new();

    // Build outputs and node_modules found during the walk, outside the
    // tool caches listed below.
    let skip = [&home.cache, &home.cargo, &home.rustup, &home.trash];
    let mut outputs: Vec<Item> = scan
        .caches
        .iter()
        .filter(|c| !skip.iter().any(|s| c.path.starts_with(s)) && owned(&c.path))
        .map(|c| {
            let what = match c.kind {
                CacheKind::NodeModules => "npm packages",
                CacheKind::Tagged if c.path.parent().is_some_and(|p| p.join("Cargo.toml").is_file()) => "Cargo build output",
                CacheKind::Tagged => "Build cache",
            };
            Item {
                title: home.short(&c.path),
                detail: format!("{what} · last changed {} ago", time::age(c.newest, now)),
                size: c.size,
                stale: now - c.newest >= STALE,
                action: Action::DeleteCache(c.path.clone()),
            }
        })
        .collect();
    outputs.sort_by_key(|x| std::cmp::Reverse(x.size));
    groups.push(Group {
        id: GroupId::BuildOutputs,
        title: "Build outputs and dependencies",
        about: "Cargo target/ folders, node_modules and other folders marked as caches. Your next build or install recreates them.".into(),
        items: outputs,
    });

    // ~/.cache, one row per application, plus fixed tool caches.
    let mut caches: Vec<Item> = Vec::new();
    if let Some(dir) = size_of(&home.cache) {
        for c in dir.children.iter().filter(|c| c.kind == Kind::Dir && c.size >= MIN_SIZE) {
            let path = home.cache.join(&*c.name);
            caches.push(Item {
                title: home.short(&path),
                detail: format!("Application cache · last changed {} ago", time::age(c.newest, now)),
                size: c.size,
                stale: now - c.newest >= STALE,
                action: Action::DeleteCache(path),
            });
        }
    }
    for (path, what) in home.known_caches() {
        if let Some(n) = size_of(&path).filter(|n| n.size >= MIN_SIZE) {
            caches.push(Item {
                title: home.short(&path),
                detail: format!("{what} · last changed {} ago", time::age(n.newest, now)),
                size: n.size,
                stale: now - n.newest >= STALE,
                action: Action::DeleteCache(path),
            });
        }
    }
    let gomod = home.home.join("go/pkg/mod");
    if let Some(n) = size_of(&gomod).filter(|n| n.size >= MIN_SIZE) {
        caches.push(Item {
            title: home.short(&gomod),
            detail: format!("Go module cache · last changed {} ago", time::age(n.newest, now)),
            size: n.size,
            stale: now - n.newest >= STALE,
            action: Action::GoCleanModcache,
        });
    }
    caches.sort_by_key(|x| std::cmp::Reverse(x.size));
    groups.push(Group {
        id: GroupId::AppCaches,
        title: "Caches",
        about: "Data applications and package managers keep to avoid downloading or computing it again. Clearing one may slow that application down once.".into(),
        items: caches,
    });

    // Rust toolchains other than the default.
    let default = std::fs::read_to_string(home.rustup.join("settings.toml"))
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("default_toolchain = ").map(|v| v.trim_matches('"').to_owned())));
    if let Some(dir) = size_of(&home.rustup.join("toolchains")) {
        let items = dir
            .children
            .iter()
            .filter(|c| c.kind == Kind::Dir && c.size >= MIN_SIZE && Some(&*c.name) != default.as_deref())
            .map(|c| Item {
                title: c.name.to_string(),
                detail: format!("Rust toolchain · installed or updated {} ago", time::age(c.newest, now)),
                size: c.size,
                stale: false,
                action: Action::RustupUninstall(c.name.to_string()),
            })
            .collect();
        groups.push(Group {
            id: GroupId::Toolchains,
            title: "Rust toolchains",
            about: format!(
                "Toolchains other than your default{}. A project that pins one installs it again on its next build.",
                default.map(|d| format!(" ({d})")).unwrap_or_default()
            ),
            items,
        });
    }

    if let Some(t) = size_of(&home.trash).filter(|n| n.size > 0) {
        groups.push(trash_group(t.size, t.files));
    }

    groups.push(Group {
        id: GroupId::Downloads,
        title: "Old downloads",
        about: "Files over 100 MB in Downloads that have not changed in a month. They go to the Trash, so empty it afterwards to free the space.".into(),
        items: scan
            .downloads
            .iter()
            .map(|d| Item {
                title: d.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                detail: format!("{} · unchanged for {}", home.short(d.path.parent().unwrap_or(&d.path)), time::age(d.mtime, now)),
                size: d.size,
                stale: true,
                action: Action::MoveToTrash(d.path.clone()),
            })
            .collect(),
    });
    groups.retain(|g| !g.items.is_empty());
    groups
}

/// The Trash, holding `files` files in `size` bytes.
pub fn trash_group(size: u64, files: u64) -> Group {
    Group {
        id: GroupId::Trash,
        title: "Trash",
        about: "Files you moved to the Trash still use space until it is emptied.".into(),
        items: vec![Item {
            title: "Empty the Trash".into(),
            detail: if files == 1 { "1 file".into() } else { format!("{files} files") },
            size,
            stale: false,
            action: Action::EmptyTrash,
        }],
    }
}

/// Counts `size` bytes in `files` files that were just moved to the Trash.
pub fn add_to_trash(groups: &mut Vec<Group>, size: u64, files: u64) {
    match groups.iter_mut().find(|g| g.id == GroupId::Trash).and_then(|g| g.items.first_mut()) {
        Some(item) => {
            item.size += size;
            let before: u64 = item.detail.split(' ').next().and_then(|n| n.parse().ok()).unwrap_or(0);
            item.detail = if before + files == 1 { "1 file".into() } else { format!("{} files", before + files) };
        }
        None => groups.push(trash_group(size, files)),
    }
}

/// Groups from Docker's `/system/df`, in display order.
#[allow(clippy::too_many_lines)] // one block per group
pub fn from_docker(df: &Df, engine: &str, now: i64) -> Vec<Group> {
    let mut images: Vec<Item> = df
        .images
        .iter()
        .filter(|i| i.containers == 0)
        .map(|i| {
            let tags = i.tags();
            let title = tags.first().cloned().unwrap_or_else(|| format!("Untagged image {}", i.short_id()));
            let mut detail = format!("created {} ago", time::age(i.created, now));
            if tags.len() > 1 {
                let _ = write!(detail, " · {} more tag{}", tags.len() - 1, if tags.len() == 2 { "" } else { "s" });
            }
            if i.labels.as_ref().is_some_and(|l| l.contains_key("dev.tilt.built")) {
                detail.push_str(" · built by Tilt");
            }
            Item {
                title,
                detail,
                size: i.unique(),
                stale: tags.is_empty() || now - i.created >= STALE,
                action: Action::DockerImage { id: i.id.clone(), tags },
            }
        })
        .collect();
    images.sort_by_key(|x| std::cmp::Reverse(x.size));

    let records = df.build_cache.as_deref().unwrap_or_default();
    let free = records.iter().filter(|r| !r.in_use && !r.shared);
    let last_used = |r: &&crate::docker::BuildRecord| {
        r.last_used_at.as_deref().and_then(time::parse_rfc3339).or_else(|| time::parse_rfc3339(&r.created_at))
    };
    let (old, recent): (Vec<_>, Vec<_>) = free.partition(|r| last_used(r).is_some_and(|t| now - t >= STALE));
    let sum = |v: &[&crate::docker::BuildRecord]| v.iter().map(|r| r.size.max(0) as u64).sum::<u64>();
    let mut cache = Vec::new();
    if sum(&old) > 0 {
        cache.push(Item {
            title: "Not used in 30 days".into(),
            detail: format!("{} cache records", old.len()),
            size: sum(&old),
            stale: true,
            action: Action::DockerBuildCache(Some(30 * 24)),
        });
    }
    if sum(&recent) > 0 {
        cache.push(Item {
            title: "Used in the last 30 days".into(),
            detail: format!("{} cache records · the next build may be slower", recent.len()),
            size: sum(&recent),
            stale: false,
            action: Action::DockerBuildCache(None),
        });
    }
    let total: u64 = records.iter().map(|r| r.size.max(0) as u64).sum();
    let shared = total.saturating_sub(records.iter().filter(|r| !r.in_use && !r.shared).map(|r| r.size.max(0) as u64).sum());

    let mut containers: Vec<Item> = df
        .containers
        .iter()
        .filter(|c| matches!(c.state.as_str(), "exited" | "created" | "dead"))
        .map(|c| Item {
            title: c.name().to_owned(),
            detail: format!("{} · {}", short_image(&c.image), if c.status.is_empty() { &c.state } else { &c.status }),
            size: c.size_rw.unwrap_or(0).max(0) as u64,
            stale: now - c.created >= STALE,
            action: Action::DockerContainer(c.id.clone()),
        })
        .collect();
    containers.sort_by_key(|x| std::cmp::Reverse(x.size));

    let mut volumes: Vec<Item> = df
        .volumes
        .iter()
        .filter(|v| v.usage_data.as_ref().is_some_and(|u| u.ref_count == 0))
        .map(|v| {
            let created = time::parse_rfc3339(&v.created_at);
            let anon = v.anonymous();
            Item {
                title: if anon { format!("{} (anonymous)", &v.name[..v.name.len().min(12)]) } else { v.name.clone() },
                detail: created.map_or_else(|| "volume".into(), |t| format!("created {} ago", time::age(t, now))),
                size: v.usage_data.as_ref().map_or(0, |u| u.size.max(0) as u64),
                stale: anon && created.is_some_and(|t| now - t >= STALE),
                action: Action::DockerVolume(v.name.clone()),
            }
        })
        .collect();
    volumes.sort_by_key(|x| std::cmp::Reverse(x.size));

    let mut groups = vec![
        Group {
            id: GroupId::DockerImages,
            title: "Container images",
            about: format!(
                "{engine} images no container uses. Sizes count only layers no other image shares, so removing several frees more than their sum."
            ),
            items: images,
        },
        Group {
            id: GroupId::DockerBuildCache,
            title: "Container build cache",
            about: format!(
                "Layers {engine} keeps to speed up builds. Another {} of cache is shared with images and is freed once those images are removed.",
                crate_bytes(shared)
            ),
            items: cache,
        },
        Group {
            id: GroupId::DockerContainers,
            title: "Stopped containers",
            about: "Containers that are not running. Removing one deletes its changes to the image's files.".into(),
            items: containers,
        },
        Group {
            id: GroupId::DockerVolumes,
            title: "Unused volumes",
            about: "Volumes no container uses. They can hold databases and other data you meant to keep, so check each before removing it."
                .into(),
            items: volumes,
        },
    ];
    groups.retain(|g| !g.items.is_empty());
    groups
}

/// An image reference without its digest, or a short ID for a bare digest.
fn short_image(image: &str) -> &str {
    match image.strip_prefix("sha256:") {
        Some(id) => &id[..id.len().min(12)],
        None => image.split_once('@').map_or(image, |(name, _)| name),
    }
}

/// Decimal bytes, like the rest of Masthead.
fn crate_bytes(b: u64) -> String {
    let units = ["B", "kB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < units.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", units[i]) }
}

/// Removes items whose action was carried out, and returns the groups that
/// are left.
pub fn without(groups: &mut Vec<Group>, done: &[Action]) {
    for g in groups.iter_mut() {
        g.items.retain(|i| !done.contains(&i.action));
    }
    groups.retain(|g| !g.items.is_empty());
}

/// For tests and the example: a tree node by path.
pub fn node_at<'a>(tree: &'a Node, p: &Path) -> Option<&'a Node> {
    tree.find(p).and_then(|i| tree.at(&i))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::{BuildRecord, Container, Image, Volume, VolumeUsage};

    #[test]
    fn docker_groups() {
        let now = 100 * DAY;
        let df = Df {
            images: vec![
                Image {
                    id: "sha256:aaaa".into(),
                    repo_tags: Some(vec!["app:1".into(), "app:latest".into()]),
                    created: now - 40 * DAY,
                    size: 500,
                    shared_size: 100,
                    containers: 0,
                    labels: None,
                },
                Image {
                    id: "sha256:bbbb".into(),
                    repo_tags: Some(vec!["used:1".into()]),
                    created: 0,
                    size: 900,
                    shared_size: 0,
                    containers: 1,
                    labels: None,
                },
                Image {
                    id: "sha256:cccc".into(),
                    repo_tags: None,
                    created: now - DAY,
                    size: 50,
                    shared_size: 0,
                    containers: 0,
                    labels: None,
                },
            ],
            containers: vec![
                Container {
                    id: "c1".into(),
                    names: vec!["/old".into()],
                    state: "exited".into(),
                    created: 0,
                    size_rw: Some(7),
                    ..Container::default()
                },
                Container { id: "c2".into(), names: vec!["/up".into()], state: "running".into(), ..Container::default() },
            ],
            volumes: vec![
                Volume { name: "db".into(), usage_data: Some(VolumeUsage { size: 70, ref_count: 0 }), ..Volume::default() },
                Volume { name: "live".into(), usage_data: Some(VolumeUsage { size: 70, ref_count: 1 }), ..Volume::default() },
            ],
            build_cache: Some(vec![
                BuildRecord { id: "1".into(), size: 10, last_used_at: Some("1970-01-02T00:00:00Z".into()), ..BuildRecord::default() },
                BuildRecord { id: "2".into(), size: 20, last_used_at: Some("1970-04-10T00:00:00Z".into()), ..BuildRecord::default() },
                BuildRecord { id: "3".into(), size: 999, shared: true, ..BuildRecord::default() },
            ]),
        };
        let g = from_docker(&df, "Docker", now);
        let ids: Vec<_> = g.iter().map(|g| g.id).collect();
        assert_eq!(ids, [GroupId::DockerImages, GroupId::DockerBuildCache, GroupId::DockerContainers, GroupId::DockerVolumes]);
        let imgs = &g[0].items;
        assert_eq!(imgs.len(), 2);
        assert_eq!((imgs[0].title.as_str(), imgs[0].size, imgs[0].stale), ("app:1", 400, true));
        assert!(imgs[0].detail.contains("· 1 more tag"), "{}", imgs[0].detail);
        assert_eq!((imgs[1].title.as_str(), imgs[1].stale), ("Untagged image cccc", true));
        let cache: Vec<_> = g[1].items.iter().map(|i| (i.size, i.action.clone())).collect();
        assert_eq!(cache, [(10, Action::DockerBuildCache(Some(720))), (20, Action::DockerBuildCache(None))]);
        assert_eq!(g[2].items.len(), 1);
        assert_eq!(g[3].items[0].title, "db");
        assert!(!g[3].items[0].stale);

        assert_eq!(short_image("sha256:0123456789abcdef0123"), "0123456789ab");
        assert_eq!(short_image("kindest/node:v1@sha256:45"), "kindest/node:v1");
        let mut g = g;
        without(&mut g, &[Action::DockerContainer("c1".into())]);
        assert!(!g.iter().any(|g| g.id == GroupId::DockerContainers));
    }

    #[test]
    fn scan_groups() {
        let tmp = tempfile::tempdir().unwrap();
        let h = tmp.path();
        let w = |p: &str, n: usize| {
            let p = h.join(p);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![0u8; n]).unwrap();
        };
        w("code/app/Cargo.toml", 10);
        std::fs::create_dir_all(h.join("code/app/target")).unwrap();
        std::fs::write(h.join("code/app/target/CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55").unwrap();
        w("code/app/target/debug/big", 40 << 20);
        w(".cache/thumbnails/x", 40 << 20);
        w(".cache/tiny/x", 10);
        w(".cargo/registry/cache/c.crate", 40 << 20);
        w(".local/share/Trash/files/old", 5 << 20);
        let home = Home {
            home: h.into(),
            cache: h.join(".cache"),
            downloads: h.join("Downloads"),
            trash: h.join(".local/share/Trash"),
            cargo: h.join(".cargo"),
            rustup: h.join(".rustup"),
        };
        let m = crate::mounts::Mount::dir(h.into());
        let scan = crate::scan(&[m], &crate::scan::Options::default(), &crate::Progress::default());
        let g = from_scan(&scan, &home, time::now());
        let summary: Vec<_> = g.iter().map(|g| (g.id, g.items.iter().map(|i| i.title.clone()).collect::<Vec<_>>())).collect();
        assert_eq!(
            summary,
            [
                (GroupId::BuildOutputs, vec!["~/code/app/target".to_owned()]),
                (GroupId::AppCaches, vec!["~/.cache/thumbnails".to_owned(), "~/.cargo/registry/cache".to_owned()]),
                (GroupId::Trash, vec!["Empty the Trash".to_owned()]),
            ]
        );
        assert!(g[0].items[0].detail.starts_with("Cargo build output"));

        let mut g = g;
        add_to_trash(&mut g, 10, 2);
        let t = g.iter().find(|g| g.id == GroupId::Trash).unwrap();
        assert_eq!(t.items[0].detail, "3 files");
        g.retain(|g| g.id != GroupId::Trash);
        add_to_trash(&mut g, 10, 1);
        assert_eq!(g.last().unwrap().items[0].detail, "1 file");
    }
}
