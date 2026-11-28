//! Text that reaches the screen came from agents, remote seats and Twitch
//! strangers. Drop anything a terminal would act on: C0 controls (ESC, BEL),
//! DEL, C1 (CSI 0x9b and friends), zero-width and bidi marks, BOM.
//! Same set as nav-deck's BAD_CHARS and comms-deck's BEACON_UNSAFE.

pub fn scrub(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|&c| !unsafe_char(c))
        .collect()
}

fn unsafe_char(c: char) -> bool {
    let u = c as u32;
    u < 0x20
        || (0x7f..=0x9f).contains(&u)
        || (0x200b..=0x200f).contains(&u)
        || (0x202a..=0x202e).contains(&u)
        || (0x2060..=0x2069).contains(&u)
        || u == 0xfeff
}

#[cfg(test)]
mod tests {
    use super::scrub;

    #[test]
    fn drops_escapes_and_bidi() {
        let s = scrub("a\x1b]52;c;aGk=\x07b\u{202e}c\u{9b}d\u{200b}e");
        assert_eq!(s, "a]52;c;aGk=bcde");
    }

    #[test]
    fn keeps_plain_text_and_emoji() {
        assert_eq!(scrub("hello ✦ wörld 🚀"), "hello ✦ wörld 🚀");
    }
}
