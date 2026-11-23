//! Who made a decision: the person signed in to this Mac (DESIGN_SPEC.md §14). The core reads it, so no screen can
//! choose the name that goes into the journal. Nitish chose this on November 22, 2026.

/// The name to record from the account's own fields: its full name when it has one, else its login. A name made only
/// of control characters or spaces counts as none.
pub fn name_from(full: &str, login: &str) -> String {
    let clean = |s: &str| -> String {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        s.chars().take(120).collect()
    };
    // The account's full name is the first comma-separated field of its description.
    let full = clean(full.split(',').next().unwrap_or_default());
    if !full.is_empty() {
        return full;
    }
    let login = clean(login);
    if !login.is_empty() {
        return login;
    }
    "the person signed in to this Mac".into()
}

/// The person signed in to this Mac now, read from the system's account record.
pub fn current() -> String {
    let (full, login) = account().unwrap_or_default();
    name_from(&full, &login)
}

fn account() -> Option<(String, String)> {
    // SAFETY: getpwuid_r writes into the buffer and the record we own; both outlive every read of them.
    unsafe {
        let mut record: libc::passwd = std::mem::zeroed();
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0 as libc::c_char; 4096];
        let rc = libc::getpwuid_r(
            libc::getuid(),
            &mut record,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        );
        if rc != 0 || found.is_null() {
            return None;
        }
        let text = |p: *const libc::c_char| {
            if p.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        Some((text(record.pw_gecos), text(record.pw_name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_full_name_is_recorded_when_the_account_has_one() {
        assert_eq!(name_from("Ana Ruiz,,,", "ana"), "Ana Ruiz");
        assert_eq!(name_from("  Ana   Ruiz ", "ana"), "Ana Ruiz");
    }

    #[test]
    fn the_login_is_recorded_when_there_is_no_full_name() {
        assert_eq!(name_from("", "ana"), "ana");
        assert_eq!(name_from(",,,", "ana"), "ana");
        assert_eq!(name_from("\u{7}\u{0}", " ana "), "ana");
    }

    #[test]
    fn with_neither_it_says_so_in_words() {
        assert_eq!(name_from("", ""), "the person signed in to this Mac");
    }

    #[test]
    fn control_characters_never_reach_the_journal_and_the_name_is_bounded() {
        assert_eq!(name_from("Ana\nRuiz\u{1b}[31m", "x"), "AnaRuiz[31m");
        assert_eq!(name_from(&"a".repeat(500), "x").chars().count(), 120);
    }

    #[test]
    fn this_mac_has_an_account_to_name() {
        let n = current();
        assert!(!n.is_empty());
        assert!(!n.chars().any(char::is_control));
    }
}
