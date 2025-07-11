use input::event::EventTrait;
use uinput;

pub use uinput::Device;

pub const EV_KEY: i32 = 0x01;

pub fn build_uinput_dev() -> Result<uinput::Device, uinput::Error> {
    use uinput::event;
    let mut builder = uinput::default()?;
    // Keyboard
    builder = builder.name("uinput device")?.event(event::Keyboard::All)?;
    builder = builder.name("uinput device")?.event(event::Event::All)?;
    // Mouse buttons
    for event in event::controller::Mouse::iter_variants() {
        builder = builder.event(event)?;
    }
    // Mouse Movement
    for event in event::relative::Position::iter_variants() {
        builder = builder.event(event)?
    }
    // Mouse Wheel
    for event in event::relative::Wheel::iter_variants() {
        builder = builder.event(event)?
    }
    builder.create()
}

pub fn emit_libinput_keyboard_event(
    device: &mut uinput::Device,
    keyboard_event: input::event::KeyboardEvent,
) -> Result<(), uinput::Error> {
    use input::event::keyboard::KeyboardEventTrait;
    let device_name = keyboard_event.device().name().to_string();
    let key_code = keyboard_event.key() as i32;
    let state = keyboard_event.key_state() as i32;
    log::info!("Device: {device_name}, key: {key_code}, state: {state:?}");

    device.write(EV_KEY, key_code, 1 - state)?;
    device.synchronize()?;
    Ok(())
}

pub fn emit_libinput_pointer_event(
    device: &mut uinput::Device,
    pointer_event: input::event::PointerEvent,
) -> Result<(), uinput::Error> {
    use input::event::PointerEvent::*;
    match pointer_event {
        Motion(motion_event) => {
            use uinput::event::{Relative, relative::Position};
            // TODO: check if mouse acceleration is enabled
            let dx = motion_event.dx_unaccelerated();
            let dy = motion_event.dy_unaccelerated();
            log::info!("Mouse: {}, dx: {dx:.2}, dy: {dy:.2}", motion_event.device().name(),);
            device.send(Relative::Position(Position::X), dx as i32)?;
            device.send(Relative::Position(Position::Y), dy as i32)?;
            device.synchronize()?;
        }
        Button(button_event) => {
            let button = button_event.button() as i32;
            let state = button_event.button_state() as i32;
            log::info!(
                "Mouse: {}, button: {button}, state: {state:?}",
                button_event.device().name(),
            );
            device.write(EV_KEY, button, 1 - state)?;
            device.synchronize()?;
        }
        ScrollWheel(scrollwheel_event) => {
            use input::event::pointer::{Axis, PointerScrollEvent};
            use uinput::event::{Relative, relative::Wheel};
            let (mut vert, mut hori) = (0.0, 0.0);
            if scrollwheel_event.has_axis(Axis::Vertical) {
                vert = scrollwheel_event.scroll_value_v120(Axis::Vertical);
            }
            if scrollwheel_event.has_axis(Axis::Horizontal) {
                hori = scrollwheel_event.scroll_value_v120(Axis::Horizontal);
            }
            log::info!(
                "Mouse: {}, wheel vert: {vert}, hori: {hori}",
                scrollwheel_event.device().name(),
            );
            let (event, value) = if vert != 0.0 {
                (Relative::Wheel(Wheel::Vertical), -vert / 120.0)
            } else {
                (Relative::Wheel(Wheel::Horizontal), -hori / 120.0)
            };
            device.send(event, value as i32)?;
            device.synchronize()?;
        }
        _ => {
            log::info!("Mouse: {}, event: {:?}", pointer_event.device().name(), pointer_event);
        }
    }
    Ok(())
}
