use std::io::{self, BufRead, Write};

pub(crate) fn prompt(label: &str) -> String {
    eprint!("{label}");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap_or(0);
    line.trim().to_string()
}

pub(crate) fn prompt_default(label: &str, default: &str) -> String {
    eprint!("{label} [{default}]: ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap_or(0);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn confirm(label: &str) -> bool {
    let answer = prompt(&format!("{label} [y/N]: "));
    matches!(answer.to_lowercase().as_str(), "y" | "yes")
}

pub(crate) fn confirm_default_yes(label: &str) -> bool {
    let answer = prompt(&format!("{label} [Y/n]: "));
    !matches!(answer.to_lowercase().as_str(), "n" | "no")
}

pub(crate) fn select(label: &str, options: &[&str]) -> usize {
    eprintln!("\n{label}");
    for (i, opt) in options.iter().enumerate() {
        eprintln!("  {}) {opt}", i + 1);
    }
    loop {
        let answer = prompt("Choice: ");
        if let Ok(n) = answer.parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return n - 1;
            }
        }
        eprintln!("  Please enter a number between 1 and {}", options.len());
    }
}

pub(crate) fn confirm_typed(label: &str, expected_phrase: &str) -> bool {
    eprintln!("{label}");
    loop {
        let answer = prompt(&format!("  Type \"{expected_phrase}\" to continue: "));
        if answer.eq_ignore_ascii_case(expected_phrase) {
            return true;
        }
        if answer.eq_ignore_ascii_case("skip") || answer.is_empty() {
            return false;
        }
        eprintln!("  Please type exactly \"{expected_phrase}\" (or \"skip\" to skip).");
    }
}

pub(crate) fn separator() {
    eprintln!("\n{}\n", "─".repeat(60));
}
