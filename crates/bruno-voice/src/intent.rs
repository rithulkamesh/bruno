//! Rule-based intent detection with Jarvis wake-phrase stripping.

use bruno_core::Intent;

const WAKE_PREFIXES: &[&str] = &["hey bruno", "bruno"];

pub fn detect(text: &str) -> Intent {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();

    if lower.contains("forget my voice")
        || lower.contains("delete my voice")
        || lower.contains("clear my voice")
    {
        return Intent::ForgetVoice;
    }

    if lower.contains("train my voice")
        || lower.contains("learn my voice")
        || lower.contains("enroll my voice")
    {
        return Intent::EnrollVoice;
    }

    let Some(command) = strip_wake(&lower, trimmed) else {
        return Intent::Ignored;
    };

    detect_command(command)
}

fn strip_wake<'a>(lower: &str, original: &'a str) -> Option<&'a str> {
    for prefix in WAKE_PREFIXES {
        if lower.starts_with(prefix) {
            let rest = original[prefix.len()..].trim_start();
            let rest = rest.trim_start_matches(|c: char| c == ',' || c == '!' || c == '.');
            let rest = rest.trim_start();
            if rest.is_empty() {
                return None;
            }
            return Some(rest);
        }
    }
    None
}

fn detect_command(command: &str) -> Intent {
    let command_lower = command.to_lowercase();
    if is_greeting(&command_lower) {
        return Intent::Greeting;
    }
    if command_lower.contains("can you hear")
        || command_lower.contains("can u hear")
        || command_lower.contains("hear me")
        || command_lower.contains("are you there")
        || command_lower.contains("you there")
    {
        return Intent::HearingCheck;
    }

    if command_lower.contains("where was i") || command_lower.contains("what was i doing") {
        return Intent::WhereWasI;
    }

    if command_lower.contains("what's on my calendar")
        || command_lower.contains("whats on my calendar")
        || command_lower.contains("my calendar")
        || command_lower.contains("on my calendar")
        || command_lower == "calendar"
    {
        return Intent::Calendar;
    }

    if command_lower.contains("focus")
        || command_lower.contains("leave me alone")
        || command_lower.contains("don't disturb")
        || command_lower.contains("do not disturb")
    {
        return Intent::EnterFocus;
    }

    if command_lower.contains("take a break") || command_lower.contains("i need a break") {
        return Intent::Break;
    }

    if command_lower.contains("what should i work on") || command_lower.contains("what's next") {
        return Intent::NextTask;
    }

    for keyword in ["research", "look up", "find"] {
        if let Some(idx) = command_lower.find(keyword) {
            let query = command[idx + keyword.len()..]
                .trim()
                .trim_start_matches(':')
                .trim();
            return Intent::Research {
                query: if query.is_empty() {
                    command.to_string()
                } else {
                    query.to_string()
                },
            };
        }
    }

    if command_lower.starts_with("do ")
        || command_lower.starts_with("open ")
        || command_lower.starts_with("remind me")
        || command_lower.starts_with("set a reminder")
        || command_lower.starts_with("turn on ")
        || command_lower.starts_with("turn off ")
    {
        return Intent::Command {
            action: command.to_string(),
        };
    }

    Intent::Converse {
        text: command.to_string(),
    }
}

fn is_greeting(lower: &str) -> bool {
    lower.contains("how are you")
        || lower.contains("how are u")
        || lower.contains("how're you")
        || lower.contains("how r you")
        || lower.starts_with("how are")
        || lower.contains("what's up")
        || lower.contains("whats up")
        || lower == "hello"
        || lower == "hi"
        || lower == "hey"
        || lower.starts_with("good morning")
        || lower.starts_with("good evening")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_wake_phrase() {
        assert!(matches!(detect("hello there"), Intent::Ignored));
        assert!(matches!(
            detect("hey bruno can you hear me"),
            Intent::HearingCheck
        ));
    }

    #[test]
    fn detects_where_was_i() {
        assert!(matches!(
            detect("Bruno, where was I?"),
            Intent::WhereWasI
        ));
    }

    #[test]
    fn detects_research() {
        assert!(matches!(
            detect("hey bruno research Rust async patterns"),
            Intent::Research { .. }
        ));
    }

    #[test]
    fn detects_calendar() {
        assert!(matches!(
            detect("hey bruno what's on my calendar"),
            Intent::Calendar
        ));
    }

    #[test]
    fn detects_command() {
        assert!(matches!(
            detect("bruno do open safari"),
            Intent::Command { .. }
        ));
    }

    #[test]
    fn detects_enroll() {
        assert!(matches!(
            detect("hey bruno train my voice"),
            Intent::EnrollVoice
        ));
    }

    #[test]
    fn defaults_to_converse_with_wake() {
        assert!(matches!(
            detect("hey bruno hello"),
            Intent::Greeting
        ));
    }

    #[test]
    fn detects_how_are_you_as_greeting() {
        assert!(matches!(
            detect("bruno how are u"),
            Intent::Greeting
        ));
    }
}
