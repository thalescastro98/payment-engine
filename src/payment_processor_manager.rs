use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::error::PaymentProcessorManagerError;
use crate::models::{
    AccountStatus, AccountSummary, ChargebackTransaction, ClientAccount, DepositTransaction,
    DisputeTransaction, ResolveTransaction, StoredTransaction, StoredTransactionKind, Transaction,
    TransactionRecordType, TransactionStatus, WithdrawalTransaction,
};

type PaymentProcessorManagerResult<T> = Result<T, PaymentProcessorManagerError>;

pub struct PaymentProcessorManager {
    accounts: HashMap<u16, ClientAccount>,
    transactions: HashMap<u32, StoredTransaction>,
}

impl PaymentProcessorManager {
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            transactions: HashMap::new(),
        }
    }

    pub fn get_accounts_summary(&self) -> Vec<AccountSummary> {
        self.accounts
            .iter()
            .map(|(client_id, account)| AccountSummary {
                client_id: *client_id,
                available_amount: account.available_amount,
                held_amount: account.held_amount,
                total_amount: account.get_total_amount(),
                status: account.status,
            })
            .collect()
    }

    pub fn process(&mut self, transaction: Transaction) -> PaymentProcessorManagerResult<()> {
        self.ensure_account_not_locked(
            transaction.transaction_type(),
            transaction.client_id(),
            transaction.transaction_id(),
        )?;

        match transaction {
            Transaction::Deposit(deposit) => self.process_deposit(deposit),
            Transaction::Withdrawal(withdrawal) => self.process_withdrawal(withdrawal),
            Transaction::Dispute(dispute) => self.process_dispute(&dispute),
            Transaction::Resolve(resolve) => self.process_resolve(&resolve),
            Transaction::Chargeback(chargeback) => self.process_chargeback(&chargeback),
        }
    }

    fn process_deposit(
        &mut self,
        deposit: DepositTransaction,
    ) -> PaymentProcessorManagerResult<()> {
        let incoming = StoredTransactionKind::Deposit(deposit);
        Self::ensure_positive_amount(&incoming)?;

        if let Some(existing) = self.transactions.get(&incoming.transaction_id()) {
            return Err(Self::duplicate_error(existing, &incoming));
        }

        let account = self.accounts.entry(incoming.client_id()).or_default();
        account.available_amount += incoming.amount();

        self.transactions
            .insert(incoming.transaction_id(), StoredTransaction::new(incoming));

        Ok(())
    }

    fn process_withdrawal(
        &mut self,
        withdrawal: WithdrawalTransaction,
    ) -> PaymentProcessorManagerResult<()> {
        let incoming = StoredTransactionKind::Withdrawal(withdrawal);
        Self::ensure_positive_amount(&incoming)?;

        if let Some(existing) = self.transactions.get(&incoming.transaction_id()) {
            return Err(Self::duplicate_error(existing, &incoming));
        }

        let account = self.accounts.get_mut(&incoming.client_id()).ok_or(
            PaymentProcessorManagerError::AccountNotFound {
                transaction_type: incoming.transaction_type(),
                client_id: incoming.client_id(),
                transaction_id: incoming.transaction_id(),
            },
        )?;

        if account.available_amount < incoming.amount() {
            return Err(PaymentProcessorManagerError::InsufficientFunds {
                transaction_type: incoming.transaction_type(),
                client_id: incoming.client_id(),
                transaction_id: incoming.transaction_id(),
                available: account.available_amount,
                required: incoming.amount(),
            });
        }

        account.available_amount -= incoming.amount();

        self.transactions
            .insert(incoming.transaction_id(), StoredTransaction::new(incoming));

        Ok(())
    }

    fn process_dispute(
        &mut self,
        dispute: &DisputeTransaction,
    ) -> PaymentProcessorManagerResult<()> {
        let transaction_type = TransactionRecordType::Dispute;
        let (amount, status, account) = self.find_deposit(
            transaction_type,
            dispute.client_id,
            dispute.transaction_id,
            TransactionStatus::Confirmed,
        )?;

        if account.available_amount < amount {
            return Err(PaymentProcessorManagerError::InsufficientFunds {
                transaction_type,
                client_id: dispute.client_id,
                transaction_id: dispute.transaction_id,
                available: account.available_amount,
                required: amount,
            });
        }

        account.available_amount -= amount;
        account.held_amount += amount;
        *status = TransactionStatus::DisputeInProgress;

        Ok(())
    }

    fn process_resolve(
        &mut self,
        resolve: &ResolveTransaction,
    ) -> PaymentProcessorManagerResult<()> {
        let (amount, status, account) = self.find_deposit(
            TransactionRecordType::Resolve,
            resolve.client_id,
            resolve.transaction_id,
            TransactionStatus::DisputeInProgress,
        )?;

        account.held_amount -= amount;
        account.available_amount += amount;
        *status = TransactionStatus::Confirmed;

        Ok(())
    }

    fn process_chargeback(
        &mut self,
        chargeback: &ChargebackTransaction,
    ) -> PaymentProcessorManagerResult<()> {
        let (amount, status, account) = self.find_deposit(
            TransactionRecordType::Chargeback,
            chargeback.client_id,
            chargeback.transaction_id,
            TransactionStatus::DisputeInProgress,
        )?;

        account.held_amount -= amount;
        account.status = AccountStatus::Locked;
        *status = TransactionStatus::ChargedBack;

        Ok(())
    }

    fn duplicate_error(
        existing: &StoredTransaction,
        incoming: &StoredTransactionKind,
    ) -> PaymentProcessorManagerError {
        let transaction_type = incoming.transaction_type();
        let client_id = incoming.client_id();
        let transaction_id = incoming.transaction_id();

        if existing.kind == *incoming {
            PaymentProcessorManagerError::TransactionAlreadyProcessed {
                transaction_type,
                client_id,
                transaction_id,
            }
        } else {
            PaymentProcessorManagerError::TransactionIdAlreadyExists {
                transaction_type,
                client_id,
                transaction_id,
                existing_transaction_type: existing.kind.transaction_type(),
                existing_client_id: existing.kind.client_id(),
                existing_amount: existing.kind.amount(),
            }
        }
    }

    fn ensure_positive_amount(
        incoming: &StoredTransactionKind,
    ) -> PaymentProcessorManagerResult<()> {
        if incoming.amount() <= Decimal::ZERO {
            return Err(PaymentProcessorManagerError::NonPositiveAmount {
                transaction_type: incoming.transaction_type(),
                client_id: incoming.client_id(),
                transaction_id: incoming.transaction_id(),
                amount: incoming.amount(),
            });
        }

        Ok(())
    }

    fn ensure_account_not_locked(
        &self,
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
    ) -> PaymentProcessorManagerResult<()> {
        match self.accounts.get(&client_id) {
            Some(account) if account.status == AccountStatus::Locked => {
                Err(PaymentProcessorManagerError::AccountLocked {
                    transaction_type,
                    client_id,
                    transaction_id,
                })
            }
            _ => Ok(()),
        }
    }

    /// Looks up deposit `transaction_id` belonging to `client_id`, only if it
    /// is currently in `expected_status`. `transaction_type` is the operation
    /// being attempted and is only used to describe errors.
    fn find_deposit(
        &mut self,
        transaction_type: TransactionRecordType,
        client_id: u16,
        transaction_id: u32,
        expected_status: TransactionStatus,
    ) -> PaymentProcessorManagerResult<(Decimal, &mut TransactionStatus, &mut ClientAccount)> {
        let stored = self.transactions.get_mut(&transaction_id).ok_or(
            PaymentProcessorManagerError::TransactionNotFound {
                transaction_type,
                client_id,
                transaction_id,
            },
        )?;

        let deposit =
            stored
                .deposit()
                .ok_or(PaymentProcessorManagerError::TransactionIsNotADeposit {
                    transaction_type,
                    client_id,
                    transaction_id,
                    found_transaction_type: stored.kind.transaction_type(),
                })?;

        if deposit.client_id != client_id {
            return Err(
                PaymentProcessorManagerError::TransactionBelongsToAnotherClient {
                    transaction_type,
                    client_id,
                    transaction_id,
                    owner_client_id: deposit.client_id,
                },
            );
        }

        if stored.status != expected_status {
            return Err(PaymentProcessorManagerError::UnexpectedTransactionStatus {
                transaction_type,
                client_id,
                transaction_id,
                status: stored.status,
                expected_status,
            });
        }

        let amount = deposit.amount;
        let account = self.accounts.get_mut(&client_id).ok_or(
            PaymentProcessorManagerError::AccountNotFound {
                transaction_type,
                client_id,
                transaction_id,
            },
        )?;

        Ok((amount, &mut stored.status, account))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amount(text: &str) -> Decimal {
        text.parse().expect("valid decimal literal")
    }

    fn deposit(client_id: u16, transaction_id: u32, value: &str) -> Transaction {
        Transaction::Deposit(DepositTransaction {
            client_id,
            transaction_id,
            amount: amount(value),
        })
    }

    fn withdrawal(client_id: u16, transaction_id: u32, value: &str) -> Transaction {
        Transaction::Withdrawal(WithdrawalTransaction {
            client_id,
            transaction_id,
            amount: amount(value),
        })
    }

    fn dispute(client_id: u16, transaction_id: u32) -> Transaction {
        Transaction::Dispute(DisputeTransaction {
            client_id,
            transaction_id,
        })
    }

    fn resolve(client_id: u16, transaction_id: u32) -> Transaction {
        Transaction::Resolve(ResolveTransaction {
            client_id,
            transaction_id,
        })
    }

    fn chargeback(client_id: u16, transaction_id: u32) -> Transaction {
        Transaction::Chargeback(ChargebackTransaction {
            client_id,
            transaction_id,
        })
    }

    /// Processes every transaction, panicking on the first failure.
    fn process_all(manager: &mut PaymentProcessorManager, transactions: Vec<Transaction>) {
        for transaction in transactions {
            manager
                .process(transaction)
                .expect("setup transaction should succeed");
        }
    }

    fn summary(manager: &PaymentProcessorManager, client_id: u16) -> AccountSummary {
        manager
            .get_accounts_summary()
            .into_iter()
            .find(|summary| summary.client_id == client_id)
            .unwrap_or_else(|| panic!("no account for client {client_id}"))
    }

    fn assert_account(
        manager: &PaymentProcessorManager,
        client_id: u16,
        available: &str,
        held: &str,
        status: AccountStatus,
    ) {
        let available = amount(available);
        let held = amount(held);
        assert_eq!(
            summary(manager, client_id),
            AccountSummary {
                client_id,
                available_amount: available,
                held_amount: held,
                total_amount: available + held,
                status,
            }
        );
    }

    // ----- deposits -----

    #[test]
    fn deposit_creates_account_with_available_funds() {
        let mut manager = PaymentProcessorManager::new();

        assert_eq!(manager.process(deposit(1, 1, "10.5")), Ok(()));

        assert_account(&manager, 1, "10.5", "0", AccountStatus::Active);
    }

    #[test]
    fn deposits_accumulate_per_client() {
        let mut manager = PaymentProcessorManager::new();

        process_all(
            &mut manager,
            vec![
                deposit(1, 1, "1.0"),
                deposit(2, 2, "2.0"),
                deposit(1, 3, "2.5"),
            ],
        );

        assert_account(&manager, 1, "3.5", "0", AccountStatus::Active);
        assert_account(&manager, 2, "2.0", "0", AccountStatus::Active);
        assert_eq!(manager.get_accounts_summary().len(), 2);
    }

    #[test]
    fn deposit_keeps_four_decimal_places_exact() {
        let mut manager = PaymentProcessorManager::new();

        process_all(
            &mut manager,
            vec![deposit(1, 1, "0.0001"), deposit(1, 2, "0.0002")],
        );

        assert_eq!(summary(&manager, 1).available_amount, amount("0.0003"));
    }

    #[test]
    fn deposit_with_non_positive_amount_is_rejected_and_creates_no_account() {
        let mut manager = PaymentProcessorManager::new();

        assert_eq!(
            manager.process(deposit(1, 1, "-1.0")),
            Err(PaymentProcessorManagerError::NonPositiveAmount {
                transaction_type: TransactionRecordType::Deposit,
                client_id: 1,
                transaction_id: 1,
                amount: amount("-1.0"),
            })
        );
        assert_eq!(
            manager.process(deposit(1, 2, "0")),
            Err(PaymentProcessorManagerError::NonPositiveAmount {
                transaction_type: TransactionRecordType::Deposit,
                client_id: 1,
                transaction_id: 2,
                amount: amount("0"),
            })
        );

        assert!(manager.get_accounts_summary().is_empty());
    }

    // ----- withdrawals -----

    #[test]
    fn withdrawal_reduces_available_funds() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(manager.process(withdrawal(1, 2, "1.5")), Ok(()));

        assert_account(&manager, 1, "3.5", "0", AccountStatus::Active);
    }

    #[test]
    fn withdrawal_of_the_entire_balance_is_allowed() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(manager.process(withdrawal(1, 2, "5.0")), Ok(()));

        assert_account(&manager, 1, "0", "0", AccountStatus::Active);
    }

    #[test]
    fn withdrawal_for_unknown_client_is_rejected() {
        let mut manager = PaymentProcessorManager::new();

        assert_eq!(
            manager.process(withdrawal(9, 1, "1.0")),
            Err(PaymentProcessorManagerError::AccountNotFound {
                transaction_type: TransactionRecordType::Withdrawal,
                client_id: 9,
                transaction_id: 1,
            })
        );

        assert!(manager.get_accounts_summary().is_empty());
    }

    #[test]
    fn withdrawal_exceeding_available_funds_is_rejected_and_changes_nothing() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "2.0")]);

        assert_eq!(
            manager.process(withdrawal(1, 2, "3.0")),
            Err(PaymentProcessorManagerError::InsufficientFunds {
                transaction_type: TransactionRecordType::Withdrawal,
                client_id: 1,
                transaction_id: 2,
                available: amount("2.0"),
                required: amount("3.0"),
            })
        );

        assert_account(&manager, 1, "2.0", "0", AccountStatus::Active);
    }

    #[test]
    fn withdrawal_only_counts_available_funds_not_held() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), deposit(1, 2, "1.0"), dispute(1, 1)],
        );
        assert_account(&manager, 1, "1.0", "5.0", AccountStatus::Active);

        assert!(matches!(
            manager.process(withdrawal(1, 3, "2.0")),
            Err(PaymentProcessorManagerError::InsufficientFunds { .. })
        ));

        assert_account(&manager, 1, "1.0", "5.0", AccountStatus::Active);
    }

    #[test]
    fn withdrawal_with_non_positive_amount_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(
            manager.process(withdrawal(1, 2, "-1.0")),
            Err(PaymentProcessorManagerError::NonPositiveAmount {
                transaction_type: TransactionRecordType::Withdrawal,
                client_id: 1,
                transaction_id: 2,
                amount: amount("-1.0"),
            })
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    // ----- duplicate ids -----

    #[test]
    fn identical_deposit_replay_is_rejected_and_not_applied_twice() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(
            manager.process(deposit(1, 1, "5.0")),
            Err(PaymentProcessorManagerError::TransactionAlreadyProcessed {
                transaction_type: TransactionRecordType::Deposit,
                client_id: 1,
                transaction_id: 1,
            })
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    #[test]
    fn deposit_reusing_an_id_with_different_data_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        let expected = |client_id, transaction_type| {
            Err(PaymentProcessorManagerError::TransactionIdAlreadyExists {
                transaction_type,
                client_id,
                transaction_id: 1,
                existing_transaction_type: TransactionRecordType::Deposit,
                existing_client_id: 1,
                existing_amount: amount("5.0"),
            })
        };

        // different amount
        assert_eq!(
            manager.process(deposit(1, 1, "6.0")),
            expected(1, TransactionRecordType::Deposit)
        );
        // different client
        assert_eq!(
            manager.process(deposit(2, 1, "5.0")),
            expected(2, TransactionRecordType::Deposit)
        );
        // different type
        assert_eq!(
            manager.process(withdrawal(1, 1, "5.0")),
            expected(1, TransactionRecordType::Withdrawal)
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
        assert_eq!(manager.get_accounts_summary().len(), 1);
    }

    #[test]
    fn identical_withdrawal_replay_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), withdrawal(1, 2, "1.0")],
        );

        assert_eq!(
            manager.process(withdrawal(1, 2, "1.0")),
            Err(PaymentProcessorManagerError::TransactionAlreadyProcessed {
                transaction_type: TransactionRecordType::Withdrawal,
                client_id: 1,
                transaction_id: 2,
            })
        );

        assert_account(&manager, 1, "4.0", "0", AccountStatus::Active);
    }

    #[test]
    fn failed_withdrawal_does_not_reserve_its_id() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert!(manager.process(withdrawal(1, 2, "100.0")).is_err());
        assert_eq!(manager.process(withdrawal(1, 2, "1.0")), Ok(()));

        assert_account(&manager, 1, "4.0", "0", AccountStatus::Active);
    }

    // ----- disputes -----

    #[test]
    fn dispute_moves_deposit_amount_from_available_to_held() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), deposit(1, 2, "3.0")],
        );

        assert_eq!(manager.process(dispute(1, 2)), Ok(()));

        assert_account(&manager, 1, "5.0", "3.0", AccountStatus::Active);
        assert_eq!(summary(&manager, 1).total_amount, amount("8.0"));
    }

    #[test]
    fn dispute_of_unknown_transaction_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(
            manager.process(dispute(1, 99)),
            Err(PaymentProcessorManagerError::TransactionNotFound {
                transaction_type: TransactionRecordType::Dispute,
                client_id: 1,
                transaction_id: 99,
            })
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    #[test]
    fn dispute_of_a_withdrawal_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), withdrawal(1, 2, "1.0")],
        );

        assert_eq!(
            manager.process(dispute(1, 2)),
            Err(PaymentProcessorManagerError::TransactionIsNotADeposit {
                transaction_type: TransactionRecordType::Dispute,
                client_id: 1,
                transaction_id: 2,
                found_transaction_type: TransactionRecordType::Withdrawal,
            })
        );

        assert_account(&manager, 1, "4.0", "0", AccountStatus::Active);
    }

    #[test]
    fn dispute_of_another_clients_deposit_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), deposit(2, 2, "1.0")],
        );

        assert_eq!(
            manager.process(dispute(2, 1)),
            Err(
                PaymentProcessorManagerError::TransactionBelongsToAnotherClient {
                    transaction_type: TransactionRecordType::Dispute,
                    client_id: 2,
                    transaction_id: 1,
                    owner_client_id: 1,
                }
            )
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
        assert_account(&manager, 2, "1.0", "0", AccountStatus::Active);
    }

    #[test]
    fn dispute_of_an_already_disputed_deposit_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0"), dispute(1, 1)]);

        assert_eq!(
            manager.process(dispute(1, 1)),
            Err(PaymentProcessorManagerError::UnexpectedTransactionStatus {
                transaction_type: TransactionRecordType::Dispute,
                client_id: 1,
                transaction_id: 1,
                status: TransactionStatus::DisputeInProgress,
                expected_status: TransactionStatus::Confirmed,
            })
        );

        assert_account(&manager, 1, "0", "5.0", AccountStatus::Active);
    }

    #[test]
    fn dispute_that_would_make_available_negative_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), withdrawal(1, 2, "4.0")],
        );

        assert_eq!(
            manager.process(dispute(1, 1)),
            Err(PaymentProcessorManagerError::InsufficientFunds {
                transaction_type: TransactionRecordType::Dispute,
                client_id: 1,
                transaction_id: 1,
                available: amount("1.0"),
                required: amount("5.0"),
            })
        );
        assert_account(&manager, 1, "1.0", "0", AccountStatus::Active);

        // The deposit is still Confirmed: once funds are back, the dispute goes through.
        process_all(&mut manager, vec![deposit(1, 3, "4.0")]);
        assert_eq!(manager.process(dispute(1, 1)), Ok(()));
        assert_account(&manager, 1, "0", "5.0", AccountStatus::Active);
    }

    // ----- resolves -----

    #[test]
    fn resolve_returns_held_funds_to_available() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), deposit(1, 2, "3.0"), dispute(1, 2)],
        );

        assert_eq!(manager.process(resolve(1, 2)), Ok(()));

        assert_account(&manager, 1, "8.0", "0", AccountStatus::Active);
    }

    #[test]
    fn resolve_without_a_dispute_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(
            manager.process(resolve(1, 1)),
            Err(PaymentProcessorManagerError::UnexpectedTransactionStatus {
                transaction_type: TransactionRecordType::Resolve,
                client_id: 1,
                transaction_id: 1,
                status: TransactionStatus::Confirmed,
                expected_status: TransactionStatus::DisputeInProgress,
            })
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    #[test]
    fn resolved_deposit_can_be_disputed_again() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), dispute(1, 1), resolve(1, 1)],
        );

        assert_eq!(manager.process(dispute(1, 1)), Ok(()));
        assert_account(&manager, 1, "0", "5.0", AccountStatus::Active);

        assert_eq!(manager.process(resolve(1, 1)), Ok(()));
        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    // ----- chargebacks -----

    #[test]
    fn chargeback_removes_held_funds_and_locks_the_account() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), deposit(1, 2, "3.0"), dispute(1, 2)],
        );

        assert_eq!(manager.process(chargeback(1, 2)), Ok(()));

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Locked);
        assert_eq!(summary(&manager, 1).total_amount, amount("5.0"));
    }

    #[test]
    fn chargeback_without_a_dispute_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(&mut manager, vec![deposit(1, 1, "5.0")]);

        assert_eq!(
            manager.process(chargeback(1, 1)),
            Err(PaymentProcessorManagerError::UnexpectedTransactionStatus {
                transaction_type: TransactionRecordType::Chargeback,
                client_id: 1,
                transaction_id: 1,
                status: TransactionStatus::Confirmed,
                expected_status: TransactionStatus::DisputeInProgress,
            })
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    #[test]
    fn chargeback_after_resolve_is_rejected() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![deposit(1, 1, "5.0"), dispute(1, 1), resolve(1, 1)],
        );

        assert!(matches!(
            manager.process(chargeback(1, 1)),
            Err(PaymentProcessorManagerError::UnexpectedTransactionStatus {
                status: TransactionStatus::Confirmed,
                ..
            })
        ));

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Active);
    }

    // ----- locked accounts -----

    #[test]
    fn locked_account_rejects_every_operation_and_keeps_its_balance() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![
                deposit(1, 1, "5.0"),
                deposit(1, 2, "3.0"),
                dispute(1, 2),
                chargeback(1, 2),
            ],
        );
        assert_account(&manager, 1, "5.0", "0", AccountStatus::Locked);

        let locked = |transaction_type, transaction_id| {
            Err(PaymentProcessorManagerError::AccountLocked {
                transaction_type,
                client_id: 1,
                transaction_id,
            })
        };

        assert_eq!(
            manager.process(deposit(1, 3, "1.0")),
            locked(TransactionRecordType::Deposit, 3)
        );
        assert_eq!(
            manager.process(withdrawal(1, 4, "1.0")),
            locked(TransactionRecordType::Withdrawal, 4)
        );
        assert_eq!(
            manager.process(dispute(1, 1)),
            locked(TransactionRecordType::Dispute, 1)
        );
        assert_eq!(
            manager.process(resolve(1, 2)),
            locked(TransactionRecordType::Resolve, 2)
        );
        assert_eq!(
            manager.process(chargeback(1, 2)),
            locked(TransactionRecordType::Chargeback, 2)
        );

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Locked);
    }

    #[test]
    fn locking_one_client_does_not_affect_others() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![
                deposit(1, 1, "5.0"),
                deposit(2, 2, "1.0"),
                dispute(1, 1),
                chargeback(1, 1),
            ],
        );

        assert_eq!(manager.process(deposit(2, 3, "2.0")), Ok(()));
        assert_eq!(manager.process(withdrawal(2, 4, "0.5")), Ok(()));

        assert_account(&manager, 1, "0", "0", AccountStatus::Locked);
        assert_account(&manager, 2, "2.5", "0", AccountStatus::Active);
    }

    #[test]
    fn active_client_cannot_dispute_a_locked_clients_deposit() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![
                deposit(1, 1, "5.0"),
                deposit(1, 2, "1.0"),
                deposit(2, 3, "1.0"),
                dispute(1, 2),
                chargeback(1, 2),
            ],
        );

        assert!(matches!(
            manager.process(dispute(2, 1)),
            Err(PaymentProcessorManagerError::TransactionBelongsToAnotherClient { .. })
        ));

        assert_account(&manager, 1, "5.0", "0", AccountStatus::Locked);
    }

    // ----- end to end -----

    #[test]
    fn example_input_produces_expected_summary() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![
                deposit(1, 1, "1.0"),
                deposit(2, 2, "2.0"),
                deposit(1, 3, "2.0"),
                withdrawal(1, 4, "1.5"),
            ],
        );
        assert!(manager.process(withdrawal(2, 5, "3.0")).is_err());

        assert_account(&manager, 1, "1.5", "0", AccountStatus::Active);
        assert_account(&manager, 2, "2.0", "0", AccountStatus::Active);
        assert_eq!(manager.get_accounts_summary().len(), 2);
    }

    #[test]
    fn full_dispute_lifecycle_across_several_deposits() {
        let mut manager = PaymentProcessorManager::new();
        process_all(
            &mut manager,
            vec![
                deposit(1, 1, "10.0"),
                deposit(1, 2, "4.0"),
                deposit(1, 3, "6.0"),
                withdrawal(1, 4, "5.0"), // available 15
                dispute(1, 2),           // available 11, held 4
                dispute(1, 3),           // available 5, held 10
                resolve(1, 2),           // available 9, held 6
            ],
        );
        assert_account(&manager, 1, "9.0", "6.0", AccountStatus::Active);

        process_all(&mut manager, vec![chargeback(1, 3)]);
        assert_account(&manager, 1, "9.0", "0", AccountStatus::Locked);
        assert_eq!(summary(&manager, 1).total_amount, amount("9.0"));
    }

    #[test]
    fn empty_manager_has_no_accounts() {
        let manager = PaymentProcessorManager::new();

        assert!(manager.get_accounts_summary().is_empty());
    }
}
