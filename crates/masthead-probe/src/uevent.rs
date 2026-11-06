//! Kernel uevents over netlink, for hotplug of network interfaces, disks,
//! sensors, batteries and GPUs without polling sysfs or linking libudev.

use rustix::fd::{AsFd, BorrowedFd, OwnedFd};
use rustix::net::{AddressFamily, SocketFlags, SocketType, netlink};
use std::io;

/// Which device classes changed since the last drain.
#[allow(clippy::struct_excessive_bools)] // one flag per device class
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changes {
    pub net: bool,
    pub block: bool,
    pub hwmon: bool,
    pub power_supply: bool,
    pub drm: bool,
}

impl Changes {
    pub fn any(&self) -> bool {
        self.net || self.block || self.hwmon || self.power_supply || self.drm
    }

    /// Record one uevent payload: `ACTION@DEVPATH\0KEY=VALUE\0...`.
    pub fn note(&mut self, msg: &[u8]) {
        let mut parts = msg.split(|b| *b == 0);
        let Some(head) = parts.next() else { return };
        // `change` events fire constantly for some devices (battery levels);
        // only additions and removals alter what we monitor.
        if !(head.starts_with(b"add@") || head.starts_with(b"remove@") || head.starts_with(b"move@")) {
            return;
        }
        for kv in parts {
            if let Some(sub) = kv.strip_prefix(b"SUBSYSTEM=") {
                match sub {
                    b"net" => self.net = true,
                    b"block" => self.block = true,
                    b"hwmon" => self.hwmon = true,
                    b"power_supply" => self.power_supply = true,
                    b"drm" => self.drm = true,
                    _ => {}
                }
            }
        }
    }
}

/// A non-blocking socket subscribed to kernel uevents (multicast group 1).
#[derive(Debug)]
pub struct UeventSocket {
    fd: OwnedFd,
    buf: Vec<u8>,
}

impl UeventSocket {
    pub fn open() -> io::Result<Self> {
        let fd = rustix::net::socket_with(
            AddressFamily::NETLINK,
            SocketType::DGRAM,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            Some(netlink::KOBJECT_UEVENT),
        )?;
        rustix::net::bind(&fd, &netlink::SocketAddrNetlink::new(0, 1))?;
        Ok(Self { fd, buf: vec![0; 8192] })
    }

    /// Read every pending message without blocking.
    pub fn drain(&mut self) -> Changes {
        let mut c = Changes::default();
        while let Ok((n, _)) = rustix::net::recv(&self.fd, &mut self.buf[..], rustix::net::RecvFlags::DONTWAIT) {
            if n == 0 {
                break;
            }
            c.note(&self.buf[..n.min(self.buf.len())]);
        }
        c
    }
}

impl AsFd for UeventSocket {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl std::os::fd::AsRawFd for UeventSocket {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        std::os::fd::AsRawFd::as_raw_fd(&self.fd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_payloads() {
        let mut c = Changes::default();
        c.note(b"change@/devices/x\0ACTION=change\0SUBSYSTEM=power_supply\0");
        assert!(!c.any());
        c.note(b"add@/devices/virtual/net/veth1\0ACTION=add\0SUBSYSTEM=net\0INTERFACE=veth1\0");
        assert!(c.net && !c.block);
        c.note(b"remove@/block/sdb\0SUBSYSTEM=block\0");
        c.note(b"add@/x\0SUBSYSTEM=hwmon\0");
        c.note(b"add@/x\0SUBSYSTEM=drm\0");
        c.note(b"add@/x\0SUBSYSTEM=power_supply\0");
        c.note(b"");
        assert!(c.block && c.hwmon && c.drm && c.power_supply);
    }

    #[test]
    fn socket_opens() {
        // Opening needs no privileges; containers without netlink may refuse.
        if let Ok(mut s) = UeventSocket::open() {
            let _ = s.drain();
        }
    }
}
