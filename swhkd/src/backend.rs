use std::{
    error::Error,
    io::{self},
};

use evdev::{InputEvent, InputEventKind};

pub trait Event {
    fn kind(&self) -> InputEventKind;
    fn value(&self) -> i32;
    fn code(&self) -> u16;
    fn to_inputevent(&self) -> InputEvent;
}

pub trait Backend {
    fn get_initial_devices(
        &mut self,
        arg_devices: &[String],
    ) -> Result<Vec<String>, Box<dyn Error>>;
    fn add_device(&mut self, device: &str) -> bool;
    fn remove_device(&mut self, device: &str) -> bool;
    fn create_uinput_devices(&mut self) -> Result<(), Box<dyn Error>>;
    async fn next_event(&mut self) -> Option<(String, InputEvent)>;
    fn emit_event(&mut self, event: &InputEvent) -> Result<(), io::Error>;
    fn emit_switch_event(&mut self, event: &InputEvent) -> Result<(), io::Error>;
}
