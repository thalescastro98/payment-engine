//! End-to-end tests: run the compiled binary on CSV fixtures and check the
//! CSV it prints, the lines it logs, and its exit code.

use std::path::PathBuf;
use std::process::{Command, Output};

const HEADER: &str = "client,available,held,total,locked";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_payment-engine"))
        .args(args)
        .output()
        .expect("failed to run payment-engine binary")
}

fn run_fixture(name: &str) -> Output {
    run(&[fixture(name).to_str().unwrap()])
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is utf-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is utf-8")
}

/// Splits stdout into the header line and the sorted data rows.
/// Row order in the output is unspecified, so tests compare sorted rows.
fn parse_stdout(output: &Output) -> (String, Vec<String>) {
    let text = stdout(output);
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default().to_string();
    let mut rows: Vec<String> = lines.map(str::to_string).collect();
    rows.sort();
    (header, rows)
}

fn skipped_lines(output: &Output) -> Vec<String> {
    stderr(output)
        .lines()
        .filter(|line| line.starts_with("skipping "))
        .map(str::to_string)
        .collect()
}

fn assert_rows(output: &Output, expected: &[&str]) {
    let (header, rows) = parse_stdout(output);
    let mut expected: Vec<String> = expected.iter().map(|row| row.to_string()).collect();
    expected.sort();

    assert_eq!(header, HEADER);
    assert_eq!(rows, expected);
}

// ----- CLI behaviour -----

#[test]
fn missing_argument_prints_usage_and_fails() {
    let output = run(&[]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).starts_with("usage:"));
}

#[test]
fn too_many_arguments_prints_usage_and_fails() {
    let output = run(&["a.csv", "b.csv"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with("usage:"));
}

#[test]
fn missing_input_file_fails_without_output() {
    let output = run(&[fixture("does_not_exist.csv").to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(1));
    assert!(stdout(&output).is_empty());
    assert!(stderr(&output).starts_with("error processing "));
}

// ----- empty inputs -----

#[test]
fn header_only_input_produces_header_only_output() {
    let output = run_fixture("header_only.csv");

    assert_eq!(output.status.code(), Some(0));
    assert_rows(&output, &[]);
    assert!(skipped_lines(&output).is_empty());
}

#[test]
fn completely_empty_file_produces_header_only_output() {
    let output = run_fixture("completely_empty.csv");

    assert_eq!(output.status.code(), Some(0));
    assert_rows(&output, &[]);
    assert!(skipped_lines(&output).is_empty());
}

// ----- input formatting -----

#[test]
fn whitespace_around_fields_is_ignored() {
    let output = run_fixture("whitespace.csv");

    assert_eq!(output.status.code(), Some(0));
    // 1.0 + 2.0 - 0.5 = 2.5, then tx 2 (2.0) disputed: available 0.5, held 2.0.
    assert_rows(&output, &["1,0.5,2.0,2.5,false"]);
    assert!(skipped_lines(&output).is_empty());
}

#[test]
fn malformed_rows_are_skipped_and_valid_rows_still_apply() {
    let output = run_fixture("malformed.csv");

    assert_eq!(output.status.code(), Some(0));
    // Only tx 1 (10.0) and tx 11 (2.5) are valid.
    assert_rows(&output, &["1,12.5,0.0,12.5,false"]);

    let skipped = skipped_lines(&output);
    assert_eq!(
        skipped.len(),
        11,
        "unexpected skip lines:\n{}",
        skipped.join("\n")
    );

    let joined = skipped.join("\n");
    // CSV-level problems carry the line number from the csv crate.
    assert!(joined.contains("malformed row") && joined.contains("line: 3"));
    assert!(joined.contains("unknown variant `refund`"));
    assert!(joined.contains("found record with 3 fields"));
    assert!(joined.contains("found record with 5 fields"));
    assert!(joined.contains("number too large to fit in target type"));
    // Validation problems name the operation, client and transaction.
    assert!(
        joined.contains("deposit 4 for client 1: amount 1.12345 has more than 4 decimal places")
    );
    assert!(joined.contains("deposit 5 for client 1: amount -2 must be greater than zero"));
    assert!(joined.contains("deposit 6 for client 1: amount is required for deposit"));
    assert!(joined.contains(
        "dispute 1 for client 1: amount 5 was given, but dispute does not take an amount"
    ));
}

// ----- larger scenarios -----

#[test]
fn disputes_and_resolves_across_several_clients() {
    let output = run_fixture("disputes_and_resolves.csv");

    assert_eq!(output.status.code(), Some(0));
    assert_rows(
        &output,
        &[
            "1,1.0,0.0,1.0,false",
            "2,20.0,0.0,20.0,false",
            "3,30.0001,0.0,30.0001,false",
            "4,0.0,0.0,0.0,false",
            "5,7.0,0.0,7.0,false",
        ],
    );

    let skipped = skipped_lines(&output);
    assert_eq!(
        skipped,
        vec![
            "skipping withdrawal 6 for client 2: insufficient funds (available 50, required 60)",
            "skipping withdrawal 12 for client 2: insufficient funds (available 50, required 55)",
            "skipping dispute 3 for client 3: insufficient funds (available 20.0001, required 25.5)",
        ]
    );
}

#[test]
fn chargebacks_lock_accounts_and_duplicates_are_rejected() {
    let output = run_fixture("chargebacks_and_duplicates.csv");

    assert_eq!(output.status.code(), Some(0));
    assert_rows(
        &output,
        &[
            "1,30.0,0.0,30.0,true",
            "2,29.5,0.0,29.5,false",
            "3,0.0,0.0,0.0,true",
            "4,2.5,0.0,2.5,false",
        ],
    );

    let skipped = skipped_lines(&output);
    assert_eq!(
        skipped,
        vec![
            "skipping deposit 1 for client 1: this exact transaction has already been processed",
            "skipping deposit 2 for client 2: id already used by a deposit of 30 for client 2",
            "skipping dispute 5 for client 1: transaction 5 is DisputeInProgress, expected Confirmed",
            "skipping deposit 6 for client 1: account is locked",
            "skipping withdrawal 7 for client 1: account is locked",
            "skipping resolve 5 for client 1: account is locked",
            "skipping withdrawal 8 for client 2: insufficient funds (available 0, required 1)",
            "skipping dispute 1 for client 4: transaction 1 belongs to client 1",
            "skipping dispute 99 for client 4: transaction 99 not found",
            "skipping dispute 10 for client 4: transaction 10 is a withdrawal, not a deposit",
            "skipping resolve 9 for client 4: transaction 9 is Confirmed, expected DisputeInProgress",
            "skipping chargeback 9 for client 4: transaction 9 is Confirmed, expected DisputeInProgress",
            "skipping withdrawal 11 for client 5: account not found",
        ]
    );
}

#[test]
fn ids_at_the_edges_of_their_ranges_are_handled() {
    let output = run_fixture("boundary_ids.csv");

    assert_eq!(output.status.code(), Some(0));
    assert_rows(
        &output,
        &[
            "0,1.0,0.0,1.0,false",
            "65534,0.0,0.0,0.0,true",
            "65535,95.0,0.0,95.0,false",
        ],
    );
    assert_eq!(
        skipped_lines(&output),
        vec![
            "skipping dispute 4294967294 for client 65534: insufficient funds (available 40, required 50)",
            "skipping deposit 4294967295 for client 65535: this exact transaction has already been processed",
        ]
    );
}

#[test]
fn example_from_the_spec() {
    let output = run(&[concat!(env!("CARGO_MANIFEST_DIR"), "/transactions.csv")]);

    assert_eq!(output.status.code(), Some(0));
    assert_rows(&output, &["1,1.5,0.0,1.5,false", "2,2.0,0.0,2.0,false"]);
    assert_eq!(
        skipped_lines(&output),
        vec!["skipping withdrawal 5 for client 2: insufficient funds (available 2, required 3)"]
    );
}
