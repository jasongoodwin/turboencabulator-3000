extern crate clap;

use std::time::Instant;

use clap::Parser;
use csv::ReaderBuilder;
use csv::Trim::All;
use tokio::sync::mpsc;

use client_accounts::ClientAccounts;

mod client_accounts;
mod transaction;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(value_parser)]
    transactions_file: String,

    #[clap(short, parse(from_flag))]
    debug: bool,
}

#[tokio::main]
async fn main() {
    let now = Instant::now(); // used to present total runtime.

    let args = Args::parse();
    let file_path = args.transactions_file;
    let debug = args.debug;

    if debug {
        println!("\nStarting...");
        println!("\tInput file: {}", file_path);
        println!("\tResult:\n");
    }

    // mpsc is used only to demonstrate how we might build on this to accept streams through other sources.
    // There is some back pressure to ensure stability. Something like Kafka would help produce
    // a more robust implementation than eg http endpoints...
    // The csv parsing is delegated to another thread which will stream the transaction records back to this main thread.
    let (tx, mut rx) = mpsc::channel(2048);

    // This would be, for example, a kafka consumer reading sets of transactions from a topic.
    // Any multiplexing would require some work to
    // ensure only one set of transactions processed at a time as transactions are ordered.
    tokio::spawn(async move {
        let mut rdr = ReaderBuilder::new()
            .trim(All) // ensures whitespace ignored.
            .from_path(file_path)
            .unwrap(); // Fails thread on missing file.

        for result in rdr.deserialize() {
            // ignores any records that fail.
            if let Ok(record) = result {
                let r = tx.send(record).await;
                if r.is_err() {
                    println!("issue transmitting... {:?}", r)
                }
            } else {
                println!("couldn't deserialize {:?}", result);
            }
        }
    });

    let mut clients = ClientAccounts::new();

    while let Some(message) = rx.recv().await {
        clients.update(message).unwrap(); // Note: fails main thread on unknown transaction type.
    }

    let csv_res = clients.write_csv(Box::new(std::io::stdout()));

    if debug || csv_res.is_err() {
        let elapsed = now.elapsed();
        println!("\nCompleted run.");
        println!("\tResult: {:?}", csv_res);
        println!("\tTook: {:.2?}", elapsed);
    }
}
