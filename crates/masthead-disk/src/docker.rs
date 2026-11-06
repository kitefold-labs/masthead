// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! A small client for the Docker Engine API over its Unix socket. Podman's
//! Docker-compatible socket works too.
//!
//! Only what disk cleanup needs: `GET /system/df`, removing images,
//! containers and volumes, and pruning the build cache. Requests are plain
//! HTTP/1.1 with `Connection: close`, so no HTTP library is needed.

use serde::Deserialize;
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Docker {
    socket: PathBuf,
    /// "Docker" or "Podman", for messages.
    pub engine: &'static str,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Df {
    pub images: Vec<Image>,
    pub containers: Vec<Container>,
    pub volumes: Vec<Volume>,
    pub build_cache: Option<Vec<BuildRecord>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Image {
    pub id: String,
    pub repo_tags: Option<Vec<String>>,
    pub created: i64,
    pub size: i64,
    /// Bytes in layers other images also use; -1 when not computed.
    pub shared_size: i64,
    /// Containers using the image, running or not; -1 when not computed.
    pub containers: i64,
    pub labels: Option<std::collections::HashMap<String, String>>,
}

impl Image {
    /// Bytes only this image holds: freed by removing it alone.
    pub fn unique(&self) -> u64 {
        (self.size - self.shared_size.max(0)).max(0) as u64
    }

    /// `repo:tag` names, without the `<none>:<none>` placeholder.
    pub fn tags(&self) -> Vec<String> {
        self.repo_tags.iter().flatten().filter(|t| t.as_str() != "<none>:<none>").cloned().collect()
    }

    pub fn short_id(&self) -> &str {
        let id = self.id.strip_prefix("sha256:").unwrap_or(&self.id);
        &id[..id.len().min(12)]
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Container {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    pub created: i64,
    pub state: String,
    pub status: String,
    pub size_rw: Option<i64>,
}

impl Container {
    pub fn name(&self) -> &str {
        self.names.first().map_or(&self.id[..self.id.len().min(12)], |n| n.trim_start_matches('/'))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Volume {
    pub name: String,
    pub created_at: String,
    pub labels: Option<std::collections::HashMap<String, String>>,
    pub usage_data: Option<VolumeUsage>,
}

impl Volume {
    pub fn anonymous(&self) -> bool {
        self.labels.as_ref().is_some_and(|l| l.contains_key("com.docker.volume.anonymous"))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct VolumeUsage {
    pub size: i64,
    pub ref_count: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct BuildRecord {
    #[serde(rename = "ID")]
    pub id: String,
    pub in_use: bool,
    /// Shared with other records or images; pruning this record alone
    /// frees nothing.
    pub shared: bool,
    pub size: i64,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Pruned {
    #[serde(default)]
    space_reclaimed: u64,
}

impl Docker {
    /// The engine socket from `DOCKER_HOST`, the system Docker socket, or the
    /// user's Podman socket, whichever exists first.
    pub fn find() -> Option<Self> {
        let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
        if let Ok(h) = std::env::var("DOCKER_HOST")
            && let Some(p) = h.strip_prefix("unix://")
        {
            candidates.push((p.into(), "Docker"));
        }
        candidates.push(("/var/run/docker.sock".into(), "Docker"));
        if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
            candidates.push((PathBuf::from(rt).join("podman/podman.sock"), "Podman"));
        }
        candidates.into_iter().find(|(p, _)| p.exists()).map(|(socket, engine)| Self { socket, engine })
    }

    pub fn df(&self) -> Result<Df, String> {
        let body = self.call("GET", "/system/df", Duration::from_mins(5))?;
        serde_json::from_slice(&body).map_err(|e| format!("{} returned unexpected data: {e}", self.engine))
    }

    /// Removes an image by tag or ID. Refuses images a container uses.
    pub fn remove_image(&self, reference: &str) -> Result<(), String> {
        self.call("DELETE", &format!("/images/{}", encode(reference)), Duration::from_mins(2)).map(drop)
    }

    /// Removes a stopped container. Refuses a running one.
    pub fn remove_container(&self, id: &str) -> Result<(), String> {
        self.call("DELETE", &format!("/containers/{}", encode(id)), Duration::from_mins(1)).map(drop)
    }

    /// Removes a volume no container uses.
    pub fn remove_volume(&self, name: &str) -> Result<(), String> {
        self.call("DELETE", &format!("/volumes/{}", encode(name)), Duration::from_mins(1)).map(drop)
    }

    /// Prunes build cache unused for `older_than_hours`, or all unused cache
    /// when `None`. Returns bytes freed.
    pub fn prune_build_cache(&self, older_than_hours: Option<u32>) -> Result<u64, String> {
        let path = match older_than_hours {
            Some(h) => format!("/build/prune?filters={}", encode(&format!(r#"{{"until":["{h}h"]}}"#))),
            None => "/build/prune".to_owned(),
        };
        let body = self.call("POST", &path, Duration::from_mins(15))?;
        serde_json::from_slice::<Pruned>(&body).map(|p| p.space_reclaimed).map_err(|e| e.to_string())
    }

    fn call(&self, method: &str, path: &str, timeout: Duration) -> Result<Vec<u8>, String> {
        let engine = self.engine;
        let mut s = UnixStream::connect(&self.socket).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => {
                format!("No permission to use {engine} at {}. Add yourself to the docker group.", self.socket.display())
            }
            _ => format!("{engine} is not running ({e})."),
        })?;
        s.set_read_timeout(Some(timeout)).ok();
        s.set_write_timeout(Some(Duration::from_secs(10))).ok();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: docker\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).map_err(|e| format!("{engine} did not answer: {e}"))?;
        let (status, body) = parse_response(&raw).ok_or_else(|| format!("{engine} sent a malformed response."))?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            #[derive(Deserialize)]
            struct Msg {
                message: String,
            }
            let msg = serde_json::from_slice::<Msg>(&body).map_or_else(|_| String::from_utf8_lossy(&body).into_owned(), |m| m.message);
            Err(msg.trim().to_owned())
        }
    }
}

/// Status code and body, with chunked transfer encoding undone.
fn parse_response(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..split]).ok()?;
    let body = &raw[split + 4..];
    let status = head.split(' ').nth(1)?.parse().ok()?;
    let chunked = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("transfer-encoding:") && l.contains("chunked")
    });
    if !chunked {
        return Some((status, body.to_vec()));
    }
    let mut out = Vec::new();
    let mut rest = body;
    loop {
        let eol = rest.windows(2).position(|w| w == b"\r\n")?;
        let len_str = std::str::from_utf8(&rest[..eol]).ok()?;
        let len = usize::from_str_radix(len_str.split(';').next()?.trim(), 16).ok()?;
        rest = &rest[eol + 2..];
        if len == 0 {
            return Some((status, out));
        }
        out.extend_from_slice(rest.get(..len)?);
        rest = rest.get(len + 2..)?;
    }
}

/// Percent-encodes everything but unreserved characters and `:`.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(parse_response(plain), Some((200, b"{}".to_vec())));
        let chunked = b"HTTP/1.1 409 Conflict\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"me\r\n9;x=y\r\nssage\":1}\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked), Some((409, b"{\"message\":1}".to_vec())));
        assert_eq!(parse_response(b"garbage"), None);
    }

    #[test]
    fn encodes_references() {
        assert_eq!(encode("ghcr.io/a/b:1.2"), "ghcr.io%2Fa%2Fb:1.2");
        assert_eq!(encode(r#"{"until":["720h"]}"#), "%7B%22until%22:%5B%22720h%22%5D%7D");
    }

    #[test]
    fn parses_system_df() {
        let json = r#"{"Images":[{"Id":"sha256:0123456789abcdef","RepoTags":["<none>:<none>"],"Created":5,"Size":100,"SharedSize":40,"Containers":0}],
            "Containers":[{"Id":"c1","Names":["/web"],"Image":"nginx","Created":1,"State":"exited","Status":"Exited (0)","SizeRw":7}],
            "Volumes":[{"Name":"v","CreatedAt":"2026-01-01T00:00:00Z","Labels":{"com.docker.volume.anonymous":""},"UsageData":{"Size":9,"RefCount":0}}],
            "BuildCache":[{"ID":"b","InUse":false,"Shared":false,"Size":3,"CreatedAt":"2026-01-01T00:00:00Z","LastUsedAt":null}],
            "Future":true}"#;
        let df: Df = serde_json::from_str(json).unwrap();
        assert_eq!(df.images[0].unique(), 60);
        assert_eq!(df.images[0].tags().len(), 0);
        assert_eq!(df.images[0].short_id(), "0123456789ab");
        assert_eq!(df.containers[0].name(), "web");
        assert!(df.volumes[0].anonymous());
        assert_eq!(df.build_cache.unwrap()[0].size, 3);
    }
}
