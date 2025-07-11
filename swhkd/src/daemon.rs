use std::error::Error;

mod config;
mod environ;
mod perms;

#[cfg(all(feature = "evdev_backend", feature = "libinput_backend"))]
compile_error!("Only one input backend can be enabled at a time");

#[cfg(not(any(feature = "evdev_backend", feature = "libinput_backend")))]
compile_error!(
    "You must enable exactly one input backend feature(evdev_backend or libinput_backend)"
);

#[cfg(feature = "evdev_backend")]
mod evdev_backend;

#[cfg(feature = "libinput_backend")]
mod libinput_backend;

#[cfg(feature = "evdev_backend")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    evdev_backend::daemon::main().await
}

#[cfg(feature = "libinput_backend")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    libinput_backend::daemon::main().await
}
