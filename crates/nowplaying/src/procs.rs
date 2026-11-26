//! Which programs are running, to pick the DJ software automatically.

/// The executable names of the running processes (Windows: "rekordbox.exe"; macOS: the
/// process name, such as "rekordbox" or "Serato DJ Pro").
pub fn running() -> Vec<String> {
    imp::running()
}

/// Whether `wanted` runs: an executable named exactly so (without ".exe"), or containing it,
/// case-insensitively.
pub fn is_running(wanted: &str) -> bool {
    find(&running(), wanted).is_some()
}

/// Whether a known program runs: its executable is named `program`, or `program` followed by
/// a word ("Serato DJ" is "Serato DJ Pro.exe"; "rekordbox" is not "rekordboxAgent.exe").
pub fn program_running(names: &[String], program: &str) -> bool {
    let needle = program.trim().to_lowercase();
    names.iter().any(|n| {
        let stem = n.to_lowercase();
        let stem = stem.trim_end_matches(".exe");
        stem == needle || stem.starts_with(&format!("{needle} "))
    })
}

/// The name in `names` that is `wanted` (without ".exe"), else the first that contains it.
pub fn find<'a>(names: &'a [String], wanted: &str) -> Option<&'a str> {
    let needle = wanted.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let stem = |n: &str| n.to_lowercase().trim_end_matches(".exe").to_string();
    names
        .iter()
        .find(|n| stem(n) == needle)
        .or_else(|| names.iter().find(|n| n.to_lowercase().contains(&needle)))
        .map(String::as_str)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    pub fn running() -> Vec<String> {
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
            let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            let mut ok = Process32FirstW(snap, &mut entry).is_ok();
            while ok {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                out.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
                ok = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn running() -> Vec<String> {
        let mut pids = vec![0 as libc::pid_t; 4096];
        let bytes = (pids.len() * size_of::<libc::pid_t>()) as libc::c_int;
        let n = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if n <= 0 {
            return Vec::new();
        }
        pids.truncate(n as usize);
        pids.into_iter()
            .filter_map(|pid| {
                let mut buf = [0u8; 256];
                let len = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
                (len > 0).then(|| String::from_utf8_lossy(&buf[..len as usize]).into_owned())
            })
            .collect()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn running() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_name_wins_over_helpers() {
        let names: Vec<String> = ["rekordboxAgent.exe", "rekordbox.exe", "Serato DJ Pro.exe"].map(String::from).into();
        assert_eq!(find(&names, "rekordbox"), Some("rekordbox.exe"));
        assert_eq!(find(&names, "Serato DJ"), Some("Serato DJ Pro.exe"));
        assert_eq!(find(&names, "traktor"), None);
        assert_eq!(find(&names, ""), None);
    }

    #[test]
    fn known_programs_ignore_helpers() {
        let agent_only: Vec<String> = ["rekordboxAgent.exe", "Serato DJ Pro.exe"].map(String::from).into();
        assert!(!program_running(&agent_only, "rekordbox"));
        assert!(program_running(&agent_only, "Serato DJ"));
        let mac: Vec<String> = ["rekordbox", "Traktor Pro 3"].map(String::from).into();
        assert!(program_running(&mac, "rekordbox"));
        assert!(program_running(&mac, "Traktor"));
    }

    #[test]
    fn this_process_is_listed() {
        assert!(!running().is_empty());
    }
}
