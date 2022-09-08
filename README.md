# Ledger

Simple ledger application.

## Assumptions

* All the details in the instructions hold true, e.g. transaction IDs never
  reappear.  If they do then changes to disputed transactions will cause
  incorrect updates of the account.
* Since each client may only have only one account the terms Account and
  Client are used interchangably.
* A frozen account may not be deposited to or withdrawn from, but disputes,
  resolutions, and chargebacks can, as these are not considered customer
  actions the "bank" has control over; they are assumed to come from an
  external party. However, changing this behavior is trivial.
* Disputes can bring the available balance of an account into the negatives.
* Both a deposit and a withdrawal can be disputed, and they have the same
  effect on the account, meaning in both cases the available funds are
  decreased by the disputed amount and the held funds are increased by the
  same amount.
* The input CSV has headers.

## Performance
The program tries to be efficient by simply opening a file handle and passing
that to `rust-csv`'s reader implementation, relying on it to correctly buffer
the file contents as needed instead of reading the entire dataset into memory.
However, in order to handle disputes the entire transaction history has to be
stored in memory. Depending on the dataset this might be more or less
efficient than simply storing the textual representation in memory.

An alternative I considered was simply re-scanning the CSV every time a past
transaction is referenced. This would be more memory efficient, but a lot less
elegant and complicated for a toy exercise.

## Personal preferences
Code is formatted using `cargo fmt`. I do not necessarily agree with all the
style choices rustfmt makes but I value consistency over my tastes.