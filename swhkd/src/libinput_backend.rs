use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fs::{File, OpenOptions},
    io,
    os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt,
};

use evdev::{uinput::VirtualDevice, InputEvent, RelativeAxisType};
use input::{
    event::{keyboard::KeyboardEventTrait, pointer::PointerScrollEvent, EventTrait},
    Device, Event, Libinput, LibinputInterface,
};
use libc::{O_RDONLY, O_RDWR, O_WRONLY};
use nix::ioctl_write_int;
use smallvec::SmallVec;
use tokio::io::unix::AsyncFd;

use crate::backend::Backend;
use crate::uinput;

struct LibinputEvent {
    inner_event: Event,
}

impl LibinputEvent {
    pub fn new(event: Event) -> Self {
        Self { inner_event: event }
    }
}

impl TryFrom<LibinputEvent> for SmallVec<[evdev::InputEvent; 2]> {
    type Error = ();

    fn try_from(value: LibinputEvent) -> Result<Self, Self::Error> {
        use evdev::EventType;
        use input::event::Event;
        let mut other_value: Option<i32> = None;
        let (type_, code, value): (EventType, u16, i32) = match value.inner_event {
            Event::Keyboard(kbd_event) => {
                (EventType::KEY, kbd_event.key() as u16, 1 - kbd_event.key_state() as i32)
            }
            Event::Pointer(ptr_event) => {
                use input::event::PointerEvent::*;
                match ptr_event {
                    Motion(motion_event) => {
                        // Seperate one libinput event into 2 evdev events by setting the optional value `other_value`.
                        let type_ = EventType::RELATIVE;
                        let value = motion_event.dx() as i32;
                        other_value = Some(motion_event.dy() as i32);
                        let code = RelativeAxisType::REL_X.0;
                        (type_, code, value)
                    }
                    MotionAbsolute(_) => todo!(),
                    Button(btn_event) => (
                        EventType::KEY,
                        btn_event.button() as u16,
                        1 - btn_event.button_state() as i32,
                    ),
                    ScrollWheel(wheel_event) => {
                        use input::event::pointer::Axis;
                        let type_ = EventType::RELATIVE;
                        let mut value = 0;
                        let code = if wheel_event.has_axis(Axis::Vertical) {
                            value = -(wheel_event.scroll_value_v120(Axis::Vertical) / 120.0) as i32;
                            RelativeAxisType::REL_WHEEL.0
                        } else {
                            value =
                                -(wheel_event.scroll_value_v120(Axis::Horizontal) / 120.0) as i32;
                            RelativeAxisType::REL_HWHEEL.0
                        };
                        (type_, code, value)
                    }
                    ScrollFinger(_) => todo!(),
                    ScrollContinuous(_) => todo!(),
                    _ => {
                        return Err(());
                    }
                }
            }
            Event::Device(_) => {
                return Err(());
            }
            Event::Switch(switch_event) => {
                use input::event::switch;
                if let switch::SwitchEvent::Toggle(switch_event) = switch_event {
                    let type_ = EventType::SWITCH;
                    let code = match switch_event.switch() {
                        Some(switch::Switch::Lid) => evdev::SwitchType::SW_LID.0,
                        Some(switch::Switch::TabletMode) => evdev::SwitchType::SW_TABLET_MODE.0,
                        Some(_) | None => {
                            return Err(());
                        }
                    };
                    let value = 1 - switch_event.switch_state() as i32;
                    (type_, code, value)
                } else {
                    return Err(());
                }
            }
            _ => {
                return Err(());
            }
        };
        let mut result = SmallVec::new();
        result.push(InputEvent::new(type_, code, value));
        // For EventType::RELATIVE, we push an additional event for movement in the Y direction.
        if type_ == EventType::RELATIVE && code == RelativeAxisType::REL_X.0 {
            if let Some(other_value) = other_value {
                result.push(InputEvent::new(type_, RelativeAxisType::REL_Y.0, other_value));
            }
        }
        Ok(result)
    }
}

ioctl_write_int!(eviocgrab, b'E', 0x90);

fn grab(fd: i32) -> io::Result<()> {
    unsafe {
        eviocgrab(fd, 1)?;
    }
    Ok(())
}

fn ungrab(fd: i32) -> io::Result<()> {
    unsafe {
        eviocgrab(fd, 0)?;
    }
    Ok(())
}

struct Interface {}

impl LibinputInterface for Interface {
    fn open_restricted(
        &mut self,
        path: &std::path::Path,
        flags: i32,
    ) -> Result<std::os::unix::prelude::OwnedFd, i32> {
        match OpenOptions::new()
            .custom_flags(flags)
            .read((flags & O_RDONLY != 0) | (flags & O_RDWR != 0))
            .write((flags & O_WRONLY != 0) | (flags & O_RDWR != 0))
            .open(path)
        {
            Ok(file) => {
                if grab(file.as_raw_fd()).is_err() {
                    log::error!("Could not grab file: {:?}", path);
                    Err(-1)
                } else {
                    Ok(file.into())
                }
            }
            Err(err) => Err(err.raw_os_error().unwrap_or(-1)),
        }
    }

    fn close_restricted(&mut self, fd: std::os::unix::prelude::OwnedFd) {
        if let Err(err) = ungrab(fd.as_raw_fd()) {
            log::error!("Could not ungrab fd: {}", err);
        }
        drop(File::from(fd));
    }
}

pub struct LibinputBackend {
    arg_devices: Vec<String>,
    devices: HashMap<String, Device>,
    input: AsyncFd<Libinput>,
    input_event_queue: VecDeque<(String, InputEvent)>,
    input_queue: VecDeque<Event>,
    uinput_device: Option<VirtualDevice>,
    uinput_switches_device: Option<VirtualDevice>,
}

impl LibinputBackend {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let input = AsyncFd::new(Libinput::new_from_path(Interface {}))?;
        Ok(Self {
            arg_devices: vec![],
            devices: HashMap::new(),
            input,
            input_event_queue: VecDeque::new(),
            input_queue: VecDeque::new(),
            uinput_device: None,
            uinput_switches_device: None,
        })
    }
}

impl Backend for LibinputBackend {
    fn get_initial_devices(
        &mut self,
        arg_devices: &[String],
    ) -> Result<Vec<String>, Box<dyn Error>> {
        self.arg_devices.clear();
        self.arg_devices.extend_from_slice(arg_devices);
        let devices: Vec<_> = if self.arg_devices.is_empty() {
            log::trace!("Attempting to find all keyboard file descriptors.");
            evdev::enumerate()
                .filter_map(|(path, _)| path.to_str().map(|p| p.to_string()))
                .collect()
        } else {
            log::trace!("arg_devices: {:?}", arg_devices);
            evdev::enumerate()
                .filter_map(|(path, dev)| {
                    log::trace!("Checking device: {}", dev.name().unwrap_or_default());
                    if self.arg_devices.contains(&dev.name().unwrap_or("").to_string()) {
                        path.to_str().map(|p| p.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        };
        Ok(devices
            .into_iter()
            .filter_map(|path| {
                if let Some(dev) = self.input.get_mut().path_add_device(&path) {
                    self.devices.insert(path.clone(), dev);
                    log::info!("Initial device added: {}", path);
                    Some(path)
                } else {
                    None
                }
            })
            .collect())
    }

    fn add_device(&mut self, path: &str) -> bool {
        if self.devices.contains_key(path) {
            log::error!("Device '{}' was already added!", path);
            return false;
        } else if !self.arg_devices.is_empty() && !self.arg_devices.contains(&path.to_owned()) {
            return false;
        }
        if let Some(dev) = self.input.get_mut().path_add_device(path) {
            log::info!("Device added: '{}'", path);
            self.devices.insert(path.to_string(), dev);
            true
        } else {
            log::error!("Could not add device: '{}'.", path);
            false
        }
    }

    fn remove_device(&mut self, path: &str) -> bool {
        if let Some(dev) = self.devices.remove(path) {
            log::info!("Removing device: '{}'", path);
            self.input.get_mut().path_remove_device(dev);
            true
        } else {
            false
        }
    }

    fn create_uinput_devices(&mut self) -> Result<(), Box<dyn Error>> {
        // Apparently, having a single uinput device with keys, relative axes and switches
        // prevents some libraries to listen to these events. The easy fix is to have separate
        // virtual devices, one for keys and relative axes (`uinput_device`) and another one
        // just for switches (`uinput_switches_device`).
        self.uinput_device = Some(uinput::create_uinput_device()?);
        self.uinput_switches_device = Some(uinput::create_uinput_switches_device()?);

        Ok(())
    }

    async fn next_event(&mut self) -> Option<(String, InputEvent)> {
        loop {
            // clear all pending events from the queues.
            while let Some((device, event)) = self.input_event_queue.pop_front() {
                return Some((device, event));
            }
            while let Some(event) = self.input_queue.pop_front() {
                let device = unsafe { event.device().udev_device() }
                    .unwrap()
                    .devnode()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();
                let res: Result<SmallVec<[InputEvent; _]>, ()> =
                    LibinputEvent::new(event).try_into();
                if let Ok(events) = res {
                    let event = events[0];
                    if events.len() > 1 {
                        self.input_event_queue.push_back((device.clone(), events[1]));
                    }
                    return Some((device, event));
                }
            }

            let mut guard = self.input.readable_mut().await.ok()?;
            match guard.try_io(|inner| {
                let input = inner.get_mut();
                input.dispatch()?;
                self.input_queue.extend(input.into_iter());
                Ok(())
            }) {
                Ok(x) => {
                    x.ok()?;
                }
                Err(_would_block) => {}
            };
            guard.clear_ready();
        }
    }

    fn emit_event(&mut self, event: &InputEvent) -> Result<(), io::Error> {
        if let Some(dev) = &mut self.uinput_device {
            return dev.emit(&[*event]);
        };
        Ok(())
    }

    fn emit_switch_event(&mut self, event: &InputEvent) -> Result<(), io::Error> {
        if let Some(dev) = &mut self.uinput_switches_device {
            return dev.emit(&[*event]);
        };
        Ok(())
    }
}
