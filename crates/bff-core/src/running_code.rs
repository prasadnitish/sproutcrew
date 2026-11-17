//! Whether this Mac allows projects that write running code: app building, and a clickable prototype added to a
//! plan. It is off until the person turns it on in Settings, and turning it off again stops new projects and new
//! prototype steps from being made. Code that is written is still shown for review before it is used, and a
//! prototype opens only in the app's own preview, which loads nothing from the internet.

use std::path::Path;

const FILE: &str = "running-code.json";

/// True when the person has allowed projects that write running code.
pub fn allowed(data: &Path) -> bool {
    std::fs::read_to_string(data.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["allowed"].as_bool())
        .unwrap_or(false)
}

/// Saves the person's choice.
pub fn set(data: &Path, on: bool) -> Result<(), String> {
    let path = data.join(FILE);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::json!({ "allowed": on }).to_string())
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// The sentence shown when a project tries to write running code before it is allowed.
pub const NOT_ALLOWED: &str = "Projects that write running code are off. Turn on \"Allow projects that write running code\" in Settings to build an app or add a clickable prototype.";

#[cfg(test)]
mod tests {
    #[test]
    fn running_code_is_off_until_the_person_turns_it_on() {
        let d = std::env::temp_dir().join(format!("bff-running-code-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert!(!super::allowed(&d));
        super::set(&d, true).unwrap();
        assert!(super::allowed(&d));
        super::set(&d, false).unwrap();
        assert!(!super::allowed(&d));
        std::fs::write(d.join(super::FILE), "not json").unwrap();
        assert!(!super::allowed(&d), "an unreadable file means off");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
