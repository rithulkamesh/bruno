//! macOS Calendar access for voice intents.

#[cfg(target_os = "macos")]
const CALENDAR_SCRIPT: &str = r#"
tell application "Calendar"
    set startOfDay to current date
    set time of startOfDay to 0
    set endOfDay to startOfDay + (1 * days)
    set found to {}
    repeat with c in calendars
        set evts to (every event of c whose start date ≥ startOfDay and start date < endOfDay)
        repeat with e in evts
            set end of found to (time string of (start date of e)) & ": " & (summary of e)
        end repeat
    end repeat
    if (count of found) is 0 then
        return "Nothing on your calendar for the rest of today."
    end if
    set AppleScript's text item delimiters to "; "
    set out to found as string
    set AppleScript's text item delimiters to ""
    return "Today: " & out
end tell
"#;

pub fn today_summary() -> String {
    #[cfg(target_os = "macos")]
    {
        match std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(CALENDAR_SCRIPT)
            .output()
        {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if text.is_empty() {
                    "Nothing on your calendar for the rest of today.".into()
                } else {
                    text
                }
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr);
                tracing::warn!(stderr = %err, "calendar script failed");
                if err.contains("Not authorized") || err.contains("-1743") {
                    "Allow Bruno to control Calendar in System Settings → Privacy → Automation."
                        .into()
                } else {
                    "Couldn't read your calendar.".into()
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "calendar script spawn failed");
                "Couldn't read your calendar.".into()
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Calendar is only available on macOS.".into()
    }
}
