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
Alternative to invoking `cargo`, you can use `make`. 
Have a look at the Makefile to see the targets. 

eg:
`make test`

## Test Data
There are files in the root that test a specific cases but primarily the unit tests will cover correctness.

To allow massive streams of data to be tested, a `generate_test_data.sh` file is included that will produce
massive files for testing large sets of deposits _only_.

# Design Analysis and Discussion

## High Level Design and Performance Characteristics
This section contains a few notes to guide you.
The next section contains more granular detail.

f64 is used for the transaction history, and `Decimal` (129bit) is used in the client.
No held or available data is held, but disputes are held, and the available/held fields are calculated.

Space complexity ends up being a bit heavy - with no transaction history stored, the app only takes a meg of memory, 
but the transaction history is held until termination so grows linearly.
Using sstables would be my major next step to eliminate memory use.
I've run it against massive files - the only thing that takes any memory is the transaction history.

Disputes simply reference records in the transaction history.
It keeps it slightly leaner.

Data is streamed between a producer thread reading the file with backpressure into the consumer that keeps the client accounts.
Tests describe anything else. 

There are some assumptions like disputes on withdrawals should not credit nor debit.
Any other behaviour seems odd - you shouldn't hold asset already withdrawn, nor should you make it available.
Only debits affect available funds. If you think I'm wrong please tell me.

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

Client accounts use Decimal without rounding to represent amount.
This is a large attribute at 125 bits.
Transaction history stores only the ammount and keeps it in mem as f64.
Calculations convert from f64 to Decimal and imprecision is discarded at that point.
This has been tested thoroughly - it offers a good balance between space and time complexity while promising correctness.

It's assumed that we may represent something like shib so I switched transactions to keep f64 instead of f32 last minute.
I tested storing floats in the client account but couldn't stop float imprecision from causing calculation issues (see unit testing.)

## Held/available funds
No notion of held or available funds exists in the data modelled.
Held funds are calculated based on the currently disputed transactions. 
It doesn't make sense to hold funds for a disputed withdraw (which is a credit) - the held funds are only for disputed deposits.
There is some cost to this, but it's marginal. Space is preferred as space complexity is relative to input.
Everything is in memory - we can spend a couple cycles to compute in effectively `O(n)` where n is the number of open disputes.
It's assumed that disputes would be rare and a client may only have one or two open.

## Duplicated Transactions
Duplicate transactions are ignored.
If we've seen it already, we won't re-calculate.
This isn't true for disputes/resolutions/chargebacks as they don't have their own transaction id.

## Performance Analysis

### Performance Summary:
It takes about 1s per meg of csv w/ 1 transaction per client on a 2020 macbook pro (intel.) Not excellent but good enough.
Memory utilizes a bit more w/ 2-3mb per 1mb of input. This is solely for the transaction history and has been confirmed in testing.
It would be possible to optimize space complexity - I just ensure that there is no leaking at this point.

No cloning is used - ownership is given of all data which prevents excessive allocation. (It's possible there are some copies on primitives tho - map keys for example are referenced on Copy primatives.)
In testing, the application can utilize ~1.5 CPUs w/ the bottleneck being the consumer side so the csv reader will keep the buffer full. 
Sharding would yield better performance by allocating a portion of each of the clients to a thread.
It uses a bounded mpsc channel to ensure backpressure so that memory utilization stays reasonable when provided large files.

A 1GB file was generated with a series of unique deposits across 100 clients. No duplicates - each is stored in memory.
It takes about 900s to run through 1GB w/ 2.3GB of memory allocated by the application. 
Only ~1.1MB is allocated by the application for the file when the `transaction_history` is not stored in memory.
This demonstrates memory consumption is almost entirely due to the storage of the complete `transaction_history` in memory.

### Recommended Space Complexity Optimizations
Transaction history could be stored to disk instead of memory to reduce consumption. 
Something like sqllite, leveldb, or especially, implementing a couple sstable "levels" ourselves could be used to move the history to disk
with reasonable read/write characteristics.
I tried to lean the transaction history a bit by using the `TransactionHistoryRecord` instead of the entire `Transaction` but it doesn't save much.
I would undo that probably for the sake of simplicity.

## Why Tokio?
Tokio is absolutely not needed here - streaming utilizing only one core and no async would be simpler.
But it demonstrates my experience and does offer some concurrency (~1.5x).
Could pin a couple threads and it'd remove the big dependency tree from tokio even.
This is only here for demonstration. Even the `mpsc` usage is overkill imo.
Async/Concurrent/Distributed and fast, safe stable systems are my jam. I've written books on this stuff!
But simplicity rules, and premature optimization is the devil.
So an actual submission would be as simple as possible and I would avoid MPSC here for the sake of juniors.