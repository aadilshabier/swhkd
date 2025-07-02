use std::{
    collections::HashSet,
    error::Error,
    fs::{File, OpenOptions},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
    process::exit,
};

use input::{
    Libinput, LibinputInterface,
};

use libc::{O_RDONLY, O_RDWR, O_WRONLY};
use tokio::io::unix::AsyncFd;

mod device;
mod uinput_layer;

struct Interface {
    devices: HashSet<String>,
}

fn handle_event(
    event: input::Event,
    uinput_dev: &mut uinput_layer::Device,
) -> Result<(), Box<dyn Error>> {
    use input::Event::*;
    match event {
        Keyboard(keyboard_event) => {
            uinput_layer::emit_libinput_keyboard_event(uinput_dev, keyboard_event)?;
        }
        Pointer(pointer_event) => {
            uinput_layer::emit_libinput_pointer_event(uinput_dev, pointer_event)?;
        }
        _ => log::info!("Event: {event:?}"),
    };
    Ok(())
}

impl Interface {
    pub fn new(path: &Path) -> std::io::Result<Self> {
        let devices = device::get_devices_from_file(path)?;
        log::debug!("Devices: {devices:?}");

        Ok(Self { devices })
    }
}

impl LibinputInterface for Interface {
    fn open_restricted(
        &mut self,
        path: &std::path::Path,
        flags: i32,
    ) -> Result<std::os::unix::io::OwnedFd, i32> {
        let name = match device::name_from_path(path) {
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
                    device::grab(file.as_raw_fd()).expect("Could not grab fd");
                    file.into()
                })
                .map_err(|err| err.raw_os_error().unwrap_or(-1))
        }
    }

    fn close_restricted(&mut self, fd: std::os::unix::io::OwnedFd) {
        device::ungrab(fd.as_raw_fd()).expect("Could not ungrab fd");
        drop(File::from(fd));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let interface = Interface::new(Path::new("./devices.txt"))?;

    let mut input = Libinput::new_with_udev(interface);
    if let Err(_) = input.udev_assign_seat("seat0") {
        log::error!("Could not assign udev to seat0");
        exit(1);
    }
    let mut input = AsyncFd::new(input)?;

    let mut uinput_dev = match uinput_layer::build_uinput_dev() {
        Err(err) => {
            log::error!("Could not create uinput device: {}", err);
            exit(1);
        }
        Ok(res) => res,
    };

    loop {
        let mut guard = input.readable_mut().await?;

        match guard.try_io(|inner| {
            let input = inner.get_mut();
            input.dispatch()?;
            for event in input {
                if let Err(err) = handle_event(event, &mut uinput_dev) {
                    log::error!("Event handling failed: {}", err);
                    exit(1);
                }
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
