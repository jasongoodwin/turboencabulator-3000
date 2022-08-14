use serde::{Deserialize, Deserializer};

/// Enum representing the 5 transaction types.
///
/// Implements Deserialize so can be used with serde.
/// Unknown transaction types will deserialize to Unknown which we just ignore.
#[derive(Debug, Eq, PartialEq)]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
    FailedWithdrawal, // shouldn't see a tx again - if it failed it's still a tx that shouldn't occur.
    Unknown(String),
}

impl<'de> Deserialize<'de> for TransactionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "deposit" => TransactionType::Deposit,
            "withdrawal" => TransactionType::Withdrawal,
            "dispute" => TransactionType::Dispute,
            "resolve" => TransactionType::Resolve,
            "chargeback" => TransactionType::Chargeback,
            _ => TransactionType::Unknown(s),
        })
    }
}

/// Implements a transaction record.
///
/// The `typ` is the type of transaction.
/// client is a u16 representing the unique client id.
/// tx is the transaction id which is an unordered number uniquely representing a transaction.
/// amount is an f32 representing the amount of the transaction. (f32 used assuming USD as it's enough for most of the crypto market cap.)
#[derive(Debug, Deserialize)]
pub struct Transaction {
    #[serde(alias = "type")]
    pub(crate) typ: TransactionType,
    pub(crate) client: u16,
    pub tx: u32,
    pub(crate) amount: Option<f64>,
}

/// This is a slimmer version of the transaction to reduce memory consumption.
/// TODO Didn't measure it much so it may be un-necessary. Easy change.
#[derive(Debug, PartialEq)]
pub struct TransactionHistoryRecord {
    pub(crate) typ: TransactionType,
    pub(crate) amount: f64,
}

#[cfg(test)]
mod tests {
    use csv::Trim::All;
    use indoc::indoc;

    use super::*;

    #[test]
    fn deserialize_deposit_should_succeed() {
        let csv = indoc!(
            "type,client,tx,amount
            deposit,1,1,1.1111
            deposit,1,1,1.1111
        "
        );
        let mut rdr = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        for result in rdr.deserialize() {
            let tx: Transaction = result.unwrap();
            assert_eq!(tx.typ, TransactionType::Deposit);
            assert_eq!(tx.client, 1);
            assert_eq!(tx.tx, 1);
            assert_eq!(tx.amount, Some(1.1111));
        }
    }

    #[test]
    fn deserialize_withdrawal_should_succeed() {
        let csv = indoc!(
            "type,client,tx,amount
            withdrawal,1,1,1.1111
            withdrawal,1,1,1.1111
        "
        );
        let mut rdr = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        for result in rdr.deserialize() {
            let tx: Transaction = result.unwrap();
            assert_eq!(tx.typ, TransactionType::Withdrawal);
            assert_eq!(tx.client, 1);
            assert_eq!(tx.tx, 1);
            assert_eq!(tx.amount, Some(1.1111));
        }
    }

    #[test]
    fn deserialize_dispute_should_succeed() {
        let csv = indoc!(
            "type,client,tx,amount
            dispute,1,1,
            dispute,1,1,
        "
        );
        let mut rdr = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        for result in rdr.deserialize() {
            let tx: Transaction = result.unwrap();
            assert_eq!(tx.typ, TransactionType::Dispute);
            assert_eq!(tx.client, 1);
            assert_eq!(tx.tx, 1);
            assert_eq!(tx.amount, None);
        }
    }

    #[test]
    fn deserialize_chargeback_should_succeed() {
        let csv = indoc!(
            "type,client,tx,amount
            chargeback,1,1,
            chargeback,1,1,
        "
        );
        let mut rdr = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        for result in rdr.deserialize() {
            let tx: Transaction = result.unwrap();
            assert_eq!(tx.typ, TransactionType::Chargeback);
            assert_eq!(tx.client, 1);
            assert_eq!(tx.tx, 1);
            assert_eq!(tx.amount, None);
        }
    }

    #[test]
    fn deserialize_unknown_tx_type_should_succeed() {
        let csv = indoc!(
            "type,client,tx,amount
            pirates_rock,1,1,
            pirates_rock,1,1,
        "
        );
        let mut rdr = csv::ReaderBuilder::new().from_reader(csv.as_bytes());
        for result in rdr.deserialize() {
            let tx: Transaction = result.unwrap();
            assert_eq!(tx.typ, TransactionType::Unknown("pirates_rock".into()));
            assert_eq!(tx.client, 1);
            assert_eq!(tx.tx, 1);
            assert_eq!(tx.amount, None);
        }
    }

    #[test]
    /// validates that the input can have insignificant space.
    /// Extraneous - this is testing the csv lib trim=All
    fn deserialize_with_tabs_and_spaces_should_not_fail() {
        let csv = indoc!(
            "type,     client,     tx,\t\t\tamount
            chargeback,\t1,   1,
            \tchargeback,1,   1,
        "
        );

        let mut rdr = csv::ReaderBuilder::new()
            .trim(All)
            .from_reader(csv.as_bytes());
        for result in rdr.deserialize() {
            let tx: Transaction = result.unwrap();
            assert_eq!(tx.typ, TransactionType::Chargeback);
            assert_eq!(tx.client, 1);
            assert_eq!(tx.tx, 1);
            assert_eq!(tx.amount, None);
        }
    }
}
