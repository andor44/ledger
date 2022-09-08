use rust_decimal::Decimal;
use std::collections::HashMap;
use thiserror::Error;

use crate::{Balance, Transaction, TransactionAmount, TransactionId};

// ProcessedTransactionState represents the state of a transaction that's been
// successfully applied to an account.
// * Settled: successfully applied (deposited/withdrawn)
// * Disputed: the transaction was disputed after being settled. Its amount
//   has been deducted from the available amount and added to the held amount.
//   A future resolution transaction can return it to settled state, adding
//   the amount to the available, and subtracting it from the held.
// * ChargeBacked: a disputed transaction can be chargebacked by the client.
//   The transaction may not be further modified.
#[derive(PartialEq, Eq, Debug)]
enum ProcessedTransactionState {
    Settled,
    Disputed,
    ChargeBacked,
}

#[derive(Debug)]
struct ProcessedTransaction {
    amount: TransactionAmount,
    state: ProcessedTransactionState,
}

#[derive(Error, PartialEq, Eq, Debug)]
pub enum TransactionError {
    #[error("The account is frozen")]
    AccountFrozen,
    #[error("Insufficient funds to withdraw requested amount")]
    InsufficientFunds,
    #[error("Attempted dispute, resolution, or chargeback of a transaction that doesn't exist")]
    NonexistentTransaction,
    #[error("The transaction that was attempted to dispute is already under dispute")]
    AlreadyDisputed,
    #[error("The transaction that was attempted to resolve is not under dispute")]
    NotDisputed,
}

#[derive(Debug)]
pub struct Account {
    // if an account is frozen no transactions can be applied to it
    frozen: bool,

    available: Decimal,
    held: Decimal,

    // `past_txs` stores transactions which:
    // 1. Have already been successfully applied to the account.
    // 2. Are of type Withdrawal or Deposit
    // because these can be disputed, and for the dispute
    // the amount needs to be known.
    past_txs: HashMap<TransactionId, ProcessedTransaction>,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            frozen: false,
            available: TransactionAmount::ZERO,
            held: TransactionAmount::ZERO,
            past_txs: HashMap::new(),
        }
    }
}

impl Account {
    pub fn held(&self) -> Balance {
        self.held
    }

    pub fn available(&self) -> Balance {
        self.available
    }

    pub fn total(&self) -> Balance {
        self.available + self.held
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    pub fn try_apply_transaction(
        &mut self,
        new_transaction_id: TransactionId,
        transaction: Transaction,
    ) -> Result<(), TransactionError> {
        use ProcessedTransactionState::*;
        use Transaction::*;

        // NOTE: the below code assumes that `new_transaction_id` is globally unique,
        // as in the specification. Otherwise, entries in `self.past_txs` would get
        // overwritten.
        match transaction {
            Deposit { amount } => {
                // If an account is frozen it can't be deposited to
                if self.frozen {
                    return Err(TransactionError::AccountFrozen);
                }

                self.past_txs.insert(
                    new_transaction_id,
                    ProcessedTransaction {
                        amount: amount,
                        state: Settled,
                    },
                );

                self.available = self.available + amount;
            }
            Withdrawal { amount } => {
                // If an account is frozen it can't be withdrawn from
                if self.frozen {
                    return Err(TransactionError::AccountFrozen);
                }

                if self.available < amount {
                    return Err(TransactionError::InsufficientFunds);
                }

                self.past_txs.insert(
                    new_transaction_id,
                    ProcessedTransaction {
                        amount: amount,
                        state: Settled,
                    },
                );

                self.available -= amount;
            }
            Dispute { id } => {
                let processed_transaction = self
                    .past_txs
                    .get_mut(&id)
                    .ok_or(TransactionError::NonexistentTransaction)?;

                // A transaction can only be disputed if it is currently Settled.
                if processed_transaction.state != Settled {
                    return Err(TransactionError::AlreadyDisputed);
                }

                processed_transaction.state = Disputed;

                self.available -= processed_transaction.amount;
                self.held += processed_transaction.amount;
            }
            Resolve { id } => {
                let processed_transaction = self
                    .past_txs
                    .get_mut(&id)
                    .ok_or(TransactionError::NonexistentTransaction)?;

                // A transaction can only be resolved if it's being disputed.
                if processed_transaction.state != Disputed {
                    return Err(TransactionError::NotDisputed);
                }

                processed_transaction.state = Settled;

                self.available += processed_transaction.amount;
                self.held -= processed_transaction.amount;
            }
            Chargeback { id } => {
                let processed_transaction = self
                    .past_txs
                    .get_mut(&id)
                    .ok_or(TransactionError::NonexistentTransaction)?;

                // A transaction can only be resolved if it's being disputed.
                if processed_transaction.state != Disputed {
                    return Err(TransactionError::NotDisputed);
                }

                processed_transaction.state = ChargeBacked;

                self.frozen = true;
                self.held -= processed_transaction.amount;
            }
        };

        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use crate::{account::TransactionError, Balance, Transaction::*};

    use super::Account;

    fn verify_account<T: Into<Balance>>(account: &Account, available: T, held: T, is_frozen: bool) {
        let available = available.into();
        let held = held.into();

        assert_eq!(account.available(), available);
        assert_eq!(account.held(), held);
        assert_eq!(account.is_frozen(), is_frozen);
        assert_eq!(account.total(), available + held);
    }

    #[test]
    fn deposit() {
        let mut account = Account::default();
        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());

        verify_account(&account, 10, 0, false);
    }

    #[test]
    fn withdraw() {
        let mut account = Account::default();
        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());
        assert!(account
            .try_apply_transaction(2, Withdrawal { amount: 4.into() })
            .is_ok());

        verify_account(&account, 6, 0, false);
    }

    #[test]
    fn overdraft_is_rejected() {
        let mut account = Account::default();
        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());
        assert!(account
            .try_apply_transaction(2, Withdrawal { amount: 4.into() })
            .is_ok());
        assert_eq!(
            account.try_apply_transaction(3, Withdrawal { amount: 8.into() }),
            Err(TransactionError::InsufficientFunds)
        );

        verify_account(&account, 6, 0, false);
    }

    #[test]
    fn frozen_account() {
        let mut account = Account::default();

        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());
        account.frozen = true;
        assert_eq!(
            account.try_apply_transaction(2, Withdrawal { amount: 4.into() }),
            Err(TransactionError::AccountFrozen)
        );
        assert_eq!(
            account.try_apply_transaction(3, Deposit { amount: 8.into() }),
            Err(TransactionError::AccountFrozen)
        );

        verify_account(&account, 10, 0, true);
    }

    #[test]
    fn dispute_and_resolve() {
        let mut account = Account::default();

        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());
        assert!(account.try_apply_transaction(2, Dispute { id: 1 }).is_ok());

        // The deposit is disputed, it should be shown as held
        verify_account(&account, 0, 10, false);

        assert!(account
            .try_apply_transaction(3, Deposit { amount: 5.into() })
            .is_ok());

        // The new deposit goes through without issues
        verify_account(&account, 5, 10, false);

        assert!(account.try_apply_transaction(2, Resolve { id: 1 }).is_ok());

        // After resolution the held amount is released
        verify_account(&account, 15, 0, false);
    }

    #[test]
    fn invalid_transitions() {
        let mut account = Account::default();

        // Referring to transactions that don't exist
        assert_eq!(
            account.try_apply_transaction(1, Dispute { id: 10 }),
            Err(TransactionError::NonexistentTransaction)
        );
        assert_eq!(
            account.try_apply_transaction(1, Resolve { id: 10 }),
            Err(TransactionError::NonexistentTransaction)
        );
        assert_eq!(
            account.try_apply_transaction(1, Chargeback { id: 10 }),
            Err(TransactionError::NonexistentTransaction)
        );

        // Try to dispute a transaction that's already disputed
        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());
        assert!(account.try_apply_transaction(2, Dispute { id: 1 }).is_ok());
        assert_eq!(
            account.try_apply_transaction(3, Dispute { id: 1 }),
            Err(TransactionError::AlreadyDisputed)
        );

        // Resolve it, then try to resolve again
        assert!(account.try_apply_transaction(4, Resolve { id: 1 }).is_ok());
        assert_eq!(
            account.try_apply_transaction(5, Resolve { id: 1 }),
            Err(TransactionError::NotDisputed)
        );
    }

    #[test]
    fn chargeback_freezes_account() {
        let mut account = Account::default();

        assert!(account
            .try_apply_transaction(1, Deposit { amount: 10.into() })
            .is_ok());
        assert!(account
            .try_apply_transaction(2, Deposit { amount: 15.into() })
            .is_ok());

        verify_account(&account, 25, 0, false);

        assert!(account.try_apply_transaction(3, Dispute { id: 1 }).is_ok());

        verify_account(&account, 15, 10, false);

        assert!(account
            .try_apply_transaction(4, Chargeback { id: 1 })
            .is_ok());

        verify_account(&account, 15, 0, true);

        // At this point no new deposits or withdrawals can be made
        assert_eq!(
            account.try_apply_transaction(5, Deposit { amount: 8.into() }),
            Err(TransactionError::AccountFrozen)
        );
        verify_account(&account, 15, 0, true);
        assert_eq!(
            account.try_apply_transaction(5, Withdrawal { amount: 8.into() }),
            Err(TransactionError::AccountFrozen)
        );
        verify_account(&account, 15, 0, true);

        // But existing transactions can still be disputed...
        assert!(account.try_apply_transaction(5, Dispute { id: 2 }).is_ok());
        verify_account(&account, 0, 15, true);
        // ... and resolved
        assert!(account.try_apply_transaction(6, Resolve { id: 2 }).is_ok());
        verify_account(&account, 15, 0, true);
    }
}
