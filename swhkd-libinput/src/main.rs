use input::{Libinput, LibinputInterface};
use libc::{O_RDONLY, O_RDWR, O_WRONLY};
use std::fs::{File, OpenOptions};
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::Path;
use tokio::io::unix::AsyncFd;
use std::io;

use nix::ioctl_write_int;

ioctl_write_int!(eviocgrab, b'E', 0x90);

pub fn grab(fd: i32) -> std::io::Result<()> {
    unsafe {
        eviocgrab(fd, 1)?;
    }
    Ok(())
}

pub fn ungrab(fd: i32) -> std::io::Result<()> {
    unsafe {
        eviocgrab(fd, 0)?;
    }
    Ok(())
}

struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        OpenOptions::new()
            .custom_flags(flags)
            .read((flags & O_RDONLY != 0) | (flags & O_RDWR != 0))
            .write((flags & O_WRONLY != 0) | (flags & O_RDWR != 0))
            .open(path)
            .map(|file| {
                // grab(file.as_raw_fd()).expect("Could not grab file");
                file.into()
            })
            .map_err(|err| err.raw_os_error().unwrap())
    }

    fn close_restricted(&mut self, fd: OwnedFd) {
        // ungrab(fd.as_raw_fd()).expect("Could not ungrab file");
        drop(File::from(fd));
    }
}


#[tokio::main]
async fn main() -> io::Result<()> {
    let mut input = Libinput::new_with_udev(Interface);
    input.udev_assign_seat("seat0").unwrap();
    let mut input = AsyncFd::new(input)?;
    loop {
        let mut guard = input.readable_mut().await?;
        
        guard.try_io(|inner| {
            let input = inner.get_mut();
            input.dispatch()?;
            for event in input {
                println!("Got event: {:?}", event);
            }
            Ok(())
        }).unwrap()?;
        // NOTE: this is very important!!
        guard.clear_ready();
    }
}
