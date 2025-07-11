# swhkd

CHANGE this

## Instructions

1. Open a terminal in the directory `swhkd/swhkd`

2. Create a file called `devices.txt` with the list of devices to grab read events from.  
**WARNING**: This will block all events from reaching your window manager, make sure you can exit the process without the devices you list.  
Example file:
    ```
    Sino Wealth USB Keyboard
    Sino Wealth USB Keyboard Consumer Control
    ```

3. Compile the project with the feature `libinput_backend`(optionally with the `--release` flag)
    ```sh
    $ cargo build --features=libinput_backend
    ```

4. Run the project, use the `RUST_LOG` flag to select your logging level.
    ```sh
    $ sudo RUST_LOG=INFO ../target/release/swhkd_libinput
    ```