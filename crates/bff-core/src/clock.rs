//! Wall-clock seconds, in one place.

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The start of the local day that `at` falls in, in seconds since 1970, from this Mac's time zone.
pub fn local_day_start(at: u64) -> u64 {
    let t = at as libc::time_t;
    // SAFETY: localtime_r and mktime read and write only the values given here.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return at - at % 86_400;
        }
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_isdst = -1;
        let start = libc::mktime(&mut tm);
        if start < 0 {
            at - at % 86_400
        } else {
            start as u64
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_local_day_starts_within_a_day_before_now() {
        let now = super::now();
        let start = super::local_day_start(now);
        assert!(
            start <= now && now - start < 86_400 + 3_600,
            "{start} {now}"
        );
        assert_eq!(super::local_day_start(start + 60), start);
    }
}
