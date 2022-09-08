use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    account::Account, AccountId, Balance, Transaction, TransactionAmount, TransactionError,
    TransactionId,
};

// ProcessedTransactionState represents the state of a transaction that's been
// successfully applied to an account.
// * Settled: successfully applied (deposited/withdrawn)
// * Disputed: the transaction was disputed after being settled. Its amount
//   has been deducted from the available amount and added to the held amount.
//   A future resolution transaction can return it to settled state, adding
//   the amount to the available, and subtracting it from the held.
// * ChargeBacked: a disputed transaction can be chargebacked by the client.
//   The transaction may not be further modified.
#[derive(PartialEq, Eq)]
pub enum ProcessedTransactionState {
    Settled,
    Disputed,
    ChargeBacked,
}

pub struct ProcessedTransaction {
    pub amount: TransactionAmount,
    pub state: ProcessedTransactionState,
}

#[derive(Default)]
pub struct Ledger {
    accounts: HashMap<AccountId, Account>,
    processed_txs: ProcessedTxs,
}

impl Ledger {
    // Attempt to apply the given transaction to the given account.
    // If the transaction can't be applied an error is returned and no change
    // is made.
    fn apply_for_account(
        &mut self,
        account: AccountId,
        tx: Transaction,
    ) -> Result<(), TransactionError> {
        let mut txs_for_account =
            ProcessedTxsForAccount::for_account(&mut self.processed_txs, account);
        let account = self.accounts.entry(account).or_default();

        account.try_apply_transaction(&mut txs_for_account, tx)
    }

    // Write the account summaries in this ledger formatted as CSV to the
    // given writer. This consumes the ledger to prevent modification
    // after writing.
    pub fn accounts_to_csv<W: std::io::Write>(self, output: &mut W) {
        let mut writer = csv::WriterBuilder::new()
            .has_headers(true)
            .from_writer(output);

        #[derive(Serialize)]
        struct OutputRecord {
            client: AccountId,
            available: Balance,
            held: Balance,
            total: Balance,
            locked: bool,
        }

        // NOTE: This is not necessary but it makes testing easier.
        // It could be removed at the cost of making tests more complicated.
        let mut sorted_accounts = self.accounts.keys().collect::<Vec<_>>();
        sorted_accounts.sort();

        for account_id in sorted_accounts {
            // This unwrap is okay, we know the key must exist because
            // this method takes self by value, so no one can have access
            // to the accounts map during this iteration.
            let account = self
                .accounts
                .get(account_id)
                .expect("accounts modified during iteration");
            let (mut available, mut held, mut total) =
                (account.available(), account.held(), account.total());

            // Output at most 4 decimal places of precision.
            available.rescale(4);
            held.rescale(4);
            total.rescale(4);

            writer
                .serialize(OutputRecord {
                    client: *account_id,
                    available: available,
                    held: held,
                    total: total,
                    locked: account.is_frozen(),
                })
                .expect("failed to write CSV output");
        }
    }

    pub fn from_csv_reader<R: std::io::Read>(reader: R) -> Ledger {
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .has_headers(true)
            .trim(csv::Trim::All)
            .from_reader(reader);

        let mut ledger = Ledger::default();

        for line in reader.deserialize::<Record>() {
            let record = match line {
                Ok(record) => record,
                Err(err) => {
                    eprintln!("invalid line in CSV: {}", err.to_string());
                    continue;
                }
            };
            let (account, transaction) = match record_to_transaction(&record) {
                Ok((account, transaction)) => (account, transaction),
                Err(err) => {
                    eprintln!("invalid record encountered {}", err);
                    continue;
                }
            };

            if let Err(e) = ledger.apply_for_account(account, transaction) {
                eprintln!("{}", e);
            }
        }

        ledger
    }
}

#[derive(Default)]
pub struct ProcessedTxs(HashMap<(AccountId, TransactionId), ProcessedTransaction>);

// ProcessedTxsForAccount is a reference into all processed transactions,
// with the added restriction that it only allows lookups and insertions
// for the specified account number.
pub struct ProcessedTxsForAccount<'a> {
    // `processed` is a reference to all processed transactions.
    processed: &'a mut ProcessedTxs,
    // Only transactions belonging to this account may be accessed through
    // this struct.
    account: AccountId,
}

impl<'a> ProcessedTxsForAccount<'a> {
    pub(crate) fn for_account(
        processed: &'a mut ProcessedTxs,
        id: AccountId,
    ) -> ProcessedTxsForAccount {
        ProcessedTxsForAccount {
            processed: processed,
            account: id,
        }
    }

    // Find a transaction by transaction ID. If the given transaction ID does
    // not belong to the account associated with this object then it won't be
    // returned.
    pub fn find<'b>(self: &'b mut Self, tx: TransactionId) -> Option<&'b mut ProcessedTransaction> {
        self.processed.0.get_mut(&(self.account, tx))
    }

    // Insert a new transaction as processed and associate it with the account
    // referenced by this object.
    pub fn insert_processed(self: &mut Self, id: TransactionId, tx: ProcessedTransaction) {
        self.processed.0.insert((self.account, id), tx);
    }
}

// NOTE: Due to the CSV crate's shortcomings the records can't
// be directly deserialized as an enum. Therefore they're
// first read as a simple record type then transformed into
// an enum.
// https://github.com/BurntSushi/rust-csv/issues/211
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
struct Record {
    #[serde(rename = "type")]
    record_type: RecordType,
    client: AccountId,
    tx: TransactionId,
    amount: Option<TransactionAmount>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecordType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

#[derive(Error, Debug, PartialEq, Eq)]
enum RecordError {
    #[error("The amount is missing for a transaction type that requires it")]
    MissingAmount,
}

fn record_to_transaction(record: &Record) -> Result<(AccountId, Transaction), RecordError> {
    use RecordError::*;
    use Transaction::*;

    let tx = match record.record_type {
        RecordType::Deposit => record
            .amount
            .map(|amount| Deposit {
                new_id: record.tx,
                amount: amount,
            })
            .ok_or(MissingAmount),
        RecordType::Withdrawal => record
            .amount
            .map(|amount| Withdrawal {
                new_id: record.tx,
                amount: amount,
            })
            .ok_or(MissingAmount),
        RecordType::Dispute => Ok(Dispute { id: record.tx }),
        RecordType::Resolve => Ok(Resolve { id: record.tx }),
        RecordType::Chargeback => Ok(Chargeback { id: record.tx }),
    };

    tx.map(|tx| (record.client, tx))
}

#[cfg(test)]
mod tests {
    use super::Ledger;
    use crate::{account::Account, Transaction};

    #[test]
    fn record_to_transaction() {
        use super::RecordError;
        use super::RecordType::*;
        use super::{record_to_transaction as f, Record};

        let tests = [
            // Withdrawals
            (
                Record {
                    record_type: Withdrawal,
                    client: 1,
                    tx: 2,
                    amount: Some(10.into()),
                },
                Ok((
                    1,
                    Transaction::Withdrawal {
                        new_id: 2,
                        amount: 10.into(),
                    },
                )),
            ),
            (
                Record {
                    record_type: Withdrawal,
                    client: 16,
                    tx: 32,
                    amount: None,
                },
                Err(RecordError::MissingAmount),
            ),
            // Deposits
            (
                Record {
                    record_type: Deposit,
                    client: 5,
                    tx: 4,
                    amount: Some(90.into()),
                },
                Ok((
                    5,
                    Transaction::Deposit {
                        new_id: 4,
                        amount: 90.into(),
                    },
                )),
            ),
            (
                Record {
                    record_type: Deposit,
                    client: 7,
                    tx: 6,
                    amount: None,
                },
                Err(RecordError::MissingAmount),
            ),
            // Disputes
            (
                Record {
                    record_type: Dispute,
                    client: 7,
                    tx: 6,
                    amount: None,
                },
                Ok((7, Transaction::Dispute { id: 6 })),
            ),
            (
                Record {
                    record_type: Dispute,
                    client: 7,
                    tx: 6,
                    // Amount on a dispute is ok, it's simply ignored
                    amount: Some(10.into()),
                },
                Ok((7, Transaction::Dispute { id: 6 })),
            ),
            // Resolve
            (
                Record {
                    record_type: Resolve,
                    client: 5,
                    tx: 2,
                    amount: None,
                },
                Ok((5, Transaction::Resolve { id: 2 })),
            ),
            (
                Record {
                    record_type: Resolve,
                    client: 2,
                    tx: 5,
                    // Amount on a resolve is ok, it's simply ignored
                    amount: Some(10.into()),
                },
                Ok((2, Transaction::Resolve { id: 5 })),
            ),
            // Chargeback
            (
                Record {
                    record_type: Chargeback,
                    client: 5,
                    tx: 2,
                    amount: None,
                },
                Ok((5, Transaction::Chargeback { id: 2 })),
            ),
            (
                Record {
                    record_type: Chargeback,
                    client: 2,
                    tx: 5,
                    // Amount on a resolve is ok, it's simply ignored
                    amount: Some(10.into()),
                },
                Ok((2, Transaction::Chargeback { id: 5 })),
            ),
        ];

        for (left, right) in tests.into_iter() {
            assert_eq!(f(&left), right);
        }
    }

    #[test]
    fn header_ordering_is_permissive() {
        let input = "\
client,amount,type,tx
5,10,deposit,1
";

        let ledger = Ledger::from_csv_reader(input.as_bytes());
        assert_eq!(ledger.accounts.len(), 1);
        assert!(ledger.accounts.contains_key(&5));
    }

    #[test]
    fn bad_records_are_ignored() {
        let input = "\
type,client,tx,amount
deposit,1,1,10
foo,1,2,10
withdraw,1,3,
dispute,1,,
";

        let ledger = Ledger::from_csv_reader(input.as_bytes());
        assert_eq!(ledger.accounts.len(), 1);
        assert_eq!(
            ledger.accounts.get(&1).map(Account::available),
            Some(10.into())
        );
    }

    #[test]
    fn csv_output() {
        let input = "\
type,client,tx,amount
deposit,1,1,10
withdrawal,1,2,4
dispute,1,2,
deposit,2,3,15
withdrawal,2,4,10
dispute,2,4,
chargeback,2,4,
";

        let ledger = Ledger::from_csv_reader(input.as_bytes());
        let mut output = vec![];
        ledger.accounts_to_csv(&mut output);
        let output = String::from_utf8(output).expect("output should be UTF8");
        assert_eq!(
            output,
            "\
client,available,held,total,locked
1,2.0000,4.0000,6.0000,false
2,-5.0000,0.0000,-5.0000,true
"
        );
    }
}
