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

## Shortcuts taken
I took some shortcuts that made writing this small project a bit quicker
that I'd likely to differently in a real-world scenario. Some examples in
no particular order:
* Some of the types (Balance, TXID, etc.) are only type aliases, in real
  code they'd fare better as newtypes for proper type safety. However,
  this way I could just type bare integers in many places for simplicity.
* As mentioned before, account and client are used interchangably.
* The CSV output is sorted by client ID to make testing the CSV output
  easier. Otherwise I'd have to sort the output afterwards or make the
  test independent of the order somehow. This might add some non-trivial
  overhead on large datasets.
* Most of the code isn't written with concurrency in mind, although
  adapting many parts shouldn't be too hard thanks to the architecture.
  Most importantly the Ledger expects to be initialized from a single
  reader. Also, all past transactions are stored in a single hashmap
  that can only be accessed by one thing at a time. However, sharding
  this on the client ID would be a good approach for making it more
  concurrency-friendly, for example.

## Personal preferences
Code is formatted using `cargo fmt`. I do not necessarily agree with all the
style choices rustfmt makes but I value consistency over my tastes.