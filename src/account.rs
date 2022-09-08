use rust_decimal::Decimal;

use crate::{
    ledger::{ProcessedTransaction, ProcessedTransactionState, ProcessedTxsForAccount},
    Balance, Transaction, TransactionAmount, TransactionError,
};

#[derive(Debug)]
pub struct Account {
    // if an account is frozen no transactions can be applied to it
    frozen: bool,

    available: Decimal,
    held: Decimal,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            frozen: false,
            available: TransactionAmount::ZERO,
            held: TransactionAmount::ZERO,
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
        past_txs: &mut ProcessedTxsForAccount,
        transaction: Transaction,
    ) -> Result<(), TransactionError> {
        use ProcessedTransactionState::*;
        use Transaction::*;

        // NOTE: the below code assumes that the new transaction IDs in `Deposit`
        // and `Withdrawal` transactions are unique, as per the specification.
        // If not, they will overwrite existing transactions.
        match transaction {
            Deposit { new_id, amount } => {
                // If an account is frozen it can't be deposited to
                if self.frozen {
                    return Err(TransactionError::AccountFrozen);
                }

                past_txs.insert_processed(
                    new_id,
                    ProcessedTransaction {
                        amount: amount,
                        state: Settled,
                    },
                );

                self.available = self.available + amount;
            }
            Withdrawal { new_id, amount } => {
                // If an account is frozen it can't be withdrawn from
                if self.frozen {
                    return Err(TransactionError::AccountFrozen);
                }

                if self.available < amount {
                    return Err(TransactionError::InsufficientFunds);
                }

                past_txs.insert_processed(
                    new_id,
                    ProcessedTransaction {
                        amount: amount,
                        state: Settled,
                    },
                );

                self.available -= amount;
            }
            Dispute { id } => {
                let processed_transaction = past_txs
                    .find(id)
                    .ok_or(TransactionError::NonexistentTransaction)?;

                // A transaction can only be disputed if it is currently Settled.
                if processed_transaction.state != Settled {
                    return Err(TransactionError::NotSettled);
                }

                processed_transaction.state = Disputed;

                self.available -= processed_transaction.amount;
                self.held += processed_transaction.amount;
            }
            Resolve { id } => {
                let processed_transaction = past_txs
                    .find(id)
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
                let processed_transaction = past_txs
                    .find(id)
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
    use crate::{
        account::TransactionError::*, ledger::ProcessedTxsForAccount, Balance, Transaction::*,
    };

    use super::Account;

    fn verify_account<T: Into<Balance>>(account: &Account, available: T, held: T, is_frozen: bool) {
        let available = available.into();
        let held = held.into();

        assert_eq!(account.available(), available);
        assert_eq!(account.held(), held);
        assert_eq!(account.is_frozen(), is_frozen);
        assert_eq!(account.total(), available + held);
    }

    fn setup() -> (Account, ProcessedTxsForAccount<'static>) {
        use crate::ledger::ProcessedTxs;

        let account = Account::default();
        let past_txs = Box::leak(Box::new(ProcessedTxs::default()));
        let x = ProcessedTxsForAccount::for_account(past_txs, 1);
        (account, x)
    }

    #[test]
    fn deposit() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());

        verify_account(&account, 10, 0, false);
    }

    #[test]
    fn withdraw() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        assert!(account
            .try_apply_transaction(
                past_txs,
                Withdrawal {
                    new_id: 2,
                    amount: 4.into()
                }
            )
            .is_ok());

        verify_account(&account, 6, 0, false);
    }

    #[test]
    fn overdraft_is_rejected() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        assert!(account
            .try_apply_transaction(
                past_txs,
                Withdrawal {
                    new_id: 2,
                    amount: 4.into()
                }
            )
            .is_ok());
        assert_eq!(
            account.try_apply_transaction(
                past_txs,
                Withdrawal {
                    new_id: 3,
                    amount: 8.into()
                }
            ),
            Err(InsufficientFunds)
        );

        verify_account(&account, 6, 0, false);
    }

    #[test]
    fn frozen_account() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        account.frozen = true;
        assert_eq!(
            account.try_apply_transaction(
                past_txs,
                Withdrawal {
                    new_id: 2,
                    amount: 4.into()
                }
            ),
            Err(AccountFrozen)
        );
        assert_eq!(
            account.try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 3,
                    amount: 8.into()
                }
            ),
            Err(AccountFrozen)
        );

        verify_account(&account, 10, 0, true);
    }

    #[test]
    fn dispute_and_resolve() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        assert!(account
            .try_apply_transaction(past_txs, Dispute { id: 1 })
            .is_ok());

        // The deposit is disputed, it should be shown as held
        verify_account(&account, 0, 10, false);

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 2,
                    amount: 5.into()
                }
            )
            .is_ok());

        // The new deposit goes through without issues
        verify_account(&account, 5, 10, false);

        assert!(account
            .try_apply_transaction(past_txs, Resolve { id: 1 })
            .is_ok());

        // After resolution the held amount is released
        verify_account(&account, 15, 0, false);
    }

    #[test]
    fn invalid_transitions() {
        let (mut account, ref mut past_txs) = setup();

        // Referring to transactions that don't exist
        assert_eq!(
            account.try_apply_transaction(past_txs, Dispute { id: 10 }),
            Err(NonexistentTransaction)
        );
        assert_eq!(
            account.try_apply_transaction(past_txs, Resolve { id: 10 }),
            Err(NonexistentTransaction)
        );
        assert_eq!(
            account.try_apply_transaction(past_txs, Chargeback { id: 10 }),
            Err(NonexistentTransaction)
        );

        // Try to dispute a transaction that's already disputed
        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        assert!(account
            .try_apply_transaction(past_txs, Dispute { id: 1 })
            .is_ok());
        assert_eq!(
            account.try_apply_transaction(past_txs, Dispute { id: 1 }),
            Err(NotSettled)
        );

        // Resolve it, then try to resolve again
        assert!(account
            .try_apply_transaction(past_txs, Resolve { id: 1 })
            .is_ok());
        assert_eq!(
            account.try_apply_transaction(past_txs, Resolve { id: 1 }),
            Err(NotDisputed)
        );
    }

    #[test]
    fn chargeback_freezes_account() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 2,
                    amount: 15.into()
                }
            )
            .is_ok());

        verify_account(&account, 25, 0, false);

        assert!(account
            .try_apply_transaction(past_txs, Dispute { id: 1 })
            .is_ok());

        verify_account(&account, 15, 10, false);

        assert!(account
            .try_apply_transaction(past_txs, Chargeback { id: 1 })
            .is_ok());

        verify_account(&account, 15, 0, true);

        // At this point no new deposits or withdrawals can be made
        assert_eq!(
            account.try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 3,
                    amount: 8.into()
                }
            ),
            Err(AccountFrozen)
        );
        verify_account(&account, 15, 0, true);
        assert_eq!(
            account.try_apply_transaction(
                past_txs,
                Withdrawal {
                    new_id: 4,
                    amount: 8.into()
                }
            ),
            Err(AccountFrozen)
        );
        verify_account(&account, 15, 0, true);

        // But existing transactions can still be disputed...
        assert!(account
            .try_apply_transaction(past_txs, Dispute { id: 2 })
            .is_ok());
        verify_account(&account, 0, 15, true);
        // ... and resolved
        assert!(account
            .try_apply_transaction(past_txs, Resolve { id: 2 })
            .is_ok());
        verify_account(&account, 15, 0, true);
    }

    #[test]
    fn chargebacked_transaction_is_final() {
        let (mut account, ref mut past_txs) = setup();

        assert!(account
            .try_apply_transaction(
                past_txs,
                Deposit {
                    new_id: 1,
                    amount: 10.into()
                }
            )
            .is_ok());
        assert!(account
            .try_apply_transaction(past_txs, Dispute { id: 1 })
            .is_ok());
        assert!(account
            .try_apply_transaction(past_txs, Chargeback { id: 1 })
            .is_ok());

        assert_eq!(
            account.try_apply_transaction(past_txs, Dispute { id: 1 }),
            Err(NotSettled)
        );
        assert_eq!(
            account.try_apply_transaction(past_txs, Resolve { id: 1 }),
            Err(NotDisputed)
        );
    }
}
