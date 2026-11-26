//! The only place the deck acts: it runs `comms-deck send` (as the owner)
//! and `comms-deck wake`. Delivery, redaction and every wake rule stay the
//! service's.

use std::process::Command;

use crate::data::program;

/// Who a chat-box line goes to, as `comms-deck send` takes it: the same rules
/// as comms-deck's `route()`. `@seat` names a seat (several are fine); `@all`
/// or no `@` is everyone; `-seat` drops that seat. Only words that start with
/// @ or - count, so "re-run" and "foo@bar" are left alone.
pub fn route(text: &str, seats: &[String]) -> Result<String, String> {
    let is_seat = |n: &str| seats.iter().any(|s| s == n);
    let mut names: Vec<String> = Vec::new();
    let mut excludes: Vec<String> = Vec::new();
    for word in words_with_prefix(text, '@', &['@']) {
        if !names.contains(&word) {
            names.push(word);
        }
    }
    for word in words_with_prefix(text, '-', &['-']) {
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

/// Words that start with `mark`, not preceded by a word character or any of
/// `not_after` (so emails and "re-run" don't count), lowercased.
fn words_with_prefix(text: &str, mark: char, not_after: &[char]) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    for i in 0..chars.len() {
        if chars[i] != mark {
            continue;
        }
        if i > 0 && (word(chars[i - 1]) || not_after.contains(&chars[i - 1])) {
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

/// Send `text` from the owner. Returns the service's answer or its error.
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

#[cfg(test)]
mod tests {
    use super::route;

    fn seats() -> Vec<String> {
        ["claude", "grok", "vibe", "owner"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn routes_like_the_python_chat_box() {
        let s = seats();
        assert_eq!(route("@grok hi", &s).unwrap(), "grok");
        assert_eq!(route("hi @Claude and @grok", &s).unwrap(), "claude, grok");
        assert_eq!(route("@all go", &s).unwrap(), "all");
        assert_eq!(route("hello", &s).unwrap(), "all");
        assert_eq!(route("@all -grok go", &s).unwrap(), "all -grok");
        assert_eq!(route("re-run it -rf", &s).unwrap(), "all");
        assert_eq!(route("mail me@grok.com", &s).unwrap(), "all");
        assert!(route("@bob hi", &s).is_err());
        assert!(route("@owner hi", &s).is_err());
        assert!(route("@grok -grok", &s).is_err());
    }
}
