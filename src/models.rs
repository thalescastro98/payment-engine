use std::fmt;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, Serializer};

use crate::error::ValidationError;

pub const MAX_AMOUNT_DECIMAL_PLACES: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    Confirmed,
    DisputeInProgress,
    ChargedBack,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransactionRecordType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

impl fmt::Display for TransactionRecordType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Deposit => "deposit",
            Self::Withdrawal => "withdrawal",
            Self::Dispute => "dispute",
            Self::Resolve => "resolve",
            Self::Chargeback => "chargeback",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Deserialize)]
pub struct TransactionRecord {
    #[serde(rename = "type")]
    pub transaction_type: TransactionRecordType,
    #[serde(rename = "client")]
    pub client_id: u16,
    #[serde(rename = "tx")]
    pub transaction_id: u32,
    pub amount: Option<Decimal>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DepositTransaction {
    pub client_id: u16,
    pub transaction_id: u32,
    pub amount: Decimal,
}

#[derive(Debug, PartialEq, Eq)]
pub struct WithdrawalTransaction {
    pub client_id: u16,
    pub transaction_id: u32,
    pub amount: Decimal,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DisputeTransaction {
    pub client_id: u16,
    pub transaction_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ResolveTransaction {
    pub client_id: u16,
    pub transaction_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ChargebackTransaction {
    pub client_id: u16,
    pub transaction_id: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Transaction {
    Deposit(DepositTransaction),
    Withdrawal(WithdrawalTransaction),
    Dispute(DisputeTransaction),
    Resolve(ResolveTransaction),
    Chargeback(ChargebackTransaction),
}

impl Transaction {
    pub fn transaction_type(&self) -> TransactionRecordType {
        match self {
            Self::Deposit(_) => TransactionRecordType::Deposit,
            Self::Withdrawal(_) => TransactionRecordType::Withdrawal,
            Self::Dispute(_) => TransactionRecordType::Dispute,
            Self::Resolve(_) => TransactionRecordType::Resolve,
            Self::Chargeback(_) => TransactionRecordType::Chargeback,
        }
    }

    pub fn client_id(&self) -> u16 {
        match self {
            Self::Deposit(t) => t.client_id,
            Self::Withdrawal(t) => t.client_id,
            Self::Dispute(t) => t.client_id,
            Self::Resolve(t) => t.client_id,
            Self::Chargeback(t) => t.client_id,
        }
    }

    pub fn transaction_id(&self) -> u32 {
        match self {
            Self::Deposit(t) => t.transaction_id,
            Self::Withdrawal(t) => t.transaction_id,
            Self::Dispute(t) => t.transaction_id,
            Self::Resolve(t) => t.transaction_id,
            Self::Chargeback(t) => t.transaction_id,
        }
    }
}

impl TryFrom<TransactionRecord> for Transaction {
    type Error = ValidationError;

    fn try_from(record: TransactionRecord) -> Result<Self, Self::Error> {
        let TransactionRecord {
            transaction_type,
            client_id,
            transaction_id,
            amount,
        } = record;

        let require_amount = || match amount {
            Some(amount) if amount.scale() > MAX_AMOUNT_DECIMAL_PLACES => {
                Err(ValidationError::TooManyDecimalPlaces {
                    transaction_type,
                    client_id,
                    transaction_id,
                    amount,
                })
            }
            Some(amount) if amount <= Decimal::ZERO => Err(ValidationError::NonPositiveAmount {
                transaction_type,
                client_id,
                transaction_id,
                amount,
            }),
            Some(amount) => Ok(amount),
            None => Err(ValidationError::MissingAmount {
                transaction_type,
                client_id,
                transaction_id,
            }),
        };

        let require_no_amount = || match amount {
            Some(amount) => Err(ValidationError::UnexpectedAmount {
                transaction_type,
                client_id,
                transaction_id,
                amount,
            }),
            None => Ok(()),
        };

        let transaction = match transaction_type {
            TransactionRecordType::Deposit => Self::Deposit(DepositTransaction {
                client_id,
                transaction_id,
                amount: require_amount()?,
            }),
            TransactionRecordType::Withdrawal => Self::Withdrawal(WithdrawalTransaction {
                client_id,
                transaction_id,
                amount: require_amount()?,
            }),
            TransactionRecordType::Dispute => {
                require_no_amount()?;
                Self::Dispute(DisputeTransaction {
                    client_id,
                    transaction_id,
                })
            }
            TransactionRecordType::Resolve => {
                require_no_amount()?;
                Self::Resolve(ResolveTransaction {
                    client_id,
                    transaction_id,
                })
            }
            TransactionRecordType::Chargeback => {
                require_no_amount()?;
                Self::Chargeback(ChargebackTransaction {
                    client_id,
                    transaction_id,
                })
            }
        };

        Ok(transaction)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    #[default]
    Active,
    Locked,
}

#[derive(Debug, Default)]
pub struct ClientAccount {
    pub available_amount: Decimal,
    pub held_amount: Decimal,
    pub status: AccountStatus,
}

impl ClientAccount {
    pub fn get_total_amount(&self) -> Decimal {
        self.available_amount + self.held_amount
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct AccountSummary {
    pub client_id: u16,
    pub available_amount: Decimal,
    pub held_amount: Decimal,
    pub total_amount: Decimal,
    pub status: AccountStatus,
}

#[derive(Debug, Serialize)]
pub struct AccountSummaryRecord {
    pub client: u16,
    #[serde(serialize_with = "serialize_amount")]
    pub available: Decimal,
    #[serde(serialize_with = "serialize_amount")]
    pub held: Decimal,
    #[serde(serialize_with = "serialize_amount")]
    pub total: Decimal,
    pub locked: bool,
}

impl AccountSummaryRecord {
    pub const HEADERS: [&'static str; 5] = ["client", "available", "held", "total", "locked"];
}

/// Writes an amount without trailing zeros but always with at least one
/// decimal place, so `2.0000` becomes `2.0` and `1.5000` becomes `1.5`.
fn serialize_amount<S: Serializer>(amount: &Decimal, serializer: S) -> Result<S::Ok, S::Error> {
    let amount = amount.normalize();
    let text = if amount.scale() == 0 {
        format!("{amount:.1}")
    } else {
        amount.to_string()
    };
    serializer.serialize_str(&text)
}

impl From<AccountSummary> for AccountSummaryRecord {
    fn from(summary: AccountSummary) -> Self {
        Self {
            client: summary.client_id,
            available: summary.available_amount,
            held: summary.held_amount,
            total: summary.total_amount,
            locked: summary.status == AccountStatus::Locked,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoredTransactionKind {
    Deposit(DepositTransaction),
    Withdrawal(WithdrawalTransaction),
}

impl StoredTransactionKind {
    pub fn transaction_type(&self) -> TransactionRecordType {
        match self {
            Self::Deposit(_) => TransactionRecordType::Deposit,
            Self::Withdrawal(_) => TransactionRecordType::Withdrawal,
        }
    }

    pub fn amount(&self) -> Decimal {
        match self {
            Self::Deposit(t) => t.amount,
            Self::Withdrawal(t) => t.amount,
        }
    }

    pub fn client_id(&self) -> u16 {
        match self {
            Self::Deposit(t) => t.client_id,
            Self::Withdrawal(t) => t.client_id,
        }
    }

    pub fn transaction_id(&self) -> u32 {
        match self {
            Self::Deposit(t) => t.transaction_id,
            Self::Withdrawal(t) => t.transaction_id,
        }
    }
}

#[derive(Debug)]
pub struct StoredTransaction {
    pub kind: StoredTransactionKind,
    pub status: TransactionStatus,
}

impl StoredTransaction {
    pub fn new(kind: StoredTransactionKind) -> Self {
        Self {
            kind,
            status: TransactionStatus::Confirmed,
        }
    }

    pub fn deposit(&self) -> Option<&DepositTransaction> {
        match &self.kind {
            StoredTransactionKind::Deposit(deposit) => Some(deposit),
            StoredTransactionKind::Withdrawal(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_ID: u16 = 1;
    const TRANSACTION_ID: u32 = 10;

    fn amount(text: &str) -> Decimal {
        text.parse().expect("valid decimal literal")
    }

    fn record(transaction_type: TransactionRecordType, amount: Option<&str>) -> TransactionRecord {
        TransactionRecord {
            transaction_type,
            client_id: CLIENT_ID,
            transaction_id: TRANSACTION_ID,
            amount: amount.map(self::amount),
        }
    }

    #[test]
    fn deposit_with_amount_is_accepted() {
        let result = Transaction::try_from(record(TransactionRecordType::Deposit, Some("1.5")));

        assert_eq!(
            result,
            Ok(Transaction::Deposit(DepositTransaction {
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("1.5"),
            }))
        );
    }

    #[test]
    fn withdrawal_with_amount_is_accepted() {
        let result = Transaction::try_from(record(TransactionRecordType::Withdrawal, Some("2.0")));

        assert_eq!(
            result,
            Ok(Transaction::Withdrawal(WithdrawalTransaction {
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("2.0"),
            }))
        );
    }

    #[test]
    fn dispute_without_amount_is_accepted() {
        let result = Transaction::try_from(record(TransactionRecordType::Dispute, None));

        assert_eq!(
            result,
            Ok(Transaction::Dispute(DisputeTransaction {
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
            }))
        );
    }

    #[test]
    fn resolve_without_amount_is_accepted() {
        let result = Transaction::try_from(record(TransactionRecordType::Resolve, None));

        assert_eq!(
            result,
            Ok(Transaction::Resolve(ResolveTransaction {
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
            }))
        );
    }

    #[test]
    fn chargeback_without_amount_is_accepted() {
        let result = Transaction::try_from(record(TransactionRecordType::Chargeback, None));

        assert_eq!(
            result,
            Ok(Transaction::Chargeback(ChargebackTransaction {
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
            }))
        );
    }

    #[test]
    fn amount_with_exactly_four_decimal_places_is_accepted() {
        let result = Transaction::try_from(record(TransactionRecordType::Deposit, Some("0.0001")));

        assert!(result.is_ok());
    }

    #[test]
    fn amount_with_more_than_four_decimal_places_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Deposit, Some("1.00001")));

        assert_eq!(
            result,
            Err(ValidationError::TooManyDecimalPlaces {
                transaction_type: TransactionRecordType::Deposit,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("1.00001"),
            })
        );
    }

    #[test]
    fn decimal_places_are_counted_as_written_even_if_trailing_zeros() {
        let result = Transaction::try_from(record(TransactionRecordType::Deposit, Some("1.50000")));

        assert_eq!(
            result,
            Err(ValidationError::TooManyDecimalPlaces {
                transaction_type: TransactionRecordType::Deposit,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("1.50000"),
            })
        );
    }

    #[test]
    fn negative_deposit_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Deposit, Some("-1.0")));

        assert_eq!(
            result,
            Err(ValidationError::NonPositiveAmount {
                transaction_type: TransactionRecordType::Deposit,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("-1.0"),
            })
        );
    }

    #[test]
    fn zero_withdrawal_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Withdrawal, Some("0.0")));

        assert_eq!(
            result,
            Err(ValidationError::NonPositiveAmount {
                transaction_type: TransactionRecordType::Withdrawal,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("0.0"),
            })
        );
    }

    #[test]
    fn deposit_without_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Deposit, None));

        assert_eq!(
            result,
            Err(ValidationError::MissingAmount {
                transaction_type: TransactionRecordType::Deposit,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
            })
        );
    }

    #[test]
    fn withdrawal_without_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Withdrawal, None));

        assert_eq!(
            result,
            Err(ValidationError::MissingAmount {
                transaction_type: TransactionRecordType::Withdrawal,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
            })
        );
    }

    #[test]
    fn dispute_with_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Dispute, Some("1.0")));

        assert_eq!(
            result,
            Err(ValidationError::UnexpectedAmount {
                transaction_type: TransactionRecordType::Dispute,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("1.0"),
            })
        );
    }

    #[test]
    fn resolve_with_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Resolve, Some("1.0")));

        assert_eq!(
            result,
            Err(ValidationError::UnexpectedAmount {
                transaction_type: TransactionRecordType::Resolve,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("1.0"),
            })
        );
    }

    #[test]
    fn chargeback_with_amount_is_rejected() {
        let result = Transaction::try_from(record(TransactionRecordType::Chargeback, Some("1.0")));

        assert_eq!(
            result,
            Err(ValidationError::UnexpectedAmount {
                transaction_type: TransactionRecordType::Chargeback,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("1.0"),
            })
        );
    }

    #[test]
    fn amount_on_non_amount_operation_is_rejected_even_if_invalid() {
        // The "no amount allowed" rule wins over amount-format rules.
        let result =
            Transaction::try_from(record(TransactionRecordType::Dispute, Some("-1.00001")));

        assert_eq!(
            result,
            Err(ValidationError::UnexpectedAmount {
                transaction_type: TransactionRecordType::Dispute,
                client_id: CLIENT_ID,
                transaction_id: TRANSACTION_ID,
                amount: amount("-1.00001"),
            })
        );
    }
}
