//! The chat box: the only place the warp core acts. A line typed here goes
//! out as Y through `comms-deck send --from owner`, exactly as the terminal
//! deck's chat box sends it (deck/src/send.rs), so delivery, redaction and
//! wake rules stay the service's. The terminal chat keeps working alongside.
//!
//! Written by: claude

use std::process::Command;

/// Who a chat-box line goes to, as `comms-deck send` takes it: the same rules
/// as comms-deck's `route()` and the deck's send.rs. `@seat` names a seat
/// (several are fine); `@all` or no `@` is everyone; `-seat` drops that seat.
/// Only words that start with @ or - count, so "re-run" and "foo@bar" are
/// left alone.
pub fn route(text: &str, seats: &[String]) -> Result<String, String> {
    let is_seat = |n: &str| seats.iter().any(|s| s == n);
    let mut names: Vec<String> = Vec::new();
    let mut excludes: Vec<String> = Vec::new();
    for word in words_with_prefix(text, '@') {
        if !names.contains(&word) {
            names.push(word);
        }
    }
    for word in words_with_prefix(text, '-') {
        if is_seat(&word) && !excludes.contains(&word) {
            excludes.push(word);
        }
    }
    let drops: Vec<String> = excludes.iter().map(|s| format!("-{s}")).collect();
    if names.is_empty() || names.iter().any(|n| n == "all") {
        let mut to = vec!["all".to_string()];
        to.extend(drops);
        return Ok(to.join(" "));
    }
    if let Some(bad) = names.iter().find(|n| !is_seat(n) || *n == "owner") {
        return Err(format!("no seat called @{bad}"));
    }
    if names.iter().all(|n| excludes.contains(n)) {
        return Err("that leaves nobody to send to".into());
    }
    let mut to = names.join(", ");
    for d in drops {
        to.push(' ');
        to.push_str(&d);
    }
    Ok(to)
}

/// Words after `mark` that start a word (not "foo@bar", not "re-run").
fn words_with_prefix(text: &str, mark: char) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if c != mark {
            continue;
        }
        if i > 0 && (word(chars[i - 1]) || chars[i - 1] == mark) {
            continue;
        }
        let rest = &chars[i + 1..];
        if !rest.first().is_some_and(|c| c.is_ascii_alphabetic()) {
            continue;
        }
        let name: String = rest.iter().take_while(|&&c| word(c) || c == '-').collect();
        out.push(name.to_lowercase());
    }
    out
}

fn program() -> String {
    std::env::var("COMMS_DECK_BIN").unwrap_or_else(|_| "comms-deck".into())
}

/// Send `text` from the owner. Returns the service's answer or its error.
/// Blocking: call it off the render thread.
pub fn send(to: &str, text: &str) -> Result<String, String> {
    let out = Command::new(program())
        .args(["--from", "owner", "send", to, text])
        .output()
        .map_err(|e| format!("couldn't run {}: {e}", program()))?;
    let say = String::from_utf8_lossy(if out.status.success() { &out.stdout } else { &out.stderr })
        .trim()
        .to_string();
    if out.status.success() {
        Ok(say)
    } else {
        Err(say)
    }
}

/// The clipboard's text, for a paste; newlines become spaces so a paste
/// never sends itself.
pub fn paste() -> String {
    let Ok(out) = Command::new("wl-paste").args(["-n", "-t", "text"]).output() else { return String::new() };
    clean_paste(&String::from_utf8_lossy(&out.stdout))
}

pub fn clean_paste(s: &str) -> String {
    let joined: String = s.chars().map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c }).collect();
    crate::feed::scrub(&joined)
}

/// Ctrl+W: the line without its last word (and the spaces before it).
pub fn drop_word(s: &str) -> String {
    let t = s.trim_end();
    match t.rfind(' ') {
        Some(i) => t[..=i].to_string(),
        None => String::new(),
    }
}

/// Adds `@seat ` to the line unless it already names that seat.
pub fn address(line: &str, seat: &str) -> String {
    if words_with_prefix(line, '@').iter().any(|w| w == seat) {
        return line.to_string();
    }
    format!("@{seat} {}", line.trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> Vec<String> {
        ["owner", "claude", "grok", "vibe", "devin"].map(String::from).to_vec()
    }

    #[test]
    fn routes_like_the_deck_chat_box() {
        assert_eq!(route("hello", &seats()).unwrap(), "all");
        assert_eq!(route("@vibe hi", &seats()).unwrap(), "vibe");
        assert_eq!(route("@vibe @Devin look", &seats()).unwrap(), "vibe, devin");
        assert_eq!(route("@all but -grok", &seats()).unwrap(), "all -grok");
        assert_eq!(route("re-run foo@bar", &seats()).unwrap(), "all", "mid-word marks don't count");
        assert!(route("@nobody hi", &seats()).unwrap_err().contains("@nobody"));
        assert!(route("@owner hi", &seats()).is_err(), "Y doesn't send to Y");
        assert!(route("@vibe -vibe", &seats()).is_err());
    }

    #[test]
    fn clicking_a_seat_addresses_it_once() {
        assert_eq!(address("", "vibe"), "@vibe ");
        assert_eq!(address("hi there", "vibe"), "@vibe hi there");
        assert_eq!(address("@vibe hi", "vibe"), "@vibe hi");
        assert_eq!(address("@vibe hi", "devin"), "@devin @vibe hi");
    }

    #[test]
    fn editing_keys() {
        assert_eq!(drop_word("@vibe can you "), "@vibe can ", "one word at a time");
        assert_eq!(drop_word("one"), "");
        assert_eq!(clean_paste("line one\nline two\x1b[31m"), "line one line two[31m");
    }
}
