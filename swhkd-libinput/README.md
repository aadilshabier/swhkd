# swhkd-libinput

## Instructions

1. Open a terminal in this directory (`swhkd/swhkd-libinput`)

2. Create a file called `devices.txt` with the list of devices to grab read events from.  
**WARNING**: This will block all events from reaching your window manager, make sure you can exit the process without the listed devices.  
Example file:
    ```
    Sino Wealth USB Keyboard
    Sino Wealth USB Keyboard Consumer Control
    ```

3. Compile the project(optionally with the `--release` flag)
    ```sh
    $ cargo build
    ```

4. Run the project, use the `RUST_LOG` flag to select your logging level.
    ```sh
    $ sudo RUST_LOG=DEBUG ../target/debug/swhkd_libinput
    ```