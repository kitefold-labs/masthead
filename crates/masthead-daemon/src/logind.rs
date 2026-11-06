// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Whether the graphical session is locked, from logind's `LockedHint`.
//!
//! The daemon runs as a systemd user service outside any session scope, so it
//! asks for the user's display session and watches that session's property.
//! zbus keeps the property cached and updated from `PropertiesChanged`, so
//! reading it each tick costs no D-Bus round trip.

use zbus::OwnedObjectPath;
use zbus::{Connection, Proxy};

#[derive(Debug, Default)]
pub struct LockState {
    session: Option<Proxy<'static>>,
}

impl LockState {
    pub async fn connect() -> Self {
        match Self::try_connect().await {
            Ok(p) => Self { session: Some(p) },
            Err(e) => {
                log::info!("logind unavailable, lock detection off: {e}");
                Self::default()
            }
        }
    }

    async fn try_connect() -> zbus::Result<Proxy<'static>> {
        let sys = Connection::system().await?;
        let user = Proxy::new(&sys, "org.freedesktop.login1", "/org/freedesktop/login1/user/self", "org.freedesktop.login1.User").await?;
        let (_, path): (String, OwnedObjectPath) = user.get_property("Display").await?;
        if path.as_str() == "/" {
            return Err(zbus::Error::Failure("no display session".into()));
        }
        Proxy::new(&sys, "org.freedesktop.login1", path, "org.freedesktop.login1.Session").await
    }

    pub fn locked(&self) -> bool {
        self.session.as_ref().and_then(|p| p.cached_property::<bool>("LockedHint").ok().flatten()).unwrap_or(false)
    }
}
