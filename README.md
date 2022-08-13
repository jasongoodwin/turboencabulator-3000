# Turbo-encabulator 3000

The Turbo-encabulator 3000 presents an obfuscated project name on github. :)
No longer simply widgets and doo-dads, the turbo-encabulator has been built to ingest transaction data and write out account state.

## CLI features

CLI has a help flag that can be invoked to see usage:
`cargo run -- --help`

## Running

A CLI has been produced which is used as follows:
`cargo run -- txs1.csv txs2.csv`
One or more sets of transactions can be provided. They will be processed in order.

Once complete, the application will print CSV to STDOUT representing account state after completing.

## Testing

To run the test suite, run the following:
`cargo test`

## Debugging

Debug output can be enabled with the `-d` flad:
`cargo run -- input.csv -d`
This will give some information on time taken.

# Design + Behavior

Held funds are calculated based on the currently disputed transactions. 
It doesn't make sense to hold funds for a disputed withdraw - the held funds are only for disputed deposits.
Can't dispute a chargeback as there isn't any transaction id for it so the history is only withdrawal and deposit.

Duplicate transactions are ignored.

## Performance 
It takes about 1s per 10 megs of csv on a 2020 macbook pro (intel.)
No cloning is used - ownership is given of all data as it's moved.
It's able to utilize ~1.5 CPUs w/ one thread producing from the CSV and another thread consuming.
It uses a bounded mpsc channel to ensure backpressure so that memory utilization stays reasonable.

A 780M file was consumed to ensure memory safety. 
~1M of memory is used by the application in processing that file.

## Concurrent Map
Rather than a full concurrent-hashmap implementation such as DashMap or Flurry, or using a young library, I decided to implement a simple concurrent hashmap solution.
Sharded was chosen as an extremely simple mechanism to shard locks rather than implementing myself.
The code would be easy to fork and maintain compared to full-on concurrent hashmap implementations.


The internals assume that massive ammounts of data may need to be processed with minimal resource consumption.

The input is assumed ordered, and it is processed in an execution context in parallel as fast as the data can be streamed to the consumer.

To ensure correctness, each account has a lock, but a lock-free datastructure has been used to store the actual accounts. One set of transactions is processed at a time, although multiple sets of transactions can be provided. The CLI can accept multiple sets of transactions to demonstrate this. It's assumed that each set of transactions needs to be processed in entirety so no more than one set of transactions is processed concurrently.

There are a few major components:

- CLI - takes the input, streams the data into the processing enginer. Renders Accounts state back to CSV.
- Accounts - accepts transactions async, updates accounts. Can provide state to a consumer.
- Persistence - account information can be persisted through runs (optionally). The application memory is the source of truth, redis is used only for faster recovery. It would be possible to use Kafka with compaction for this use case as well. This is done once at the end of the processing of a set of transactions atomically to ensure the update is either done in entirety or not. Coupled with Kafka for example, this would ensure the consumer can safely recover from a crash or restart. 

The idea behind the design is that we may want to remove the CLI and have multiple producers that are putting the transactions somewhere (eg kafka), and we may be able to process them very quickly if we can continuously stream into the accounts from the source. There is some fan out from the input so it allows parallelization of the processing.

Tokio is used as the runtime.
To improve performance, accounts are written to redis using protocol buffers+compression.

# Alternatives Considered

Rather than persisting to redis, it would be possible to use Kafka with topic compaction in this use case. Each account could be compacted, and entire topic could be re-read on startup to re-acquire the total state of all accounts quickly. 

https://developer.confluent.io/learn-kafka/architecture/compaction/#:~:text=Topic%20Compaction%20Guarantees,value%20for%20a%20given%20key.

If kafka was used to consume and then produce the current account states back into kafka after each message processed, the movement of the consumer offset and updated messages would need to be written back with transactional semantics.

Beyond considering a single consumer, it becomes easy to understand how to horizontally scale utilizing consumer groups that are sharded on account id. Changes to the consumer group (adding/removing nodes) would need to be handled carefully as the accounts would need to be re-read on redistribution of partitions.
