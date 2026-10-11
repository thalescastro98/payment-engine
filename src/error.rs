use std::error::Error;
use std::fmt;

use rust_decimal::Decimal;

use crate::models::{MAX_AMOUNT_DECIMAL_PLACES, TransactionRecordType, TransactionStatus};

/// Why a CSV row could not be turned into a `Transaction`. Every variant
/// identifies the row (`transaction_type`, `client_id`, `transaction_id`).
#[derive(Debug, PartialEq, Eq)]
pub enum ValidationError {
    TooManyDecimalPlaces {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        amount: Decimal,
    },
    NonPositiveAmount {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        amount: Decimal,
    },
    UnexpectedAmount {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        amount: Decimal,
    },
    MissingAmount {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyDecimalPlaces {
                transaction_type,
                client_id,
                transaction_id,
                amount,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: amount {amount} has more than {MAX_AMOUNT_DECIMAL_PLACES} decimal places"
            ),
            Self::NonPositiveAmount {
                transaction_type,
                client_id,
                transaction_id,
                amount,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: amount {amount} must be greater than zero"
            ),
            Self::UnexpectedAmount {
                transaction_type,
                client_id,
                transaction_id,
                amount,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: amount {amount} was given, but {transaction_type} does not take an amount"
            ),
            Self::MissingAmount {
                transaction_type,
                client_id,
                transaction_id,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: amount is required for {transaction_type}"
            ),
        }
    }
}

impl Error for ValidationError {}

/// Why an operation could not be applied. Every variant identifies the
/// operation being attempted (`transaction_type`, `client_id`,
/// `transaction_id`).
#[derive(Debug, PartialEq, Eq)]
pub enum PaymentProcessorManagerError {
    NonPositiveAmount {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        amount: Decimal,
    },
    AccountLocked {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
    },
    AccountNotFound {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
    },
    InsufficientFunds {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        available: Decimal,
        required: Decimal,
    },
    TransactionNotFound {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
    },
    TransactionAlreadyProcessed {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
    },
    TransactionIdAlreadyExists {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        existing_transaction_type: TransactionRecordType,
        existing_client_id: u16,
        existing_amount: Decimal,
    },
    TransactionIsNotADeposit {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        found_transaction_type: TransactionRecordType,
    },
    TransactionBelongsToAnotherClient {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        owner_client_id: u16,
    },
    UnexpectedTransactionStatus {
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        status: TransactionStatus,
        expected_status: TransactionStatus,
    },
}

impl fmt::Display for PaymentProcessorManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveAmount {
                transaction_type,
                client_id,
                transaction_id,
                amount,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: amount {amount} must be greater than zero"
            ),
            Self::AccountLocked {
                transaction_type,
                client_id,
                transaction_id,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: account is locked"
            ),
            Self::AccountNotFound {
                transaction_type,
                client_id,
                transaction_id,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: account not found"
            ),
            Self::InsufficientFunds {
                transaction_type,
                client_id,
                transaction_id,
                available,
                required,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: insufficient funds (available {available}, required {required})"
            ),
            Self::TransactionNotFound {
                transaction_type,
                client_id,
                transaction_id,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: transaction {transaction_id} not found"
            ),
            Self::TransactionAlreadyProcessed {
                transaction_type,
                client_id,
                transaction_id,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: this exact transaction has already been processed"
            ),
            Self::TransactionIdAlreadyExists {
                transaction_type,
                client_id,
                transaction_id,
                existing_transaction_type,
                existing_client_id,
                existing_amount,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: id already used by a {existing_transaction_type} of {existing_amount} for client {existing_client_id}"
            ),
            Self::TransactionIsNotADeposit {
                transaction_type,
                client_id,
                transaction_id,
                found_transaction_type,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: transaction {transaction_id} is a {found_transaction_type}, not a deposit"
            ),
            Self::TransactionBelongsToAnotherClient {
                transaction_type,
                client_id,
                transaction_id,
                owner_client_id,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: transaction {transaction_id} belongs to client {owner_client_id}"
            ),
            Self::UnexpectedTransactionStatus {
                transaction_type,
                client_id,
                transaction_id,
                status,
                expected_status,
            } => write!(
                f,
                "{transaction_type} {transaction_id} for client {client_id}: transaction {transaction_id} is {status:?}, expected {expected_status:?}"
            ),
        }
    }
}

impl Error for PaymentProcessorManagerError {}
