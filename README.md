# payment-engine

A small Rust command-line tool that reads a CSV of financial transactions and prints the final state of every client account as CSV.

It processes deposits, withdrawals, disputes, resolves and chargebacks, in file order, keeping track of available and held funds per client and locking an account after a chargeback.

## Running

```sh
cargo run -- transactions.csv > accounts.csv
```

The account summary goes to **stdout**. Anything that could not be processed is reported on **stderr**, one line per skipped row, so redirecting stdout to a file gives you a clean CSV and the log stays visible in the terminal.

The process exits with status `0` after a run, even if some rows were skipped. It exits with status `1` only when the input file cannot be read or the arguments are wrong.

## Tests

```sh
cargo test
```

This runs:

- unit tests for row validation (`src/models.rs`) and for the processing rules (`src/payment_processor_manager.rs`);
- end-to-end tests in `tests/cli.rs`, which run the compiled binary against the CSV fixtures in `tests/fixtures/` and check the output, the log lines and the exit code.

## Input format

A CSV with a header and four columns:

```csv
type, client, tx, amount
deposit, 1, 1, 1.0
deposit, 2, 2, 2.0
deposit, 1, 3, 2.0
withdrawal, 1, 4, 1.5
withdrawal, 2, 5, 3.0
dispute, 1, 3,
resolve, 1, 3,
```

| Column   | Type                    | Notes                                                                 |
|----------|-------------------------|-----------------------------------------------------------------------|
| `type`   | string                  | `deposit`, `withdrawal`, `dispute`, `resolve`, `chargeback`           |
| `client` | u16                     | client id                                                             |
| `tx`     | u32                     | transaction id                                                        |
| `amount` | decimal, up to 4 places | required for `deposit` and `withdrawal`; must be empty for the others |

Whitespace around values is ignored. Rows without an amount still need the trailing comma, as in the example above, so every row has four fields.

Rows that cannot be parsed or fail validation are skipped and logged. Processing continues with the next row.

## Output format

```csv
client,available,held,total,locked
1,1.5,0.0,1.5,false
2,2.0,0.0,2.0,false
```

| Column      | Meaning                                               |
|-------------|-------------------------------------------------------|
| `client`    | client id                                             |
| `available` | funds the client can withdraw                         |
| `held`      | funds frozen by open disputes                         |
| `total`     | `available + held`                                    |
| `locked`    | `true` once a chargeback has happened on this account |

Amounts are printed without trailing zeros but always with at least one decimal place (`2.0`, `1.5`, `0.0001`). Rows are not sorted; their order is unspecified.

## How it works

The code is split into four modules under `src/`:

- `main.rs` reads the input row by row, converts each row into a typed transaction, hands it to the processor and finally writes the summary.
- `models.rs` holds the data types. `TransactionRecord` is the raw CSV row. `Transaction` is an enum with one variant per operation, each carrying exactly the fields that operation needs. The conversion between the two is where validation happens, so processing code never has to re-check the input.
- `payment_processor_manager.rs` holds `PaymentProcessorManager`, which owns the accounts and applies the rules.
- `error.rs` defines the two error types: validation errors for bad rows and processing errors for rows that are well-formed but cannot be applied. Every error names the operation, client and transaction it refers to, plus whatever explains the failure.

The processor keeps two maps: client id to account, and transaction id to the stored deposit or withdrawal together with its status (`Confirmed`, `DisputeInProgress` or `ChargedBack`).

The rules, in the order they are checked:

- Any operation on a **locked** account is rejected.
- **Deposit** adds to available funds, creating the account if needed.
- **Withdrawal** subtracts from available funds. It is rejected if the client has no account or not enough available funds.
- Deposits and withdrawals are **rejected if their transaction id was already used**. The error distinguishes an exact replay of the same row from a different row reusing the id.
- **Dispute** references an earlier deposit of the same client. It moves the deposit amount from available to held and marks the deposit as disputed. It is rejected if the deposit is not found, is not a deposit, belongs to another client, is already disputed or charged back, or if available funds would go negative.
- **Resolve** reverses a dispute: the amount moves back from held to available and the deposit returns to `Confirmed`. It can be disputed again later.
- **Chargeback** closes a dispute by removing the held amount and locking the account. The deposit becomes `ChargedBack` and cannot be disputed again.

Failures are returned as typed errors and logged by `main.rs`; the processor never prints anything itself.

## Decisions and things that could be different

- **Decimal places are counted as written.** `1.50000` is rejected because it has five places, even though its value fits in four. This is a one-line check on the parsed value's scale and is easy to relax to a value-based check.
- **Amounts use `rust_decimal::Decimal`.** An alternative is to parse the input into an `i64` scaled by 10,000 and do integer arithmetic. That would be smaller and faster, but `Decimal` is more flexible if the required precision ever changes and avoids hand-rolled scaling in the output.
- **Dispute, resolve and chargeback must not carry an amount.** A row such as `dispute, 1, 3, 2.0` is rejected. Dropping this validation and simply ignoring the amount would not affect anything else.
- **Zero amounts are rejected** along with negative ones. A zero deposit or withdrawal is a no-op, so refusing it costs nothing, but accepting it would be a one-character change.
- **Bad rows are skipped, not fatal.** A malformed or invalid row is logged and the run continues. Only I/O errors stop the program. The alternative, failing the whole run on the first bad row, is a small change in `main.rs`.
- **A failed withdrawal does not reserve its transaction id.** Only applied deposits and withdrawals are stored, so a later row may reuse the id of a withdrawal that was rejected.
- **Only deposits can be disputed.** Withdrawals are stored, but a dispute referencing one is rejected.
- **Every applied deposit and withdrawal is kept in memory** for the whole run, so memory grows with the input. Withdrawals are only kept for duplicate detection and could be reduced to their ids if memory became a concern.
- **Log messages use `Decimal`'s default formatting**, which drops trailing zeros (`available 50`), while the output CSV always shows at least one decimal place (`50.0`). Both could share the same formatter.
