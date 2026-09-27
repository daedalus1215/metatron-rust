//! Exit codes are a contract with CI, and this file is where it is written down:
//!
//! * `0` — the command ran and the answer is in the output.
//! * `1` — the command ran and the answer is "no".
//! * `2` — the command produced no answer at all.
//!
//! The distinction that matters is 1 against 2. A pipeline that reads `1` as
//! "architecture regressed" and `2` as "the tool is broken" cannot tell a real
//! regression from a typo in a path if the tool reports both as `1`.

use std::process::Command;

const NO_ANSWER: i32 = 2;
const REGRESSION: i32 = 1;

/// Every subcommand, so a new one cannot be added without a decision about
/// what it returns when it fails.
const COMMANDS: &[&str] = &["scan", "classify", "cohesion", "check", "views", "baseline"];

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_metatron"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the metatron binary should be runnable")
}

fn code(args: &[&str]) -> i32 {
    run(args)
        .status
        .code()
        .expect("the process should exit rather than be signalled")
}

#[test]
fn a_directory_that_is_not_there_is_not_a_regression() {
    for cmd in COMMANDS {
        assert_eq!(
            code(&[cmd, "no/such/directory"]),
            NO_ANSWER,
            "`metatron {cmd}` returned {} for a directory it never saw, so a CI \
             job cannot tell a broken build from a broken tool",
            code(&[cmd, "no/such/directory"])
        );
    }
}

#[test]
fn a_directory_that_is_not_there_says_so_on_stderr() {
    for cmd in COMMANDS {
        let out = run(&[cmd, "no/such/directory"]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("metatron:") && err.contains("no/such/directory"),
            "`metatron {cmd}` failed without saying which directory it wanted: {err}"
        );
    }
}

#[test]
fn a_scan_that_ran_is_zero() {
    assert_eq!(code(&["scan", "--stdout", "tests/fixtures/dual"]), 0);
}

#[test]
fn the_json_and_text_paths_of_check_agree() {
    // `--json` used to `std::process::exit` from inside the print branch, and
    // the text path returned normally. Two exits for one verdict is how the
    // two paths drift apart; they are now one return value.
    let text = code(&["check", "."]);
    let json = code(&["check", "--json", "."]);
    assert_eq!(
        text, json,
        "`check` and `check --json` disagreed on the verdict"
    );
    assert!(
        text == 0 || text == REGRESSION,
        "a check that ran should answer 0 or {REGRESSION}, got {text}"
    );
}

#[test]
fn an_unrecognised_flag_is_not_a_regression() {
    // clap exits 2 on a usage error, which is the same meaning: nothing was
    // checked. Asserted so a future clap change cannot quietly make a typo
    // read as a failing architecture.
    assert_eq!(code(&["check", "--min-covrage", "50", "."]), NO_ANSWER);
}
