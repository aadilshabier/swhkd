use input::event::keyboard::{KeyState, KeyboardEventTrait};
use input::{Libinput, LibinputInterface};
use libc::{O_RDONLY, O_RDWR, O_WRONLY};
use std::fs::{File, OpenOptions};
use std::os::unix::{fs::OpenOptionsExt, io::OwnedFd};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use nix::ioctl_write_int;

const MAX_DELAY: Duration = Duration::from_millis(20);

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


fn main() {
    
    let mut input = Libinput::new_with_udev(Interface);
    input.udev_assign_seat("seat0").unwrap();
    let mut last = Instant::now();
    loop {
        input.dispatch().unwrap();
        for event in &mut input {
            use input::event::{Event::*, KeyboardEvent::Key};
            // let x = event.device().;
            if let Keyboard(Key(x)) = &event {
                if x.key_state() == KeyState::Pressed {
                    println!("Pressed {}", x.key());
                }
            }
            println!("Got event: {:?}", event);
        }
        let now = Instant::now();
        let delta = now - last;
        if MAX_DELAY > delta {
            sleep(MAX_DELAY - delta);
        }
        last = Instant::now();
    }
}
