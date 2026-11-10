//! Minimal Linux hidraw access: discovery through sysfs, report I/O through the
//! character device and feature reports through `HIDIOC[SG]FEATURE` ioctls.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::protocol;

/// Static information about a hidraw node that belongs to a supported receiver.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DeviceInfo {
    /// `/dev/hidrawN`
    pub devnode: PathBuf,
    pub vendor_id: u16,
    pub product_id: u16,
    /// USB product string, e.g. "MCHOSE V9 PRO".
    pub product: String,
    pub manufacturer: String,
    pub serial: String,
    /// Kernel HID physical path (stable while plugged into the same port).
    pub phys: String,
}

/// Lists hidraw nodes that expose the MCHOSE vendor interface.
pub fn enumerate() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/hidraw") else {
        return out;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let sys = entry.path();
        if let Some(info) = probe(&sys) {
            out.push(info);
        }
    }
    out
}

fn read_trimmed(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

fn probe(sys: &Path) -> Option<DeviceInfo> {
    let dev = sys.join("device");
    let uevent = fs::read_to_string(dev.join("uevent")).ok()?;
    let mut hid_id = None;
    let mut phys = String::new();
    let mut uniq = String::new();
    let mut hid_name = String::new();
    for line in uevent.lines() {
        if let Some(v) = line.strip_prefix("HID_ID=") {
            hid_id = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("HID_PHYS=") {
            phys = v.to_string();
        } else if let Some(v) = line.strip_prefix("HID_UNIQ=") {
            uniq = v.to_string();
        } else if let Some(v) = line.strip_prefix("HID_NAME=") {
            hid_name = v.to_string();
        }
    }
    // HID_ID=0003:0000291D:0000385D  (bus:vendor:product)
    let hid_id = hid_id?;
    let mut parts = hid_id.split(':');
    let bus = parts.next()?;
    let vid = u32::from_str_radix(parts.next()?, 16).ok()? as u16;
    let pid = u32::from_str_radix(parts.next()?, 16).ok()? as u16;
    if bus != "0003" || vid != protocol::VENDOR_ID || pid != protocol::PRODUCT_ID {
        return None;
    }
    let descriptor = fs::read(dev.join("report_descriptor")).ok()?;
    if !protocol::is_vendor_interface(&descriptor) {
        return None;
    }
    // device -> .../<usb-dev>/<usb-dev>:<cfg>.<intf>/<hid-dev>; strings live on <usb-dev>.
    let usb_dev = fs::canonicalize(&dev).ok()?.parent()?.parent()?.to_path_buf();
    let product = read_trimmed(usb_dev.join("product"))
        .unwrap_or_else(|| hid_name.trim_start_matches("C-Media Electronics Inc").trim().to_string());
    let manufacturer = read_trimmed(usb_dev.join("manufacturer")).unwrap_or_default();
    let serial = read_trimmed(usb_dev.join("serial")).unwrap_or(uniq);
    Some(DeviceInfo {
        devnode: Path::new("/dev").join(sys.file_name()?),
        vendor_id: vid,
        product_id: pid,
        product,
        manufacturer,
        serial,
        phys,
    })
}

/// An open hidraw character device.
#[derive(Debug)]
pub struct Hidraw {
    fd: OwnedFd,
}

// _IOC(dir, type, nr, size) as defined in <asm-generic/ioctl.h>
const fn ioc(dir: u64, ty: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (ty << 8) | nr
}
const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;
const fn hidioc_sfeature(len: usize) -> u64 {
    ioc(IOC_WRITE | IOC_READ, b'H' as u64, 0x06, len as u64)
}
const fn hidioc_gfeature(len: usize) -> u64 {
    ioc(IOC_WRITE | IOC_READ, b'H' as u64, 0x07, len as u64)
}

impl Hidraw {
    pub fn open(path: &Path) -> io::Result<Self> {
        let c = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        // SAFETY: plain open(2) with a valid NUL-terminated path.
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fd is a freshly opened, owned descriptor.
        Ok(Self { fd: unsafe { OwnedFd::from_raw_fd(fd) } })
    }

    /// Sends an output report. `frame[0]` must be the report ID.
    pub fn write(&self, frame: &[u8]) -> io::Result<()> {
        // SAFETY: buffer pointer/length are valid for the duration of the call.
        let n = unsafe { libc::write(self.fd.as_raw_fd(), frame.as_ptr().cast(), frame.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if n as usize != frame.len() {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short HID write"));
        }
        Ok(())
    }

    /// Waits up to `timeout_ms` for an input report. Returns `Ok(None)` on timeout.
    /// The returned frame starts with the report ID.
    pub fn read_timeout(&self, timeout_ms: i32) -> io::Result<Option<Vec<u8>>> {
        let mut pfd = libc::pollfd { fd: self.fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: single valid pollfd.
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        if r == 0 {
            return Ok(None);
        }
        if pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "HID device went away"));
        }
        let mut buf = vec![0u8; 256];
        // SAFETY: buffer is valid for buf.len() bytes.
        let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::WouldBlock { Ok(None) } else { Err(e) };
        }
        buf.truncate(n as usize);
        Ok(Some(buf))
    }

    /// SET_FEATURE. `frame[0]` must be the report ID.
    pub fn set_feature(&self, frame: &[u8]) -> io::Result<()> {
        let mut buf = frame.to_vec();
        // SAFETY: ioctl reads `buf.len()` bytes from a valid buffer.
        let r = unsafe { libc::ioctl(self.fd.as_raw_fd(), hidioc_sfeature(buf.len()) as _, buf.as_mut_ptr()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// GET_FEATURE. Returns the frame including the report ID at index 0.
    pub fn get_feature(&self, report_id: u8, len: usize) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        buf[0] = report_id;
        // SAFETY: ioctl writes at most `buf.len()` bytes into a valid buffer.
        let r = unsafe { libc::ioctl(self.fd.as_raw_fd(), hidioc_gfeature(buf.len()) as _, buf.as_mut_ptr()) };
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(r as usize);
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_kernel_headers() {
        // Values computed by <linux/hidraw.h> for a 64-byte buffer.
        assert_eq!(hidioc_sfeature(64), 0xC040_4806);
        assert_eq!(hidioc_gfeature(64), 0xC040_4807);
    }
}
