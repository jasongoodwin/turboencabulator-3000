extern crate clap;

use std::collections::{HashMap, HashSet};
use std::error::Error;

use csv::Writer;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

use crate::transaction::{Transaction, TransactionHistoryRecord, TransactionType};

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
    total: Decimal, // 129 bit. tested w/ floats but floating point imprecision appears
    locked: bool,
    transaction_history: HashMap<u32, TransactionHistoryRecord>,
    disputed: HashSet<u32>,
}

impl ClientAccount {
    /// returns the total disputed funds (deposits only! withdrawals are ignored)
    fn held(&self) -> Decimal {
        let mut held: Decimal = dec!(0.0);

        for txid in self.disputed.iter() {
            match self.transaction_history.get(txid) {
                Some(hist) if hist.typ == TransactionType::Deposit => {
                    held += Decimal::from_f64(hist.amount).unwrap()
                }
                _ => {}
            }
        }

        held
    }

    /// available returns a positive value if funds are available.
    /// It's calculated based on total funds less all disputed funds.
    fn available(&self) -> Decimal {
        let res = self.total - self.held();
        res.max(dec!(0.0))
    }
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
        ClientAccount {
            id,
            disputed: Default::default(),
            total: dec!(0.0),
            locked: false,
            transaction_history: Default::default(),
        }
    }

    fn update(&mut self, tx: Transaction) {
        match tx.typ {
            TransactionType::Deposit
                if !self.transaction_history.contains_key(&tx.tx) && tx.amount.is_some() =>
            {
                self.total += Decimal::from_f64(tx.amount.unwrap()).unwrap();
                self.transaction_history.insert(
                    tx.tx,
                    TransactionHistoryRecord {
                        typ: tx.typ,
                        amount: tx.amount.unwrap(),
                    },
                );
            }

            TransactionType::Withdrawal
                if !self.transaction_history.contains_key(&tx.tx) && tx.amount.is_some() =>
            {
                let tx_amount = Decimal::from_f64(tx.amount.unwrap()).unwrap();
                if self.available() - tx_amount >= dec!(0.0) {
                    self.total -= tx_amount;
                    self.transaction_history.insert(
                        tx.tx,
                        TransactionHistoryRecord {
                            typ: tx.typ,
                            amount: tx.amount.unwrap(),
                        },
                    );
                } else {
                    self.transaction_history.insert(
                        tx.tx,
                        TransactionHistoryRecord {
                            typ: TransactionType::FailedWithdrawal,
                            amount: tx.amount.unwrap(),
                        },
                    );
                }
            }

            TransactionType::Dispute => {
                // look for a transaction that was applied. If it exists then insert as disputed.
                if self.transaction_history.get(&tx.tx).is_some() {
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
                        TransactionType::Deposit => {
                            self.total -= Decimal::from_f64(history.amount).unwrap()
                        }
                        TransactionType::Withdrawal => {
                            self.total += Decimal::from_f64(history.amount).unwrap()
                        } // TODO do we actually want to debit these?
                        _ => (), // shouldn't happen.
                    }
                }
            }
            _ => (), // any unknown type, or undisputed resolve or chargeback.
        };
    }
}

#[derive(Debug)]
pub struct ClientAccounts {
    map: HashMap<u16, ClientAccount>,
}

impl ClientAccounts {
    pub fn new() -> ClientAccounts {
        ClientAccounts {
            map: HashMap::new(),
        }
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
    // I chose to not round here as the input is expected to be 4 digit precision -
    // The conversion to decimal should keep the values as 4 digit decimal precision.
    pub fn write_csv<T: std::io::Write>(self, writer: T) -> Result<(), Box<dyn Error>> {
        let mut wtr: Writer<T> = csv::Writer::from_writer(writer);
        // write header
        wtr.write_record(&["id", "available", "held", "total", "locked"])?;

        // then write each record
        for (_, v) in self.map.into_iter() {
            wtr.serialize(v)?;
        }

        wtr.flush()?;
        Ok(())
    }
}

#[cfg(test)]
// Lots of tests here - cover all assumptions and most any behaviour I could think up.
mod tests {
    use std::io::BufWriter;

    use super::*;

    #[test]
    fn should_be_able_to_create_new_client_account() {
        let acct = ClientAccount::new(1);
        assert_eq!(acct.id, 1);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(0.0));
        assert!(acct.disputed.is_empty());
        assert!(acct.transaction_history.is_empty());

        assert_eq!(acct.available(), dec!(0.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_deposit_and_store_in_history() {
        let mut acct = ClientAccount::new(2);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        assert_eq!(acct.id, 2);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.1111));
        assert!(acct.disputed.is_empty());
        assert_eq!(acct.transaction_history.len(), 1);
        assert_eq!(
            acct.transaction_history.get(&0).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Deposit,
                amount: 1.1111
            }
        );

        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_withdrawal_and_store_in_history() {
        let mut acct = ClientAccount::new(2);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.id, 2);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.0));
        assert!(acct.disputed.is_empty());
        assert_eq!(acct.transaction_history.len(), 2);
        assert_eq!(
            acct.transaction_history.get(&1).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Withdrawal,
                amount: 0.1111
            }
        );

        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_deposit_dispute() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 0,
            amount: None,
        });

        assert_eq!(acct.id, 1);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.1111));

        // one record should be the deposit tx
        assert_eq!(acct.disputed.len(), 1);
        assert!(acct.disputed.contains(&0));

        assert_eq!(acct.transaction_history.len(), 1);
        assert_eq!(
            acct.transaction_history.get(&0).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Deposit,
                amount: 1.1111
            }
        );

        assert_eq!(acct.available(), dec!(0.0));
        assert_eq!(acct.held(), dec!(1.1111));
    }

    #[test]
    fn client_account_should_fail_to_withdraw_disputed_funds() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 0,
            amount: None,
        });

        // this should be ignored as all funds held
        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.id, 1);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.1111));

        // one record should be the deposit tx
        assert_eq!(acct.disputed.len(), 1);
        assert!(acct.disputed.contains(&0));

        assert_eq!(acct.transaction_history.len(), 2); // failed tx should be logged still.
        assert_eq!(
            acct.transaction_history.get(&0).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Deposit,
                amount: 1.1111
            }
        );

        assert_eq!(acct.available(), dec!(0.0));
        assert_eq!(acct.held(), dec!(1.1111));
    }

    #[test]
    fn client_account_should_ignore_duplicate_deposits() {
        let mut acct = ClientAccount::new(2);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        // This one is entirely ignored
        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        assert_eq!(acct.id, 2);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.1111));
        assert!(acct.disputed.is_empty());
        assert_eq!(acct.transaction_history.len(), 1);
        assert_eq!(
            acct.transaction_history.get(&0).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Deposit,
                amount: 1.1111
            }
        );

        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_ignore_duplicate_withdrawals() {
        let mut acct = ClientAccount::new(2);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        // this one is ignored.
        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.id, 2);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.0));
        assert!(acct.disputed.is_empty());
        assert_eq!(acct.transaction_history.len(), 2);
        assert_eq!(
            acct.transaction_history.get(&1).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Withdrawal,
                amount: 0.1111
            }
        );

        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_deposit_resolution_and_withdraw_funds() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.1111));

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 0,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 1);
        assert!(acct.disputed.contains(&0));
        assert_eq!(acct.total, dec!(1.1111));

        // this should be invalid.
        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(1.1111),
        });

        assert_eq!(acct.total, dec!(1.1111));

        acct.update(Transaction {
            typ: TransactionType::Resolve,
            client: 1,
            tx: 0,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.1111));

        // this should be ignored as it's a duplicate
        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.total, dec!(1.1111));

        // this should be processed as unique
        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(0.1111),
        });

        assert_eq!(acct.id, 1);
        assert_eq!(acct.locked, false);
        assert_eq!(acct.total, dec!(1.0));

        // one record should be the deposit tx
        assert_eq!(acct.disputed.len(), 0);
        assert!(acct.disputed.is_empty());

        assert_eq!(acct.transaction_history.len(), 3); // two are processed, one failed.
        assert_eq!(
            acct.transaction_history.get(&0).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Deposit,
                amount: 1.1111
            }
        );
        assert_eq!(
            acct.transaction_history.get(&1).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::FailedWithdrawal,
                amount: 1.1111
            }
        );
        assert_eq!(
            acct.transaction_history.get(&2).unwrap(),
            &TransactionHistoryRecord {
                typ: TransactionType::Withdrawal,
                amount: 0.1111
            }
        );

        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_deposit_chargeback_if_disputed() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 0,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 1);
        assert!(acct.disputed.contains(&0));
        assert_eq!(acct.total, dec!(1.1111));
        assert_eq!(acct.available(), dec!(0.0));
        assert_eq!(acct.held(), dec!(1.1111));

        acct.update(Transaction {
            typ: TransactionType::Chargeback,
            client: 1,
            tx: 0,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(0.0));
        assert_eq!(acct.available(), dec!(0.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_ignore_deposit_chargeback_if_not_disputed() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.1111));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.0));

        acct.update(Transaction {
            typ: TransactionType::Chargeback,
            client: 1,
            tx: 0,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.1111));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_withdrawal_dispute() {
        // kind of a wierd case but it's managed without holding as an assumption.
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.0));
        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        // one dispute, but no change in held assets
        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.0));
        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_withdrawal_resolution() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        acct.update(Transaction {
            typ: TransactionType::Resolve,
            client: 1,
            tx: 1,
            amount: None,
        });

        // ensure dispute removed
        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.0));
        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_process_withdrawal_chargeback() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        acct.update(Transaction {
            typ: TransactionType::Chargeback,
            client: 1,
            tx: 1,
            amount: None,
        });

        // ensure dispute removed and account debited
        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.1111));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.0));
        assert!(acct.locked);
    }

    #[test]
    fn client_account_should_ignore_larger_withdrawal_than_available() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(1.1112),
        });

        assert_eq!(acct.total, dec!(1.1111));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_ignore_larger_withdrawal_than_available_with_held_funds() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.2222));
        assert_eq!(acct.held(), dec!(0.0));

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        // check dispute applied
        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.1111));

        // try to draw just a bit more
        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(1.1112),
        });

        // ensure it's just ignored.
        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.1111));
    }

    #[test]
    fn client_account_should_ignore_unknown_disputes() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.0));
        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));

        // Reference invalid tx id
        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 3,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 0);
        assert_eq!(acct.total, dec!(1.0));
        assert_eq!(acct.available(), dec!(1.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_account_should_ignore_unknown_resolution() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.1111));

        acct.update(Transaction {
            typ: TransactionType::Resolve,
            client: 1,
            tx: 6, // bad tx
            amount: None,
        });

        // ensure dispute is not resolved.
        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.1111));
    }

    #[test]
    fn client_account_should_ignore_unknown_chargeback() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.1111));

        acct.update(Transaction {
            typ: TransactionType::Chargeback,
            client: 1,
            tx: 6, // bad tx
            amount: None,
        });

        // ensure dispute is not resolved.
        assert_eq!(acct.disputed.len(), 1);
        assert_eq!(acct.total, dec!(1.2222));
        assert_eq!(acct.available(), dec!(1.1111));
        assert_eq!(acct.held(), dec!(0.1111));
    }

    #[test]
    fn client_account_should_calculate_held_with_disputed_deposit_and_withdrawal() {
        let mut acct = ClientAccount::new(1);

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 0,
            amount: Some(1.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Deposit,
            client: 1,
            tx: 1,
            amount: Some(0.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Withdrawal,
            client: 1,
            tx: 2,
            amount: Some(0.1111),
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 1,
            amount: None,
        });

        acct.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 2,
            amount: None,
        });

        assert_eq!(acct.disputed.len(), 2);
        assert_eq!(acct.total, dec!(1.1111));
        assert_eq!(acct.available(), dec!(1.0000));
        // ensure only the disputed deposit is held.
        assert_eq!(acct.held(), dec!(0.1111));
    }

    #[test]
    // This test detects any kind of imprecision accumulation -
    // I had to switch the impl to use Decimal to make this pass!
    // f32 even while rounding off imprecision along the way gets some drift after a couple k.
    // Decimal is proven here. We can store f32 in the tx history but we need Decimal in the client.
    fn client_account_should_not_loose_precision_on_many_updates() {
        let mut acct = ClientAccount::new(1);

        for tx in 0..100000 {
            acct.update(Transaction {
                typ: TransactionType::Deposit,
                client: 1,
                tx,
                amount: Some(0.1111),
            });
        }

        assert_eq!(acct.total, dec!(11110.0))
    }

    #[test]
    // This test detects any kind of imprecision accumulation -
    // I had to switch the impl to use Decimal to make this pass!
    // f32 even while rounding off imprecision along the way gets some drift after a couple k.
    // Decimal is proven here. We can store f32 in the tx history but we need Decimal in the client.
    fn client_account_should_not_loose_precision_on_many_disputes() {
        let mut acct = ClientAccount::new(1);

        // add a slew of additions
        for tx in 0..100000 {
            acct.update(Transaction {
                typ: TransactionType::Deposit,
                client: 1,
                tx,
                amount: Some(0.1111),
            });
        }

        assert_eq!(acct.total, dec!(11110.0));
        assert_eq!(acct.available(), dec!(11110.0));
        assert_eq!(acct.held(), dec!(0.0));

        // check that we can maintain precision while holding history w/ f32 instead of 129bit Decimal
        for tx in 100000..150000 {
            acct.update(Transaction {
                typ: TransactionType::Deposit,
                client: 1,
                tx: tx - 100000,
                amount: Some(0.1111),
            });
        }

        assert_eq!(acct.total, dec!(11110.0));
        assert_eq!(acct.available(), dec!(11110.0));
        assert_eq!(acct.held(), dec!(0.0));
    }

    #[test]
    fn client_accounts_should_write_csv() -> Result<(), Box<dyn Error>> {
        let mut accts = ClientAccounts::new();

        for tx in 0..100000 {
            let _ = &accts.update(Transaction {
                typ: TransactionType::Deposit,
                client: 1,
                tx,
                amount: Some(0.1111),
            })?;
        }

        // check that we can maintain precision while holding history w/ f32 instead of 129bit Decimal
        for tx in 100000..150000 {
            let _ = &accts.update(Transaction {
                typ: TransactionType::Withdrawal,
                client: 1,
                tx,
                amount: Some(0.1111),
            })?;
        }

        let mut buf = BufWriter::new(Vec::new());

        accts.write_csv(&mut buf)?;

        let bytes = buf.into_inner()?;
        let string = String::from_utf8(bytes)?;

        // 0.1111 * (100000-50000) = 5555
        assert_eq!(
            string,
            "id,available,held,total,locked\n1,5555.0000,0.0,5555.0000,false\n"
        );

        Ok(())
    }

    #[test]
    fn client_accounts_should_write_csv_with_open_dispute() -> Result<(), Box<dyn Error>> {
        let mut accts = ClientAccounts::new();

        for tx in 0..100000 {
            let _ = &accts.update(Transaction {
                typ: TransactionType::Deposit,
                client: 1,
                tx,
                amount: Some(0.1111),
            })?;
        }

        // check that we can maintain precision while holding history w/ f32 instead of 129bit Decimal
        for tx in 100000..150000 {
            let _ = &accts.update(Transaction {
                typ: TransactionType::Withdrawal,
                client: 1,
                tx,
                amount: Some(0.1111),
            })?;
        }

        accts.update(Transaction {
            typ: TransactionType::Dispute,
            client: 1,
            tx: 4,
            amount: None,
        })?;

        let mut buf = BufWriter::new(Vec::new());

        accts.write_csv(&mut buf)?;

        let bytes = buf.into_inner()?;
        let string = String::from_utf8(bytes)?;

        // 5555.0 - 0.1111 = 5554.8889
        // Total funds should be 5555, held .1111, and avail 5554.8889.
        assert_eq!(
            string,
            "id,available,held,total,locked\n1,5554.8889,0.1111,5555.0000,false\n"
        );

        Ok(())
    }
}
