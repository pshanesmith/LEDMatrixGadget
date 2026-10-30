//! Small helpers shared by the data feeds and the run loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// A flag threads can wait on with a timeout (Python's `threading.Event`).
#[derive(Default)]
pub struct Signal {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl Signal {
    pub fn new() -> Self {
        Signal::default()
    }

    pub fn set(&self) {
        *self.flag.lock().unwrap() = true;
        self.cv.notify_all();
    }

    pub fn clear(&self) {
        *self.flag.lock().unwrap() = false;
    }

    pub fn is_set(&self) -> bool {
        *self.flag.lock().unwrap()
    }

    /// Wait up to `timeout` for the flag; true if it is set.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut flag = self.flag.lock().unwrap();
        while !*flag {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            flag = self.cv.wait_timeout(flag, left).unwrap().0;
        }
        *flag
    }
}

/// Wait for a thread to end, but give up after `timeout` (the thread is left to finish by itself).
pub fn join_within(handle: JoinHandle<()>, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

/// Lock a mutex even if a thread panicked while holding it: the data feeds only keep plain values
/// behind their locks, and a panic is already logged where it happened.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run one step of a background loop, turning a panic into a logged `None` so the thread lives on.
pub fn run_guarded<T>(what: &str, step: impl FnOnce() -> T) -> Option<T> {
    match catch_unwind(AssertUnwindSafe(step)) {
        Ok(v) => Some(v),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            log::error!("{what} panicked: {msg}; retrying after a back-off");
            None
        }
    }
}

/// Process-wide flag set from signal handlers.
pub type Flag = std::sync::Arc<AtomicBool>;

pub fn flag_is_set(flag: &Flag) -> bool {
    flag.load(Ordering::Relaxed)
}

/// Python's `fnmatch.fnmatchcase`: `*`, `?`, `[abc]`, `[a-z]` and `[!abc]`, case-sensitive.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let pat: Vec<char> = pattern.chars().collect();
    matches_from(&name, &pat)
}

fn matches_from(name: &[char], pat: &[char]) -> bool {
    let (mut n, mut p) = (0usize, 0usize);
    let (mut star_p, mut star_n) = (None::<usize>, 0usize);
    loop {
        if p < pat.len() {
            match pat[p] {
                '*' => {
                    star_p = Some(p);
                    star_n = n;
                    p += 1;
                    continue;
                }
                '?' if n < name.len() => {
                    n += 1;
                    p += 1;
                    continue;
                }
                '[' => {
                    if let Some((ok, next)) = class_match(pat, p, name.get(n).copied()) {
                        if ok && n < name.len() {
                            n += 1;
                            p = next;
                            continue;
                        }
                    } else if n < name.len() && name[n] == '[' {
                        // no closing bracket: a literal '['
                        n += 1;
                        p += 1;
                        continue;
                    }
                }
                c if n < name.len() && name[n] == c && c != '?' => {
                    n += 1;
                    p += 1;
                    continue;
                }
                _ => {}
            }
        } else if n == name.len() {
            return true;
        }
        match star_p {
            Some(sp) if star_n < name.len() => {
                star_n += 1;
                n = star_n;
                p = sp + 1;
            }
            _ => return false,
        }
    }
}

/// Match `ch` against the class starting at `pat[start] == '['`. Returns (matched, index after ']'),
/// or None when the class is not closed.
fn class_match(pat: &[char], start: usize, ch: Option<char>) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = pat.get(i) == Some(&'!');
    if negate {
        i += 1;
    }
    let first = i;
    let mut matched = false;
    loop {
        let c = *pat.get(i)?;
        if c == ']' && i > first {
            break;
        }
        if pat.get(i + 1) == Some(&'-') && pat.get(i + 2).is_some_and(|e| *e != ']') {
            let end = pat[i + 2];
            if let Some(ch) = ch {
                if c <= ch && ch <= end {
                    matched = true;
                }
            }
            i += 3;
        } else {
            if ch == Some(c) {
                matched = true;
            }
            i += 1;
        }
    }
    Some((matched != negate, i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(fnmatch("light.kitchen_rgb_indicator", "light.*_rgb_indicator"));
        assert!(fnmatch("light.x_ap_led", "light.*_led"));
        assert!(!fnmatch("light.led_strip", "light.*_led"));
        assert!(!fnmatch("light.desk_led_2", "light.*_led"));
        assert!(fnmatch("lock.guest_a", "lock.guest_*"));
        assert!(fnmatch("a", "?"));
        assert!(!fnmatch("ab", "?"));
        assert!(fnmatch("lock.a1", "lock.a[0-9]"));
        assert!(!fnmatch("lock.ab", "lock.a[0-9]"));
        assert!(fnmatch("lock.ab", "lock.a[!0-9]"));
        assert!(!fnmatch("Light.x", "light.*"));
        assert!(fnmatch("", "*"));
        assert!(fnmatch("a[", "a["));
        assert!(fnmatch("abcabc", "*abc"));
    }

    #[test]
    fn guarded_steps_survive_panics_and_poisoned_locks() {
        let m = Mutex::new(1);
        assert_eq!(run_guarded("test", || {
            let _g = m.lock().unwrap();
            panic!("boom")
        }), None::<()>);
        assert!(m.is_poisoned());
        *lock(&m) += 1;
        assert_eq!(*lock(&m), 2);
        assert_eq!(run_guarded("test", || 5), Some(5));
    }

    #[test]
    fn signal_times_out_and_wakes() {
        let s = Signal::new();
        assert!(!s.wait(Duration::from_millis(20)));
        s.set();
        assert!(s.wait(Duration::from_millis(20)));
        s.clear();
        assert!(!s.is_set());
    }
}
