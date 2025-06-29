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

fn build_uinput_dev() -> Result<uinput::Device, ()> {
    use uinput::event;
    let mut builder = uinput::default().expect("uinput module not loaded");
    // Keyboard
    builder = builder.name("uinput device").unwrap().event(event::Keyboard::All).unwrap();
    // Mouse buttons
    for event in event::controller::Mouse::iter_variants() {
        builder = builder.event(event).unwrap();
    }
    // Mouse Movement
    for event in event::relative::Position::iter_variants() {
        builder = builder.event(event).unwrap()
    }
    // Mouse Wheel
    for event in event::relative::Wheel::iter_variants() {
        builder = builder.event(event).unwrap()
    }
    builder.create().map_err(|_| ())
}

fn handle_event(event: input::Event, uinput_dev: &mut uinput::Device) -> Result<(), ()> {
    use input::Event::*;
    match event {
        Keyboard(keyboard_event) => {
            emit_libinput_keyboard_event(uinput_dev, keyboard_event).unwrap();
        }
        Pointer(pointer_event) => {
            emit_libinput_pointer_event(uinput_dev, pointer_event).unwrap();
        }
        _ => log::info!("Event: {event:?}"),
    };
    Ok(())
}

fn emit_libinput_keyboard_event(
    device: &mut uinput::Device,
    keyboard_event: input::event::KeyboardEvent,
) -> Result<(), ()> {
    let device_name = keyboard_event.device().name().to_string();
    let key_code = keyboard_event.key() as i32;
    let state = keyboard_event.key_state() as i32;
    log::info!("Device: {device_name}, key: {key_code}, state: {state:?}");

    device.write(1, key_code, 1 - state).unwrap();
    device.synchronize().unwrap();
    Ok(())
}

fn emit_libinput_pointer_event(
    device: &mut uinput::Device,
    pointer_event: input::event::PointerEvent,
) -> Result<(), ()> {
    use input::event::PointerEvent::*;
    match pointer_event {
        Motion(motion_event) => {
            use uinput::event::{Relative, relative::Position};
            let dx = motion_event.dx_unaccelerated();
            let dy = motion_event.dy_unaccelerated();
            log::info!("Mouse: {}, dx: {dx:.2}, dy: {dy:.2}", motion_event.device().name(),);
            device.send(Relative::Position(Position::X), dx as i32).unwrap();
            device.send(Relative::Position(Position::Y), dy as i32).unwrap();
            device.synchronize().unwrap();
        }
        Button(button_event) => {
            let button = button_event.button() as i32;
            let state = button_event.button_state() as i32;
            log::info!(
                "Mouse: {}, button: {button}, state: {state:?}",
                button_event.device().name(),
            );
            device.write(1, button, 1 - state).unwrap();
            device.synchronize().unwrap();
        }
        ScrollWheel(scrollwheel_event) => {
            use uinput::event::{Relative, relative::Wheel};
            let vert = scrollwheel_event.scroll_value_v120(input::event::pointer::Axis::Vertical);
            let hori = scrollwheel_event.scroll_value_v120(input::event::pointer::Axis::Horizontal);
            log::info!(
                "Mouse: {}, wheel vert: {vert}, hori: {hori}",
                scrollwheel_event.device().name(),
            );
            let (event, value) = if vert != 0.0 {
                (Relative::Wheel(Wheel::Vertical), -vert/120.0)
            } else {
                (Relative::Wheel(Wheel::Horizontal), -hori/120.0)
            };
            device.send(event, value as i32).unwrap();
            device.synchronize().unwrap();
        }
        _ => {
            log::info!("Mouse: {}, event: {:?}", pointer_event.device().name(), pointer_event);
        }
    }
    Ok(())
}

impl Interface {
    pub fn new(path: &Path) -> std::io::Result<Self> {
        let devices = get_devices_from_file(path)?;
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

    let mut uinput_dev = build_uinput_dev().unwrap();

    loop {
        let mut guard = input.readable_mut().await?;

        match guard.try_io(|inner| {
            let input = inner.get_mut();
            input.dispatch()?;
            for event in input {
                handle_event(event, &mut uinput_dev).unwrap();
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
