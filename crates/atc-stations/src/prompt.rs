//! Line-based console prompts: numbered menus, questions, yes/no and multi-select lists.
//!
//! Plain text only (no ANSI codes), so it works in a bare Windows console. End of input makes
//! every prompt return its "cancel" answer; see [`Prompt::eof`].

use std::io::{BufRead, Write};

pub struct Prompt<R: BufRead, W: Write> {
    input: R,
    output: W,
    eof: bool,
    /// First write error; the session is treated as ended once there is one.
    write_error: Option<std::io::Error>,
}

impl<R: BufRead, W: Write> Prompt<R, W> {
    pub fn new(input: R, output: W) -> Self {
        Self {
            input,
            output,
            eof: false,
            write_error: None,
        }
    }

    /// The output, e.g. a `Vec<u8>` captured in tests.
    pub fn into_output(self) -> W {
        self.output
    }

    /// True once the input has ended (or the output failed): every prompt then cancels.
    pub fn eof(&self) -> bool {
        self.eof || self.write_error.is_some()
    }

    /// Takes the first output error, if any.
    pub fn take_write_error(&mut self) -> Option<std::io::Error> {
        self.write_error.take()
    }

    /// Prints one line.
    pub fn say(&mut self, line: impl AsRef<str>) {
        self.put(&format!("{}\n", line.as_ref()));
    }

    fn put(&mut self, text: &str) {
        if self.write_error.is_some() {
            return;
        }
        if let Err(e) = self
            .output
            .write_all(text.as_bytes())
            .and_then(|_| self.output.flush())
        {
            self.write_error = Some(e);
        }
    }

    /// One trimmed input line; `None` at end of input.
    fn line(&mut self) -> Option<String> {
        if self.eof() {
            return None;
        }
        let mut s = String::new();
        match self.input.read_line(&mut s) {
            Ok(0) => {
                self.eof = true;
                self.put("\n");
                None
            }
            Err(_) => {
                self.put("\nCould not read input; quitting.\n");
                self.eof = true;
                None
            }
            Ok(_) => Some(s.trim().to_string()),
        }
    }

    /// Numbered menu; returns the chosen number (1-based), or 0 at end of input.
    pub fn menu(&mut self, title: &str, items: &[&str]) -> usize {
        self.say(title);
        for (i, item) in items.iter().enumerate() {
            self.say(format!("  {} {item}", i + 1));
        }
        loop {
            self.put("> ");
            let Some(answer) = self.line() else {
                return 0;
            };
            match answer.parse::<usize>() {
                Ok(n) if (1..=items.len()).contains(&n) => return n,
                _ => self.say(format!("Please enter a number from 1 to {}.", items.len())),
            }
        }
    }

    /// Free-text question; Enter gives `default` (or ""), end of input gives "".
    pub fn ask(&mut self, question: &str, default: Option<&str>) -> String {
        match default {
            Some(d) => self.put(&format!("{question} [{d}]: ")),
            None => self.put(&format!("{question}: ")),
        }
        match self.line() {
            None => String::new(),
            Some(a) if a.is_empty() => default.unwrap_or_default().to_string(),
            Some(a) => a,
        }
    }

    /// Yes/no; Enter gives `default`, end of input gives `false`.
    pub fn confirm(&mut self, question: &str, default: bool) -> bool {
        let hint = if default { "[Y/n]" } else { "[y/N]" };
        loop {
            self.put(&format!("{question} {hint} "));
            let Some(a) = self.line() else {
                return false;
            };
            match a.to_lowercase().as_str() {
                "" => return default,
                "y" | "yes" => return true,
                "n" | "no" => return false,
                _ => self.say("Please answer y or n."),
            }
        }
    }

    /// Multi-select over `items` (0-based indices in and out). Input: numbers like `1 3 5`,
    /// `all`, `none`; Enter keeps `preselected`. End of input returns `preselected`; callers
    /// check [`Prompt::eof`] to cancel.
    pub fn pick_many(
        &mut self,
        title: &str,
        items: &[String],
        preselected: &[usize],
    ) -> Vec<usize> {
        let selectable = vec![true; items.len()];
        self.pick_some(title, items, preselected, &selectable)
    }

    /// [`Prompt::pick_many`] where items with `selectable[i] == false` are shown but refused.
    pub fn pick_some(
        &mut self,
        title: &str,
        items: &[String],
        preselected: &[usize],
        selectable: &[bool],
    ) -> Vec<usize> {
        let can = |i: usize| selectable.get(i).copied().unwrap_or(true);
        self.say(title);
        for (i, item) in items.iter().enumerate() {
            let mark = if preselected.contains(&i) {
                "[x]"
            } else {
                "[ ]"
            };
            self.say(format!("{:>4} {mark} {item}", i + 1));
        }
        loop {
            self.put("Numbers like 1 3 5, all or none; Enter keeps the [x] ones: ");
            let Some(answer) = self.line() else {
                return preselected.to_vec();
            };
            match answer.to_lowercase().as_str() {
                "" => return preselected.to_vec(),
                "all" => return (0..items.len()).filter(|&i| can(i)).collect(),
                "none" => return Vec::new(),
                _ => {}
            }
            let mut picked = Vec::new();
            let mut problem = None;
            for tok in answer.split(|c: char| c == ',' || c.is_whitespace()) {
                if tok.is_empty() {
                    continue;
                }
                match tok.parse::<usize>() {
                    Ok(n) if (1..=items.len()).contains(&n) && can(n - 1) => {
                        if !picked.contains(&(n - 1)) {
                            picked.push(n - 1);
                        }
                    }
                    Ok(n) if (1..=items.len()).contains(&n) => {
                        problem = Some(format!("{n} cannot be picked."));
                        break;
                    }
                    _ => {
                        problem = Some(format!(
                            "Please use numbers from 1 to {}, all or none.",
                            items.len()
                        ));
                        break;
                    }
                }
            }
            match problem {
                Some(p) => self.say(p),
                None => {
                    picked.sort_unstable();
                    return picked;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn prompt(input: &str) -> Prompt<Cursor<Vec<u8>>, Vec<u8>> {
        Prompt::new(Cursor::new(input.as_bytes().to_vec()), Vec::new())
    }

    #[test]
    fn menu_reasks_and_returns_zero_at_eof() {
        let mut p = prompt("0\n3\n2\n");
        assert_eq!(p.menu("T", &["a", "b"]), 2);
        assert_eq!(p.menu("T", &["a", "b"]), 0);
        let out = String::from_utf8(p.into_output()).unwrap();
        assert_eq!(out.matches("Please enter a number from 1 to 2.").count(), 2);
    }

    #[test]
    fn ask_and_confirm_defaults() {
        let mut p = prompt("\nx\n\nmaybe\nY\n");
        assert_eq!(p.ask("Q", Some("d")), "d");
        assert_eq!(p.ask("Q", None), "x");
        assert!(!p.confirm("C", false));
        assert!(p.confirm("C", false));
        assert!(!p.confirm("C", true), "end of input cancels");
        assert!(p.eof());
    }

    #[test]
    fn pick_many_forms() {
        let items: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let mut p = prompt("\n3 1\nall\nnone\n4\n2,3\n");
        assert_eq!(p.pick_many("T", &items, &[1]), [1]);
        assert_eq!(p.pick_many("T", &items, &[1]), [0, 2]);
        assert_eq!(p.pick_many("T", &items, &[]), [0, 1, 2]);
        assert!(p.pick_many("T", &items, &[0]).is_empty());
        assert_eq!(p.pick_many("T", &items, &[]), [1, 2]);
    }

    #[test]
    fn unselectable_items_are_refused() {
        let items: Vec<String> = ["a", "b"].map(String::from).to_vec();
        let mut p = prompt("2\nall\n");
        assert_eq!(p.pick_some("T", &items, &[], &[true, false]), [0]);
        let out = String::from_utf8(p.into_output()).unwrap();
        assert!(out.contains("2 cannot be picked."));
    }

    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("broken"))
        }
    }

    #[test]
    fn read_error_quits_with_a_message() {
        let mut p = Prompt::new(std::io::BufReader::new(Broken), Vec::new());
        assert_eq!(p.menu("T", &["a"]), 0);
        assert!(p.eof());
        let out = String::from_utf8(p.into_output()).unwrap();
        assert!(out.contains("Could not read input; quitting."), "{out}");
    }
}
