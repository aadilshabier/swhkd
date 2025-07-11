use std::{
    collections::HashSet,
    fs::File,
    io::{self, BufRead},
    path::Path,
};

use nix::ioctl_write_int;

ioctl_write_int!(eviocgrab, b'E', 0x90);

pub fn grab(fd: i32) -> io::Result<()> {
    unsafe {
        eviocgrab(fd, 1)?;
    }
    Ok(())
}

pub fn ungrab(fd: i32) -> io::Result<()> {
    unsafe {
        eviocgrab(fd, 0)?;
    }
    Ok(())
}

pub fn name_from_path(path: &Path) -> io::Result<String> {
    let ev = path.strip_prefix("/dev/input").expect("This path should begin with /dev/input");
    let name_path = Path::new("/sys/class/input/").join(ev).join("device/name");
    let name = std::fs::read_to_string(name_path)?.trim_ascii_end().to_string();
    Ok(name)
}

pub fn get_devices_from_file(path: &Path) -> io::Result<HashSet<String>> {
    let file = File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let result = reader.lines().map(|x| x.unwrap().trim_ascii_end().to_string()).collect();
    Ok(result)
}
