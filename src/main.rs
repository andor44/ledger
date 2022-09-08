use rust_decimal::Decimal;
use std::error::Error;
use thiserror::Error;

mod account;
mod ledger;

// Define some types used across the entire program
type TransactionId = u32;
type AccountId = u16;
type Balance = Decimal;
type TransactionAmount = Decimal;

#[derive(Debug, PartialEq, Eq)]
pub enum Transaction {
    Deposit {
        new_id: TransactionId,
        amount: TransactionAmount,
    },
    Withdrawal {
        new_id: TransactionId,
        amount: TransactionAmount,
    },
    Dispute {
        id: TransactionId,
    },
    Resolve {
        id: TransactionId,
    },
    Chargeback {
        id: TransactionId,
    },
}

#[derive(Error, PartialEq, Eq, Debug)]
pub enum TransactionError {
    #[error("The account is frozen")]
    AccountFrozen,
    #[error("Insufficient funds to withdraw requested amount")]
    InsufficientFunds,
    #[error("Attempted dispute, resolution, or chargeback of a transaction that doesn't exist")]
    NonexistentTransaction,
    #[error("The transaction that was attempted to dispute is not currently settled")]
    NotSettled,
    #[error("The transaction that was attempted to resolve is not under dispute")]
    NotDisputed,
}

fn main() -> Result<(), Box<dyn Error>> {
    // Attempt to open the file passed on the command line.
    let file = std::env::args()
        // The 0th argument is the program name, the 1st should be the filename.
        .nth(1)
        // Error out if no filename is given
        .ok_or("no filename given")
        // If filename was given attempt to open it as a File.
        .map(std::fs::File::open)??;

    let ledger = ledger::Ledger::from_csv_reader(file);

    let mut stdout = std::io::stdout();
    ledger.accounts_to_csv(&mut stdout);

    Ok(())
}
