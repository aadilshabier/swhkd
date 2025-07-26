use std::{error::Error, io};

use evdev::{uinput::VirtualDevice, Device, EventStream, InputEvent, Key};
use tokio_stream::{StreamExt, StreamMap};

use crate::backend::{self, Backend};

// pub mod daemon;
mod uinput;

struct EvdevEvent {
    inner_event: InputEvent,
}

impl EvdevEvent {
    fn new(inner_event: InputEvent) -> Self {
        Self { inner_event }
    }
}

impl backend::Event for EvdevEvent {
    fn kind(&self) -> evdev::InputEventKind {
        self.inner_event.kind()
    }
    fn value(&self) -> i32 {
        self.inner_event.value()
    }
    fn code(&self) -> u16 {
        self.inner_event.code()
    }

    fn to_inputevent(&self) -> InputEvent {
        InputEvent::new(self.inner_event.event_type(), self.code(), self.value())
    }
}

pub struct EvdevBackend {
    arg_devices: Vec<String>,
    uinput_device: Option<VirtualDevice>,
    uinput_switches_device: Option<VirtualDevice>,
    keyboard_stream_map: StreamMap<String, EventStream>,
}

impl EvdevBackend {
    pub fn new() -> Self {
        Self {
            arg_devices: vec![],
            uinput_device: None,
            uinput_switches_device: None,
            keyboard_stream_map: StreamMap::new(),
        }
    }

    fn check_device_is_keyboard(device: &Device) -> bool {
        if device.supported_keys().is_some_and(|keys| keys.contains(Key::KEY_ENTER)) {
            if device.name() == Some("swhkd virtual output") {
                return false;
            }
            log::debug!("Keyboard: {}", device.name().unwrap(),);
            true
        } else {
            log::trace!("Other: {}", device.name().unwrap(),);
            false
        }
    }
}

impl Drop for EvdevBackend {
    fn drop(&mut self) {
        for path in self.keyboard_stream_map.keys() {
            let _ = Device::open(path).map(|mut d| d.ungrab());
        }
    }
}

impl Backend for EvdevBackend {
    fn get_initial_devices(
        &mut self,
        arg_devices: &[String],
    ) -> Result<Vec<String>, Box<dyn Error>> {
        // replace arg_devices
        self.arg_devices.clear();
        self.arg_devices.extend_from_slice(arg_devices);
        let devices: Vec<_> = if self.arg_devices.is_empty() {
            log::trace!("Attempting to find all keyboard file descriptors.");
            evdev::enumerate()
                .filter(|(_, dev)| EvdevBackend::check_device_is_keyboard(&dev))
                .collect()
        } else {
            evdev::enumerate()
                .filter(|(_, dev)| {
                    log::trace!("Checking device: {}", dev.name().unwrap_or_default());
                    self.arg_devices.contains(&dev.name().unwrap_or("").to_string())
                })
                .collect()
        };

        Ok(devices
            .into_iter()
            .filter_map(|(path, mut device)| {
                let _ = device.grab();
                let path_string = match path.to_str() {
                    Some(p) => p.to_string(),
                    None => {
                        return None;
                    }
                };
                self.keyboard_stream_map
                    .insert(path_string.clone(), device.into_event_stream().ok()?);

                Some(path_string)
            })
            .collect())
    }

    fn add_device(&mut self, node: &str) -> bool {
        let mut device = match Device::open(node) {
            Err(e) => {
                log::error!("Could not open evdev device at {}: {}", node, e);
                return false;
            }
            Ok(device) => device,
        };
        let name = device.name().unwrap_or("[unknown]").to_string();
        if self.arg_devices.contains(&name) || EvdevBackend::check_device_is_keyboard(&device) {
            log::info!("Device '{}' at '{}' added.", name, node);
            let _ = device.grab();
            let event_stream = match device.into_event_stream() {
                Ok(event_stream) => event_stream,
                Err(_) => {
                    return false;
                }
            };
            self.keyboard_stream_map.insert(node.to_string(), event_stream);
            true
        } else {
            false
        }
    }

    fn remove_device(&mut self, node: &str) -> bool {
        if self.keyboard_stream_map.contains_key(node) {
            let stream = self.keyboard_stream_map.remove(node).expect("device not in stream_map");
            let name = stream.device().name().unwrap_or("[unknown]");
            log::info!("Device '{}' at '{}' removed", name, node);
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

    async fn next_event(&mut self) -> Option<(String, Box<dyn backend::Event>)> {
        match self.keyboard_stream_map.next().await {
            Some((node, Ok(event))) => Some((node, Box::new(EvdevEvent::new(event)))),
            _ => None,
        }
    }

    fn emit_event(&mut self, event: &Box<dyn backend::Event>) -> Result<(), io::Error> {
        let ev = event.to_inputevent();
        if let Some(dev) = &mut self.uinput_device {
            return dev.emit(&[ev]);
        };
        Ok(())
    }

    fn emit_switch_event(&mut self, event: &Box<dyn backend::Event>) -> Result<(), io::Error> {
        todo!("NOOP")
    }
}
