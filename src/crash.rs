//! The crash report: when the game panics, what happened is written to
//! `<folder>-crash.log` beside it (and, on Windows, to its folder in
//! `%APPDATA%`, with a message box, as a release build has no console),
//! before Rust's own report. The game's name and folder are its identity's
//! ([`crate::game_info`]), if it was read before the panic; its build is
//! [`crate::build`].

/// What the report says, and the file it is written to, for a game called
/// `title` keeping its files in `folder`, of the build `build`, that fell
/// with `info`.
pub fn report(title: &str, folder: &str, build: &str, info: &str) -> (String, String) {
    let file = format!("{folder}-crash.log");
    let message = format!(
        "{title} {build} se ha cerrado por un error:\n\n{info}\n\nSe ha guardado un informe en {file}."
    );
    (file, message)
}

/// Sets the panic hook that writes the report, keeping Rust's own after it.
/// Called once, as the game starts.
pub fn install() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // What the game is called, if it was read before it fell.
        let (title, folder) = crate::game_info::read_already()
            .map_or(("The game", "game"), |game| (game.title, game.folder));
        let (file, message) = report(title, folder, crate::build(), &info.to_string());
        let _ = std::fs::write(&file, &message);
        #[cfg(windows)]
        windows::report(title, folder, &message);
        default_hook(info);
    }));
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    /// The report in the game's folder in `%APPDATA%`, and a message box
    /// saying it.
    pub fn report(title: &str, folder: &str, message: &str) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            let path = std::path::PathBuf::from(appdata)
                .join(folder)
                .join("crash.log");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, message);
        }

        let title: Vec<u16> = OsStr::new(&format!("{title} - Error"))
            .encode_wide()
            .chain(Some(0))
            .collect();
        let text: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();

        unsafe {
            type MessageBoxW = unsafe extern "system" fn(
                hwnd: *mut std::ffi::c_void,
                lp_text: *const u16,
                lp_caption: *const u16,
                u_type: u32,
            ) -> i32;

            unsafe extern "system" {
                fn LoadLibraryA(lpLibFileName: *const u8) -> *mut std::ffi::c_void;
                fn GetProcAddress(
                    hModule: *mut std::ffi::c_void,
                    lpProcName: *const u8,
                ) -> *mut std::ffi::c_void;
            }

            let user32 = LoadLibraryA(b"user32.dll\0".as_ptr());
            if !user32.is_null() {
                let proc = GetProcAddress(user32, b"MessageBoxW\0".as_ptr());
                if !proc.is_null() {
                    let msg_box: MessageBoxW = std::mem::transmute(proc);
                    msg_box(
                        std::ptr::null_mut(),
                        text.as_ptr(),
                        title.as_ptr(),
                        0x10, /* MB_ICONERROR */
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_names_the_game_its_build_and_its_file() {
        let (file, message) = report(
            "Motorust Engine",
            "motorust-engine",
            "0.1.0 (engine)",
            "panicked at src/lib.rs:1:1",
        );
        assert_eq!(file, "motorust-engine-crash.log");
        assert!(message.starts_with("Motorust Engine 0.1.0 (engine) se ha cerrado"));
        assert!(message.contains("panicked at src/lib.rs:1:1"));
        assert!(message.ends_with("motorust-engine-crash.log."));
    }
}
