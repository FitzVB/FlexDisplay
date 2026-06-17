use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(windows)]
pub fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_cmd: &mut Command) {}

#[cfg(windows)]
pub fn hide_tokio_command(cmd: &mut tokio::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_tokio_command(_cmd: &mut tokio::process::Command) {}

pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program.as_ref());
    hide_console(&mut cmd);
    cmd
}
