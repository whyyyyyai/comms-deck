//! The machine's own numbers for the stats panel on the left of the scene
//! (t53, Y: "system stats, like gpu, memory, ram, context"). Read from
//! /proc and the SMC's hwmon sensors every couple of seconds; each read is
//! a few small files, never a program or the journal.
//!
//! The M1's GPU (Asahi) reports no busy figure and shares the machine's
//! RAM, so the GPU line is how the GPU is treating us: frame rate, the
//! worst frame and the stalls (frames over a quarter second). Stalls are
//! what came before every freeze on 27 Nov, so they are the one to watch.

use std::collections::VecDeque;

/// A frame longer than this is a stall (the same mark as the journal's
/// `warp-core hitch` lines).
pub const STALL_S: f32 = 0.25;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Memory {
    pub total_kb: u64,
    pub available_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
    /// Shared memory: on Asahi the GPU's buffers live here, with tmpfs.
    pub shmem_kb: u64,
}

impl Memory {
    pub fn used_kb(&self) -> u64 {
        self.total_kb.saturating_sub(self.available_kb)
    }
    pub fn swap_used_kb(&self) -> u64 {
        self.swap_total_kb.saturating_sub(self.swap_free_kb)
    }
    /// Share of RAM in use, 0..1.
    pub fn used_share(&self) -> f64 {
        if self.total_kb == 0 { 0.0 } else { self.used_kb() as f64 / self.total_kb as f64 }
    }
    pub fn swap_share(&self) -> f64 {
        if self.swap_total_kb == 0 { 0.0 } else { self.swap_used_kb() as f64 / self.swap_total_kb as f64 }
    }
}

/// /proc/meminfo's lines (`Key:   1234 kB`) into the figures we show.
pub fn parse_meminfo(text: &str) -> Memory {
    let mut m = Memory::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(key), Some(val)) = (it.next(), it.next()) else { continue };
        let Ok(v) = val.parse::<u64>() else { continue };
        match key {
            "MemTotal:" => m.total_kb = v,
            "MemAvailable:" => m.available_kb = v,
            "SwapTotal:" => m.swap_total_kb = v,
            "SwapFree:" => m.swap_free_kb = v,
            "Shmem:" => m.shmem_kb = v,
            _ => {}
        }
    }
    m
}

/// The one-minute load average from /proc/loadavg.
pub fn parse_loadavg(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

/// A process's resident memory (kB) from /proc/<pid>/status.
pub fn parse_rss_kb(status: &str) -> Option<u64> {
    status.lines().find_map(|l| l.strip_prefix("VmRSS:")).and_then(|v| v.split_whitespace().next()?.parse().ok())
}

/// The SMC's "Total System Power" in watts, from a hwmon directory's
/// `power*_label` / `power*_input` (microwatts) pairs.
pub fn system_power_w(pairs: &[(String, String)]) -> Option<f64> {
    pairs
        .iter()
        .find(|(label, _)| label.trim() == "Total System Power")
        .and_then(|(_, input)| input.trim().parse::<f64>().ok())
        .map(|uw| uw / 1_000_000.0)
}

/// Read the SMC power sensor from /sys/class/hwmon, if there is one.
fn read_power_w() -> Option<f64> {
    for hw in std::fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let dir = hw.path();
        let mut pairs = Vec::new();
        for i in 1..=4 {
            let (Ok(l), Ok(v)) = (
                std::fs::read_to_string(dir.join(format!("power{i}_label"))),
                std::fs::read_to_string(dir.join(format!("power{i}_input"))),
            ) else {
                continue;
            };
            pairs.push((l, v));
        }
        if let Some(w) = system_power_w(&pairs) {
            return Some(w);
        }
    }
    None
}

/// One reading of the machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub mem: Memory,
    pub load1: Option<f64>,
    pub power_w: Option<f64>,
    /// The warp core's own resident memory.
    pub self_rss_kb: Option<u64>,
}

pub fn read_machine() -> Machine {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    Machine {
        mem: parse_meminfo(&read("/proc/meminfo")),
        load1: parse_loadavg(&read("/proc/loadavg")),
        power_w: read_power_w(),
        self_rss_kb: parse_rss_kb(&read("/proc/self/status")),
    }
}

/// The frame record behind the GPU line: frames in the last second, and
/// every stall in the last minute.
#[derive(Debug, Default)]
pub struct Frames {
    count: u32,
    window: f32,
    pub fps: f32,
    /// (when, how long) of each frame over a minute old drops off.
    recent: VecDeque<(f32, f32)>,
}

impl Frames {
    /// Record one frame of `dt` seconds ending at `now` (real seconds).
    pub fn frame(&mut self, now: f32, dt: f32) {
        self.count += 1;
        self.window += dt;
        if self.window >= 1.0 {
            self.fps = self.count as f32 / self.window;
            self.count = 0;
            self.window = 0.0;
        }
        self.recent.push_back((now, dt));
        while self.recent.front().is_some_and(|(t, _)| now - t > 60.0) {
            self.recent.pop_front();
        }
    }
    /// The longest frame in the last minute, in seconds.
    pub fn worst(&self) -> f32 {
        self.recent.iter().map(|(_, d)| *d).fold(0.0, f32::max)
    }
    /// Stalls in the last minute.
    pub fn stalls(&self) -> usize {
        self.recent.iter().filter(|(_, d)| *d > STALL_S).count()
    }
}

fn gb(kb: u64) -> f64 {
    kb as f64 / 1024.0 / 1024.0
}

/// When a figure is past where trouble starts: stalls at all, RAM over
/// 90% or swap over 75% (the 26 Nov crash was memory; 27 Nov's were stalls).
pub fn warning(m: &Machine, stalls: usize) -> bool {
    stalls > 0 || m.mem.used_share() > 0.9 || m.mem.swap_share() > 0.75
}

/// The panel's text. `context` is (seat label, context used %) for the
/// seats that report it.
pub fn panel_text(m: &Machine, f: &Frames, context: &[(String, f64)]) -> String {
    let mut s = String::from("SYSTEM\n");
    let stalls = f.stalls();
    s.push_str(&format!(
        "gpu    {:.0} fps  worst {:.0} ms  {} stall{}/min{}\n",
        f.fps,
        f.worst() * 1000.0,
        stalls,
        if stalls == 1 { "" } else { "s" },
        if stalls > 0 { "  !" } else { "" }
    ));
    if m.mem.total_kb > 0 {
        s.push_str(&format!(
            "ram    {:.1} / {:.1} GB  {:.0}%{}\n",
            gb(m.mem.used_kb()),
            gb(m.mem.total_kb),
            m.mem.used_share() * 100.0,
            if m.mem.used_share() > 0.9 { "  !" } else { "" }
        ));
        if m.mem.swap_total_kb > 0 {
            s.push_str(&format!(
                "swap   {:.1} / {:.1} GB{}\n",
                gb(m.mem.swap_used_kb()),
                gb(m.mem.swap_total_kb),
                if m.mem.swap_share() > 0.75 { "  !" } else { "" }
            ));
        } else {
            s.push_str("swap   none  !\n");
        }
        s.push_str(&format!("shared {:.1} GB  (gpu buffers, tmpfs)\n", gb(m.mem.shmem_kb)));
    }
    if let Some(l) = m.load1 {
        s.push_str(&format!("cpu    load {l:.2}\n"));
    }
    if let Some(w) = m.power_w {
        s.push_str(&format!("power  {w:.0} W\n"));
    }
    if let Some(r) = m.self_rss_kb {
        s.push_str(&format!("core   {:.0} MB  (this window)\n", r as f64 / 1024.0));
    }
    if !context.is_empty() {
        s.push_str("\nCONTEXT\n");
        for (name, pct) in context {
            s.push_str(&format!("{name:<7}{pct:>3.0}%{}\n", if *pct >= 90.0 { "  !" } else { "" }));
        }
    }
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "MemTotal:        7549776 kB\nMemFree:          720000 kB\nMemAvailable:    2621440 kB\nShmem:           3355443 kB\nSwapTotal:       4194300 kB\nSwapFree:        1677720 kB\n";

    #[test]
    fn meminfo_reads_the_figures_we_show() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!(m.total_kb, 7549776);
        assert_eq!(m.available_kb, 2621440);
        assert_eq!(m.shmem_kb, 3355443);
        assert_eq!(m.swap_used_kb(), 4194300 - 1677720);
        assert!((m.used_share() - 0.6528).abs() < 0.001);
        assert_eq!(parse_meminfo("garbage\nMemTotal: x kB\n"), Memory::default());
    }

    #[test]
    fn load_rss_and_power_parse() {
        assert_eq!(parse_loadavg("1.53 0.53 0.19 2/900 1234\n"), Some(1.53));
        assert_eq!(parse_loadavg(""), None);
        assert_eq!(parse_rss_kb("Name:\twarp-core\nVmRSS:\t  836312 kB\n"), Some(836312));
        let pairs = vec![
            ("AC Input Power\n".to_string(), "49081302\n".to_string()),
            ("Total System Power\n".to_string(), "42408859\n".to_string()),
        ];
        assert!((system_power_w(&pairs).unwrap() - 42.41).abs() < 0.01);
        assert_eq!(system_power_w(&[]), None);
    }

    #[test]
    fn frames_count_stalls_for_a_minute() {
        let mut f = Frames::default();
        let mut t = 0.0;
        for _ in 0..120 {
            t += 1.0 / 60.0;
            f.frame(t, 1.0 / 60.0);
        }
        assert!((f.fps - 60.0).abs() < 1.0, "fps {}", f.fps);
        assert_eq!(f.stalls(), 0);
        t += 1.0;
        f.frame(t, 1.0); // a swap-chain timeout
        assert_eq!(f.stalls(), 1);
        assert!((f.worst() - 1.0).abs() < 1e-6);
        // A minute of smooth frames later it has aged out.
        for _ in 0..(61 * 60) {
            t += 1.0 / 60.0;
            f.frame(t, 1.0 / 60.0);
        }
        assert_eq!(f.stalls(), 0);
        assert!(f.worst() < 0.02);
    }

    #[test]
    fn the_panel_flags_trouble() {
        let m = Machine { mem: parse_meminfo(MEMINFO), load1: Some(1.5), power_w: Some(42.4), self_rss_kb: Some(836312) };
        let mut f = Frames::default();
        f.frame(1.0, 1.0 / 60.0);
        let ctx = vec![("claude".to_string(), 45.0), ("grok".to_string(), 93.0)];
        let text = panel_text(&m, &f, &ctx);
        assert!(text.starts_with("SYSTEM\ngpu"), "{text}");
        assert!(text.contains("ram    4.7 / 7.2 GB  65%"), "{text}");
        assert!(text.contains("swap   2.4 / 4.0 GB"), "{text}");
        assert!(text.contains("power  42 W"), "{text}");
        assert!(text.contains("CONTEXT\nclaude  45%\ngrok    93%  !"), "{text}");
        assert!(!warning(&m, 0));
        assert!(warning(&m, 1), "a stall is a warning");
        let tight = Machine { mem: Memory { total_kb: 100, available_kb: 5, ..Default::default() }, ..Default::default() };
        assert!(warning(&tight, 0), "RAM over 90% is a warning");
        assert!(panel_text(&tight, &f, &[]).contains("swap   none  !"));
    }
}
