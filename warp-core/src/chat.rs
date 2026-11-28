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
/// `available` is the seats signalling they're free (idle or ready). A note
/// with no @names goes to them alone (Y, 06:11); with nobody free, or with
/// @all or @names, it goes the usual way.
pub fn route(text: &str, seats: &[String], available: &[String]) -> Result<String, String> {
    // The deck validates Proton against its registry even when Warp Core's
    // visible roster omits the hidden seat.
    let is_seat = |n: &str| n == "lumo" || seats.iter().any(|s| s == n);
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
    if names.is_empty() {
        let free: Vec<&String> = available.iter().filter(|s| is_seat(s) && !excludes.contains(s)).collect();
        if !free.is_empty() {
            return Ok(free.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        }
    }
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
        let name = name.to_lowercase();
        out.push(match name.as_str() {
            "luna" | "proton" => "lumo".into(),
            "jupiter" => "claude".into(),
            "mercury" => "vibe".into(),
            "saturn" => "codex".into(),
            "venus" => "devin".into(),
            _ => name,
        });
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

/// One wake attempt for a seat through `comms-deck wake`, so its rules
/// decide (never a spent seat, never over a draft). Blocking.
pub fn wake(seat: &str) -> Result<String, String> {
    if seat.is_empty() || !seat.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(format!("no seat called {seat}"));
    }
    let out = Command::new(program())
        .args(["wake", seat])
        .output()
        .map_err(|e| format!("couldn't run {}: {e}", program()))?;
    let say = String::from_utf8_lossy(if out.status.success() { &out.stdout } else { &out.stderr }).trim().to_string();
    if out.status.success() {
        Ok(if say.is_empty() { format!("woke {seat}") } else { say })
    } else {
        Err(say)
    }
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
        let (Ok(stat), Ok(comm)) =
            (std::fs::read_to_string(d.path().join("stat")), std::fs::read_to_string(d.path().join("comm")))
        else {
            continue;
        };
        let Some(after) = stat.rsplit_once(')') else { continue };
        let Some(ppid) = after.1.split_whitespace().nth(1).and_then(|p| p.parse().ok()) else { continue };
        tree.insert(pid, (ppid, comm.trim().to_string()));
    }
    tree
}

/// Takes Y to a seat's own window, on whatever desktop it's on, and closes
/// the scratchpad (where the warp core lives) so the window is in view.
/// Hyprland focus only: nothing is typed and nobody is woken (the deck's
/// focus_seat). Blocking.
pub fn focus_window(seat: &str) -> Result<String, String> {
    let (holder, by_process) = seat_holder(seat).ok_or_else(|| format!("{seat} has no window here"))?;
    let out = Command::new("hyprctl").args(["clients", "-j"]).output().map_err(|e| format!("couldn't run hyprctl: {e}"))?;
    let clients: Vec<serde_json::Value> =
        serde_json::from_slice(&out.stdout).map_err(|_| "hyprctl clients unreadable".to_string())?;
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
    // Close the scratchpad first if it's showing, so the window isn't behind it.
    let mons = Command::new("hyprctl").args(["monitors", "-j"]).output().ok();
    let special_open = mons
        .and_then(|o| serde_json::from_slice::<Vec<serde_json::Value>>(&o.stdout).ok())
        .is_some_and(|ms| {
            ms.iter().any(|m| m["focused"].as_bool() == Some(true) && m["specialWorkspace"]["name"].as_str() == Some("special:magic"))
        });
    if special_open {
        let _ = Command::new("hyprctl").args(["dispatch", "hl.dsp.workspace.toggle_special(\"magic\")"]).output();
    }
    let expr = format!("hl.dsp.focus({{ window = \"address:{address}\" }})");
    Command::new("hyprctl").args(["dispatch", &expr]).output().map_err(|e| format!("couldn't run hyprctl: {e}"))?;
    Ok(format!("showing {seat}'s window"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> Vec<String> {
        ["owner", "claude", "grok", "vibe", "devin", "codex"].map(String::from).to_vec()
    }

    #[test]
    fn routes_like_the_deck_chat_box() {
        let route = |t: &str, s: &[String]| super::route(t, s, &[]);
        assert_eq!(route("hello", &seats()).unwrap(), "all");
        assert_eq!(route("@vibe hi", &seats()).unwrap(), "vibe");
        assert_eq!(route("@vibe @Devin look", &seats()).unwrap(), "vibe, devin");
        assert_eq!(route("@all but -grok", &seats()).unwrap(), "all -grok");
        assert_eq!(route("re-run foo@bar", &seats()).unwrap(), "all", "mid-word marks don't count");
        assert!(route("@nobody hi", &seats()).unwrap_err().contains("@nobody"));
        assert!(route("@owner hi", &seats()).is_err(), "Y doesn't send to Y");
        assert!(route("@vibe -vibe", &seats()).is_err());
        assert_eq!(route("@Luna @lumo hello", &seats()).unwrap(), "lumo");
        assert_eq!(route("@all -Luna", &seats()).unwrap(), "all -lumo");
        assert_eq!(route("@Proton hello", &[]).unwrap(), "lumo");
        assert_eq!(route("@all -Proton", &seats()).unwrap(), "all -lumo");
        assert_eq!(route("@Jupiter hello", &seats()).unwrap(), "claude");
        assert_eq!(route("@all -Jupiter", &seats()).unwrap(), "all -claude");
        assert_eq!(route("@Mercury hello", &seats()).unwrap(), "vibe");
        assert_eq!(route("@all -Mercury", &seats()).unwrap(), "all -vibe");
        assert_eq!(route("@Saturn hello", &seats()).unwrap(), "codex");
        assert_eq!(route("@all -Saturn", &seats()).unwrap(), "all -codex");
        assert_eq!(route("@VENUS hello", &seats()).unwrap(), "devin");
        assert_eq!(route("@all -Venus", &seats()).unwrap(), "all -devin");
    }

    #[test]
    fn a_plain_note_goes_to_the_free_agents_first() {
        let free = vec!["vibe".to_string(), "devin".to_string()];
        assert_eq!(route("hello", &seats(), &free).unwrap(), "vibe, devin", "free ones only");
        assert_eq!(route("hello -devin", &seats(), &free).unwrap(), "vibe");
        assert_eq!(route("hello -vibe -devin", &seats(), &free).unwrap(), "all -vibe -devin", "none left free: as usual");
        assert_eq!(route("@all hello", &seats(), &free).unwrap(), "all", "@all still means all");
        assert_eq!(route("@grok hi", &seats(), &free).unwrap(), "grok", "a name still wins");
        assert_eq!(route("hello", &seats(), &[]).unwrap(), "all", "everyone busy: as usual");
    }

    #[test]
    fn every_agent_seat_has_a_window_to_find() {
        for seat in ["claude", "grok", "gravity", "devin", "vibe", "lumo", "alice"] {
            assert!(seat_holder(seat).is_some(), "{seat}");
        }
        assert!(seat_holder("owner").is_none(), "Y has no agent window");
        assert!(focus_window("nobody").is_err());
    }

    #[test]
    fn editing_keys() {
        assert_eq!(drop_word("@vibe can you "), "@vibe can ", "one word at a time");
        assert_eq!(drop_word("one"), "");
        assert_eq!(clean_paste("line one\nline two\x1b[31m"), "line one line two[31m");
    }
}
