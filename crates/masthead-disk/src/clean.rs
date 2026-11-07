// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Carrying out cleanup actions, and the file actions of the disk map.
//!
//! Directory deletion re-checks the directory right before it runs: it must
//! still be a real directory (not a symlink) owned by the user, and still be
//! a cache by the same rule that listed it.

use crate::docker::Docker;
use crate::findings::{Action, Home};
use std::fmt::Write as _;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;

/// Carries out `action`. Returns bytes freed when the tool reports them.
pub fn run(action: &Action, home: &Home, docker: Option<&Docker>) -> Result<Option<u64>, String> {
    let need_docker = || docker.ok_or_else(|| "No container engine was found.".to_owned());
    match action {
        Action::DeleteCache(p) => {
            check_cache(p, home)?;
            std::fs::remove_dir_all(p).map_err(|e| format!("Could not delete {}: {e}", home.short(p)))?;
            Ok(None)
        }
        Action::GoCleanModcache => tool("go", &["clean", "-modcache"]).map(|()| None),
        Action::RustupUninstall(name) => tool("rustup", &["toolchain", "uninstall", name]).map(|()| None),
        Action::EmptyTrash => tool("gio", &["trash", "--empty"]).map(|()| None),
        Action::MoveToTrash(p) => trash(p).map(|()| None),
        Action::DockerImage { id, tags } => {
            let d = need_docker()?;
            for t in tags {
                d.remove_image(t)?;
            }
            // Removing the last tag usually removes the image; an untagged
            // image, or one still referenced by digest, is removed by ID.
            match d.remove_image(id) {
                Err(e) if !tags.is_empty() && e.contains("No such image") => Ok(None),
                r => r.map(|()| None),
            }
        }
        Action::DockerContainer(id) => need_docker()?.remove_container(id).map(|()| None),
        Action::DockerVolume(name) => need_docker()?.remove_volume(name).map(|()| None),
        Action::DockerBuildCache(hours) => need_docker()?.prune_build_cache(*hours).map(Some),
    }
}

/// The rule [`crate::findings`] used to list `p`, checked again now.
fn check_cache(p: &Path, home: &Home) -> Result<(), String> {
    let md = std::fs::symlink_metadata(p).map_err(|e| format!("{} is gone: {e}", home.short(p)))?;
    if !md.is_dir() {
        return Err(format!("{} is not a folder.", home.short(p)));
    }
    if md.uid() != rustix::process::getuid().as_raw() {
        return Err(format!("{} belongs to another user.", home.short(p)));
    }
    let tagged = crate::scan::has_cache_signature(&p.join("CACHEDIR.TAG"));
    let node_modules = p.file_name().is_some_and(|n| n == "node_modules");
    let app_cache = p.parent() == Some(home.cache.as_path());
    let known = home.known_caches().iter().any(|(k, _)| k == p);
    if tagged || node_modules || app_cache || known {
        Ok(())
    } else {
        Err(format!("{} is no longer marked as a cache, so it was left alone.", home.short(p)))
    }
}

/// Moves a file or folder to the Trash.
pub fn trash(p: &Path) -> Result<(), String> {
    let out = Command::new("gio").arg("trash").arg("--").arg(p).output().map_err(|e| format!("Could not run gio: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().trim_start_matches("gio: ").to_owned()) }
}

/// Opens the file manager with `p` selected, or at `p` if that fails.
pub fn show_in_files(p: &Path) -> Result<(), String> {
    let uri = file_uri(p);
    let shown = Command::new("gdbus")
        .args(["call", "--session", "--dest", "org.freedesktop.FileManager1", "--object-path", "/org/freedesktop/FileManager1"])
        .args(["--method", "org.freedesktop.FileManager1.ShowItems", &format!("['{uri}']"), ""])
        .output()
        .is_ok_and(|o| o.status.success());
    if shown {
        return Ok(());
    }
    let dir = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
    Command::new("xdg-open").arg(dir).spawn().map(drop).map_err(|e| format!("Could not open the file manager: {e}"))
}

/// A `file://` URI, percent-encoding everything but unreserved bytes and `/`.
fn file_uri(p: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            s.push(b as char);
        } else {
            let _ = write!(s, "%{b:02X}");
        }
    }
    s
}

fn tool(program: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(program).args(args).output().map_err(|e| format!("Could not run {program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program} failed: {}", err.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home(h: &Path) -> Home {
        Home {
            home: h.into(),
            cache: h.join(".cache"),
            downloads: h.join("Downloads"),
            trash: h.join(".local/share/Trash"),
            cargo: h.join(".cargo"),
            rustup: h.join(".rustup"),
        }
    }

    #[test]
    fn deletes_only_what_is_still_a_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let h = tmp.path();
        let home = home(h);
        let target = h.join("p/target");
        std::fs::create_dir_all(target.join("debug")).unwrap();
        std::fs::write(target.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55").unwrap();
        let fake = h.join("p/fake");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("CACHEDIR.TAG"), "not a tag").unwrap();
        let plain = h.join("p/src");
        std::fs::create_dir_all(&plain).unwrap();
        let link = h.join("p/link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let nm = h.join("web/node_modules");
        std::fs::create_dir_all(nm.join("x")).unwrap();

        assert!(run(&Action::DeleteCache(plain.clone()), &home, None).unwrap_err().contains("no longer marked"));
        assert!(run(&Action::DeleteCache(fake.clone()), &home, None).unwrap_err().contains("no longer marked"));
        assert!(fake.exists());
        assert!(run(&Action::DeleteCache(link.clone()), &home, None).unwrap_err().contains("not a folder"));
        assert!(plain.exists() && link.exists());
        run(&Action::DeleteCache(target.clone()), &home, None).unwrap();
        run(&Action::DeleteCache(nm.clone()), &home, None).unwrap();
        assert!(!target.exists() && !nm.exists());
        assert!(run(&Action::DeleteCache(PathBuf::from("/nonexistent/x")), &home, None).unwrap_err().contains("is gone"));
    }

    #[test]
    fn uris() {
        assert_eq!(file_uri(Path::new("/home/a b/ü.txt")), "file:///home/a%20b/%C3%BC.txt");
    }
}
