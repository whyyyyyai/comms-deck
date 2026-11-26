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

/// One wake attempt for a seat: `comms-deck wake`, so its rules decide
/// (never a spent seat, never over a draft).
pub fn wake(seat: &str) -> Result<String, String> {
    if !seat.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("no seat called {seat}"));
    }
    let out = Command::new(program())
        .args(["wake", seat])
        .output()
        .map_err(|e| format!("couldn't run {}: {e}", program()))?;
    let say = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout }).trim().to_string();
    if out.status.success() {
        Ok(if say.is_empty() { format!("woke {seat}") } else { say })
    } else {
        Err(if say.is_empty() { format!("{seat} not woken") } else { say })
    }
}

/// Focus a workspace by deck label or number (nav-deck's dispatch_workspace).
pub fn goto(label: &str) -> Result<String, String> {
    let expr = format!("hl.dsp.focus({{ workspace = {} }})", crate::navdata::goto_target(label));
    Command::new("hyprctl")
        .args(["dispatch", &expr])
        .output()
        .map_err(|e| format!("couldn't run hyprctl: {e}"))?;
    Ok(format!("going to {label}"))
}

/// The program or window class that holds a seat, as scratchpad-layout
/// finds it: terminal agents by their process, app seats by class.
fn seat_holder(seat: &str) -> Option<(&'static str, bool)> {
    Some(match seat {
        "claude" => ("claude", true),
        "grok" => ("grok", true),
        "gravity" => ("agy", true),
        "devin" => ("devin", true),
        "vibe" => ("vibe-rs", true),
        "codex" => ("codex", true),
        "lumo" => ("lumo-desktop", false),
        "alice" => ("grok-bot", false),
        _ => return None,
    })
}

/// {pid: (parent, comm)} for every process.
fn proc_tree() -> std::collections::HashMap<u32, (u32, String)> {
    let mut tree = std::collections::HashMap::new();
    let Ok(dir) = std::fs::read_dir("/proc") else { return tree };
    for d in dir.flatten() {
        let Ok(pid) = d.file_name().to_string_lossy().parse::<u32>() else { continue };
        let (Ok(stat), Ok(comm)) = (std::fs::read_to_string(d.path().join("stat")), std::fs::read_to_string(d.path().join("comm"))) else { continue };
        let Some(after) = stat.rsplit_once(')') else { continue };
        let Some(ppid) = after.1.split_whitespace().nth(1).and_then(|p| p.parse().ok()) else { continue };
        tree.insert(pid, (ppid, comm.trim().to_string()));
    }
    tree
}

/// Show a seat's own window (it may be on the scratchpad): Hyprland focus
/// only, nothing is typed and nobody is woken.
pub fn focus_seat(seat: &str) -> Result<String, String> {
    let (holder, by_process) = seat_holder(seat).ok_or_else(|| format!("{seat} has no window here"))?;
    let out = Command::new("hyprctl").args(["clients", "-j"]).output().map_err(|e| format!("couldn't run hyprctl: {e}"))?;
    let clients: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).map_err(|_| "hyprctl clients unreadable".to_string())?;
    let class = |c: &serde_json::Value| c["class"].as_str().unwrap_or("").to_string();
    let address = if by_process {
        let kitties: std::collections::HashMap<u32, String> = clients
            .iter()
            .filter(|c| class(c) == "kitty")
            .filter_map(|c| Some((c["pid"].as_u64()? as u32, c["address"].as_str()?.to_string())))
            .collect();
        let tree = proc_tree();
        tree.iter().filter(|(_, (_, name))| name == holder).find_map(|(&pid, _)| {
            let mut p = pid;
            while p > 1 {
                if let Some(a) = kitties.get(&p) {
                    return Some(a.clone());
                }
                p = tree.get(&p).map_or(0, |t| t.0);
            }
            None
        })
    } else {
        clients.iter().find(|c| class(c) == holder).and_then(|c| c["address"].as_str().map(String::from))
    };
    let address = address.ok_or_else(|| format!("{seat}'s window isn't open"))?;
    if !address.chars().all(|c| c.is_ascii_hexdigit() || c == 'x') {
        return Err("odd window address".into());
    }
    let expr = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    Command::new("hyprctl").args(["dispatch", &expr]).output().map_err(|e| format!("couldn't run hyprctl: {e}"))?;
    Ok(format!("showing {seat}'s window"))
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
