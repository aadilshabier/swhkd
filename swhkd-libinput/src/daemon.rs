use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::BufRead,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};

use input::{
    Libinput, LibinputInterface,
    event::{EventTrait, keyboard::KeyboardEventTrait},
};
use libc::{O_RDONLY, O_RDWR, O_WRONLY};
use nix::ioctl_write_int;
use tokio::io::unix::AsyncFd;

ioctl_write_int!(eviocgrab, b'E', 0x90);

fn grab(fd: i32) -> std::io::Result<()> {
    unsafe {
        eviocgrab(fd, 1)?;
    }
    Ok(())
}

fn ungrab(fd: i32) -> std::io::Result<()> {
    unsafe {
        eviocgrab(fd, 0)?;
    }
    Ok(())
}

fn name_from_path(path: &Path) -> std::io::Result<String> {
    let ev = path.strip_prefix("/dev/input").expect("This path should begin with /dev/input");
    let name_path = Path::new("/sys/class/input/").join(ev).join("device/name");
    let name = std::fs::read_to_string(name_path)?.trim_ascii_end().to_string();
    Ok(name)
}

fn get_devices_from_file(path: &Path) -> std::io::Result<HashSet<String>> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let result = reader.lines().map(|x| x.unwrap().trim_ascii_end().to_string()).collect();
    Ok(result)
}

struct Interface {
    devices: HashSet<String>,
}

impl Interface {
    pub fn new(path: &Path) -> std::io::Result<Self> {
        let devices = get_devices_from_file(path)?;
        log::debug!("Devices: {:?}", devices);

        Ok(Self { devices })
    }
}

impl LibinputInterface for Interface {
    fn open_restricted(
        &mut self,
        path: &std::path::Path,
        flags: i32,
    ) -> Result<std::os::unix::io::OwnedFd, i32> {
        let name = match name_from_path(path) {
            Ok(name) => name,
            Err(err) => return Err(err.raw_os_error().unwrap_or(-1)),
        };
        if !self.devices.contains(&name) {
            log::debug!("Skipping: {}, {}", path.to_str().unwrap(), name.trim_ascii_end());
            Err(-414)
        } else {
            log::debug!("Opening: {}, {}", path.to_str().unwrap(), name.trim_ascii_end());
            OpenOptions::new()
                .custom_flags(flags)
                .read((flags & O_RDONLY != 0) | (flags & O_RDWR != 0))
                .write((flags & O_WRONLY != 0) | (flags & O_RDWR != 0))
                .open(path)
                .map(|file| {
                    grab(file.as_raw_fd()).expect("Could not grab fd");
                    file.into()
                })
                .map_err(|err| err.raw_os_error().unwrap())
        }
    }

    fn close_restricted(&mut self, fd: std::os::unix::io::OwnedFd) {
        ungrab(fd.as_raw_fd()).expect("Could not ungrab fd");
        drop(File::from(fd));
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let interface = Interface::new(Path::new("./devices.txt"))?;

    let mut input = Libinput::new_with_udev(interface);
    input.udev_assign_seat("seat0").unwrap();
    let mut input = AsyncFd::new(input)?;

    loop {
        let mut guard = input.readable_mut().await?;

        match guard.try_io(|inner| {
            let input = inner.get_mut();
            input.dispatch()?;
            for event in input {
                match event {
                    input::Event::Keyboard(keyboard_event) => {
                        let device = keyboard_event.device().name().to_string();
                        let key = keyboard_event.key();
                        let key_state = keyboard_event.key_state();
                        log::info!(
                            "Device: {}, ev: {}, key pressed: {}, state: {:?}",
                            device,
                            keyboard_event.device().sysname(),
                            key,
                            key_state
                        );
                    }
                    _ => log::info!("Event: {:?}", event),
                };
            }
            Ok(())
        }) {
            Ok(x) => {
                x?;
            }
            Err(_would_block) => {}
        };

        // NOTE: very important, without this the fd is always ready to read from
        guard.clear_ready();
    }
}
