# Turboencabulator 3000
The Turboencabulator 3000 presents an obfuscated project name on github. :)
No longer simply widgets and doo-dads, the Turboencabulator 3000 has been designed to ingest transaction data and write out account state.

I had fun writing this and did probably too much analysis of its behavior.

## CLI help
CLI has a help flag that can be invoked to see usage:
`cargo run -- --help`

## Running
A CLI has been produced which is used as follows:
`cargo run -- txs1.csv txs2.csv`
One or more sets of transactions can be provided. They will be processed in order.

Once complete, the application will print CSV to STDOUT representing account state after completing.

## Debugging
Some additional output such as run time can be printed by passing the `-d` flag:
`cargo run -- input.csv -d`

## Unit Test
To run the test suite, run the following:
`cargo test`

## Make
Alternative to invoking eg cargo, you can use make. 
Have a look at the Makefile to see the targets. 

## Commited Test Data
There are files in the root that test a few specific cases.

## Generating Test Data
To allow massive streams of data to be tested, a `generate_test_data.sh` file is included that will produce
massive files for testing large sets of deposits _only_.

# Design Analysis and Discussion

## General Notes
It's maybe a little over-engineered but I had fun.
Most of the project is testing. I tested it fairly well.

I made the decision to keep the transaction records as floats, but the clients as Decimal.
No rounding is done, but the Decimal conversions will cleave w/o `retain` so it all works out without explicit rounding.

Using sstables would probably be my major next step to eliminate memory use.
Space complexity apx O(n) where n is number of debit/withdrawal transactions in input.
I've run it against massive files - the only thing that takes any memory is the transaction history.

Disputes simply reference records in the transaction history.
The clients themselves actually don't store available/held but only disputes.
Everything is calculated at some expense of time but reduced space.

## Assumptions
There were some assumptions made as I had no-one to ask.
The dispute workflow doesn't clarify deposits vs withdrawals - I assume that withdrawal disputes should't 
GIVE assets back to the user, nor hold them.
I still allow chargebacks on them, so I store them in memory.

I assume deposits and withdrawals always have an `amount`.
Otherwise, records ignore amount. The amount is always unwrapped so will cause death if missing.
Thread will die if this requirement is not met at the moment.

## Abstract
The general idea is to read potentially massive files off of disk as a stream and keep memory usage small.
If no transaction history is stored in memory, the application memory won't (1.1MB for a 1TB file in testing.)
Transaction history will consume memory tho...

A `producer` reads line by line and signals over mpsc channel to a `consumer` that will process serially. A buffer is used to ensure that memory
is not consumed un-necessarily. This provides backpressure and stability. 
The consumers could be sharded on client account to allow for better throughput as it's the bottleneck.

A thread is spawned as the `producer` reading csv, and the main thread acts as the `consumer` in the mpsc channel.
The receiver side will call until the sender has gone out of scope, and then will continue to print the csv to STDOUT.
Errors in the CSV reader will panic the thread and it could be a bit more elegant in reporting errors.

# Model
Internally, the ClientAccounts are modelled, and each client has its own struct with a transaction history and 
list of open debated transactions.

As mentioned, there is no tracking of available or held funds, but they are instead calculated on each transaction.
Because we need to track the debated transactions, it's much simpler to just store debates and calculate the available and held funds.
It's effectively linear time on the number of open debated transactions - that's preferred as space can get big.

## Model Precision Issues

f32 is used to represent amount assuming it's eg USD not SHIB. This is likely a bad assumption.
This is the biggest area of concern - if shib is $0.000016CAD today it's too easily to hold an f32 worth...
f32 holds most of the crypto market cap in USD tho.
In production, I'd probably use `Decimal` not `f32` but it's 129 bits.
I chose to start with `f32` and implementing a rounding to prevent any accumulating imprecision.

## Held/available funds
Held funds are calculated based on the currently disputed transactions. 
It doesn't make sense to hold funds for a disputed withdraw (which is a credit) - the held funds are only for disputed deposits.

Held funds and available funds are not stored. Makes no sense - we need to hold the open disputes.
All we need to do is add an open dispute, and then calculate based on open disputes.
Everything is in memory - we can spend a couple cycles to compute in effectively `O(n)` where n is the number of open disputes.

## Duplicated Transactions
Duplicate transactions are ignored.
If we've seen it already, we won't re-calculate.

Transaction id is unique - we shouldn't process the same debit/credit twice if it happens to appear.

## Performance Analysis

### Performance Summary:
It takes about 1s per meg of csv w/ 1 transaction per client on a 2020 macbook pro (intel.) Not excellent but good enough.
Memory utilizes a bit more w/ 3mb per 1mb of input apx solely for the transaction history.
There is likely some optimization possible here.

No cloning is used - ownership is given of all data as it's moved. (It's possible there are some copies on primitives tho eg in map keys.)
In testing, the application can utilize ~1.5 CPUs w/ the bottleneck being the consumer side. Sharding would yield better speed.
It uses a bounded mpsc channel to ensure backpressure so that memory utilization stays reasonable when provided large files.

A 1GB file was generated with a series of unique deposits across 100 clients. No duplicates - each is stored in memory.
It takes about 900s to run through 1GB w/ 2.3GB of memory allocated by the application. 
Only ~1.1MB is allocated by the application for the file when the `transaction_history` is not stored in memory.
This demonstrates memory consumption is almost entirely due to the storage of the complete `transaction_history` in memory.

### Recommended Space Complexity Optimizations
Transaction history could be stored to disk instead of memory to reduce consumption. 
Something like sqllite, leveldb, or even implementing a couple sstable "levels" ourselves could be used to move the history to disk
with reasonable read/write characteristics.
I tried to lean the transaction history a bit by using the `TransactionHistoryRecord` instead of the entire `Transaction` but it doesn't save much.

## Why Tokio?
Tokio is absolutely not needed here, but it demonstrates my experience and does offer some concurrency (~1.5x).
Could pin a couple threads and it'd remove the big dependency tree.
This is only here for demonstration. Even the `mpsc` usage is overkill imo.
Async/Concurrent/Distributed and fast, safe stable systems are my jam. I've written books on this stuff!

Not much other premature optimization done - it's enough for today and has been validated reasonably without more info.
