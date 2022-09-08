use account::Account;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, error::Error};

mod account;

type TransactionId = u32;
type AccountId = u16;
type Balance = Decimal;
type TransactionAmount = Decimal;

// NOTE: Due to the CSV crate's shortcomings the records can't
// be directly deserialized as an enum.
//
// https://github.com/BurntSushi/rust-csv/issues/211
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct Record {
    #[serde(rename = "type")]
    record_type: RecordType,
    client: AccountId,
    tx: TransactionId,
    amount: Option<TransactionAmount>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RecordType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

pub enum Transaction {
    Deposit { amount: TransactionAmount },
    Withdrawal { amount: TransactionAmount },
    Dispute { id: TransactionId },
    Resolve { id: TransactionId },
    Chargeback { id: TransactionId },
}

fn main() -> Result<(), Box<dyn Error>> {
    // Try to initialize a CSV reader based on the filename passed as a command line argument
    let mut csv_reader = std::env::args()
        // The 0th argument is the program name
        .nth(1)
        // Error out if no filename is given
        .ok_or("no filename given")
        // If filename is OK, try to open it with a CSV reader
        .map(|path| {
            // Make it permissive
            csv::ReaderBuilder::new()
                .flexible(true)
                .has_headers(true)
                .trim(csv::Trim::All)
                .from_path(path)
        })??;

    let mut ledger: HashMap<AccountId, Account> = HashMap::new();

    for line in csv_reader.deserialize() {
        let record: Record = if let Ok(record) = line {
            record
        } else {
            eprintln!("invalid record encountered");
            continue;
        };

        let transaction = match record.record_type {
            RecordType::Deposit => {
                let amount = if let Some(amount) = record.amount {
                    amount
                } else {
                    eprintln!("deposit record type missing amount");
                    continue;
                };
                Transaction::Deposit { amount }
            }
            RecordType::Withdrawal => {
                let amount = if let Some(amount) = record.amount {
                    amount
                } else {
                    eprintln!("withdrawal record type missing amount");
                    continue;
                };
                Transaction::Withdrawal { amount }
            }
            RecordType::Dispute => Transaction::Dispute { id: record.tx },
            RecordType::Resolve => Transaction::Resolve { id: record.tx },
            RecordType::Chargeback => Transaction::Chargeback { id: record.tx },
        };

        let account = ledger.entry(record.client).or_default();
        if let Err(e) = account.try_apply_transaction(record.tx, transaction) {
            eprintln!("{}", e);
        }
    }

    let mut writer = csv::Writer::from_writer(std::io::stdout());

    #[derive(Serialize)]
    struct OutputRecord {
        client: AccountId,
        available: Balance,
        held: Balance,
        total: Balance,
        locked: bool,
    }

    for (account_id, account) in ledger {
        let _ = writer.serialize(OutputRecord {
            client: account_id,
            available: account.available(),
            held: account.held(),
            total: account.total(),
            locked: account.is_frozen(),
        });
    }

    Ok(())
}
