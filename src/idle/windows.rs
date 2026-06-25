use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

pub fn idle_seconds() -> u64 {
    unsafe {
        let mut lii: LASTINPUTINFO = std::mem::zeroed();
        lii.cbSize = std::mem::size_of::<LASTINPUTINFO>() as u32;
        if GetLastInputInfo(&mut lii) == 0 {
            return 0;
        }
        let now = windows_sys::Win32::System::SystemInformation::GetTickCount();
        if now < lii.dwTime {
            return 0;
        }
        ((now - lii.dwTime) / 1000) as u64
    }
}
