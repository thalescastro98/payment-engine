mod error;
mod models;
mod payment_processor_manager;

use std::env;
use std::error::Error;
use std::io;
use std::process;

use models::{AccountSummaryRecord, Transaction, TransactionRecord};
use payment_processor_manager::PaymentProcessorManager;

fn run(path: &str) -> Result<(), Box<dyn Error>> {
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_path(path)?;

    let mut manager = PaymentProcessorManager::new();

    for record in reader.deserialize::<TransactionRecord>() {
        let record = match record {
            Ok(record) => record,
            Err(err) if err.is_io_error() => return Err(err.into()),
            Err(err) => {
                eprintln!("skipping malformed row: {err}");
                continue;
            }
        };

        let transaction = match Transaction::try_from(record) {
            Ok(transaction) => transaction,
            Err(err) => {
                eprintln!("skipping {err}");
                continue;
            }
        };

        if let Err(err) = manager.process(transaction) {
            eprintln!("skipping {err}");
        }
    }

    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(io::stdout());
    writer.write_record(AccountSummaryRecord::HEADERS)?;
    for summary in manager.get_accounts_summary() {
        writer.serialize(AccountSummaryRecord::from(summary))?;
    }
    writer.flush()?;

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: {} <transactions.csv>", args[0]);
        process::exit(1);
    }

    if let Err(err) = run(&args[1]) {
        eprintln!("error processing {}: {err}", args[1]);
        process::exit(1);
    }
}
