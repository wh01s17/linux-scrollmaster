// SPDX-License-Identifier: GPL-3.0-or-later
use super::{session::check_deadline, Transport};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::AsRawFd,
    time::Instant,
};

/// Raw Linux report I/O. N2 must discover/validate a device before supplying its file.
/// Caller transfers a unique open file description (no dup/try_clone/other reader).
/// This type deliberately provides no discovery, arbitrary path or IPC entry point.
pub struct Hidraw {
    file: File,
}
impl Hidraw {
    pub fn from_file(file: File) -> io::Result<Self> {
        // SAFETY: the owned File keeps this descriptor valid through each fcntl call.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        let result =
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { file })
    }

    fn wait(&self, events: libc::c_short, deadline: Instant) -> io::Result<()> {
        loop {
            let remaining = check_deadline(deadline)?;
            let millis = remaining
                .as_millis()
                .saturating_add(1)
                .min(i32::MAX as u128) as i32;
            let mut fd = libc::pollfd {
                fd: self.file.as_raw_fd(),
                events,
                revents: 0,
            };
            // SAFETY: one initialized pollfd, valid for the duration of poll.
            let result = unsafe { libc::poll(&mut fd, 1, millis) };
            if result < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            check_deadline(deadline)?;
            if result == 0 {
                continue;
            }
            // Consume an available final report even when HUP is also set.
            if fd.revents & events != 0 {
                return Ok(());
            }
            if fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "HID++ descriptor disconnected",
                ));
            }
        }
    }
}
impl Transport for Hidraw {
    fn write_report(&mut self, report: &[u8], deadline: Instant) -> io::Result<()> {
        loop {
            self.wait(libc::POLLOUT, deadline)?;
            match self.file.write(report) {
                Ok(n) if n == report.len() => return check_deadline(deadline).map(|_| ()),
                // Never write a remainder as a second HID report.
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Partial HID++ write",
                    ))
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn read_report(&mut self, deadline: Instant) -> io::Result<Vec<u8>> {
        loop {
            self.wait(libc::POLLIN, deadline)?;
            // Extra byte detects an oversized report instead of silently accepting a prefix.
            let mut bytes = [0; 21];
            match self.file.read(&mut bytes) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "HID++ EOF")),
                Ok(n) => return Ok(bytes[..n].to_vec()),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                    ) =>
                {
                    continue
                }
                Err(error) => return Err(error),
            }
        }
    }
}
