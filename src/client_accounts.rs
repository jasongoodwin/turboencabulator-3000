use serde::{Serialize, Deserializer, Serializer};

extern crate clap;

use clap::Parser;
use serde::Deserialize;
use std::time::Instant;
use std::error::Error;
use csv::Writer;
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use crate::transaction::{Transaction, TransactionType};
use serde::ser::{SerializeMap, SerializeSeq};

static DECIMAL_PRECISION: u32 = 4;

fn round_serialize<S>(x: &f32, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
{
    s.serialize_f32(x.round())
}

#[derive(Debug)]
/// Struct representing current state of client account.
///
/// `id`: unique client id
/// `total`: the current value of the account
/// `locked`: if the account had a charge back, it will be marked locked.
/// `transaction_history`: a collection of all successfully applied transactions.
/// `disputed`: a set of disputed transaction ids in `transaction_history`.
///
/// `held()`: sum of disputed transactions.
/// `available()`: total funds less held funds.
struct ClientAccount {
    id: u16,
    total: f32,
    locked: bool,
    transaction_history: HashMap<u32, Transaction>,
    disputed: HashSet<u32>,
}

impl ClientAccount {
    /// returns the total disputed funds (deposits only! withdrawals are ignored)
    fn held(&self) -> f32 {
        let mut held: f32 = 0.0;

        for txid in self.disputed.iter() {
            match self.transaction_history.get(txid) {
                Some(hist) if hist.typ == TransactionType::Deposit => {
                    held = held + hist.amount.unwrap()
                },
                _ => {}
            }
        }

        held
    }

    fn available(&self) -> f32 {
        f32::min(self.total - self.held(), 0.0)
    }
}

/// round will round an f32 to the DECIMAL_PRECISION
fn round(x: f32) -> f32 {
    let y = 10i32.pow(DECIMAL_PRECISION) as f32;
    (x * y).round() / y
}

/// Serialize will add an available field and will not serialize the transaction_history.
impl Serialize for ClientAccount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(5))?;
        seq.serialize_element(&self.id)?;
        seq.serialize_element(&self.available())?;
        seq.serialize_element(&self.held())?;
        seq.serialize_element(&self.total)?;
        seq.serialize_element(&self.locked)?;
        seq.end()
    }
}

impl ClientAccount {
    fn new(id: u16) -> ClientAccount {
        ClientAccount { id, disputed: Default::default(), total: 0.0, locked: false, transaction_history: Default::default() }
    }

    fn update(&mut self, tx: Transaction) {
        match tx.typ {
            TransactionType::Deposit if !self.transaction_history.contains_key(&tx.tx) => {
                self.total = self.total + tx.amount.unwrap();
                println!("deposit. tx history size: {:?}", self.transaction_history.len());
                self.transaction_history.insert(tx.tx, tx);
            }

            TransactionType::Withdrawal if !self.transaction_history.contains_key(&tx.tx) => {
                if self.available() - tx.amount.unwrap() >= 0.0 {
                    println!("can't withdraw");
                    self.total = self.total - tx.amount.unwrap();
                    self.transaction_history.insert(tx.tx, tx);
                }
            }

            TransactionType::Dispute => {
                // look for a transaction that was applied.
                if let Some(history) = self.transaction_history.get(&tx.tx) {
                    self.disputed.insert(tx.tx);
                }
            }

            TransactionType::Resolve => {
                self.disputed.remove(&tx.tx);
            }

            TransactionType::Chargeback if self.disputed.contains(&tx.tx) => {
                if let Some(history) = self.transaction_history.get(&tx.tx) {
                    self.disputed.remove(&tx.tx);
                    self.locked = true;

                    match history.typ {
                        TransactionType::Deposit => self.total = self.total - history.amount.unwrap(),
                        TransactionType::Withdrawal => self.total = self.total + history.amount.unwrap(), // TODO do we want to debit these?
                        _ => () // shouldn't happen.
                    }
                }
            }
            _ => (), // any unknown type, or undisputed resolve or chargeback.
        };
    }
}

#[derive(Debug)]
pub struct ClientAccounts {
    map: HashMap<u16, ClientAccount>
}

impl ClientAccounts {
    pub fn new() -> ClientAccounts {
        ClientAccounts { map: HashMap::new() }
    }

    // TODO no failures
    pub fn update(&mut self, tx: Transaction) -> Result<(), Box<dyn Error>> {
        match self.map.get_mut(&tx.client) {
            None => {
                let mut acct = ClientAccount::new(tx.client);
                acct.update(tx);
                self.map.insert(acct.id, acct);
            }
            Some(acct) => {
                acct.update(tx);
            }
        }

        Ok(())
    }

    // Will write the current state of all accounts to specified Writer.
    // Will fail and return error if one is encountered.
    pub fn write_csv<T: std::io::Write>(self, writer: T) -> Result<(), Box<dyn Error>> {
        let mut wtr: Writer<T> = csv::Writer::from_writer(writer);
        println!("writing csv now...");
        // write header
        wtr.write_record(&["id", "available", "held", "total", "locked"])?;

        // then write each record
        for (_, v) in self.map.into_iter() {
            println!("tx history size: {}", v.transaction_history.len());
            wtr.serialize(v)?;
        }

        wtr.flush()?;
        Ok(())
    }
}
