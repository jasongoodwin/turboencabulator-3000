use serde::{Deserialize, Deserializer};

/// Enum representing the 5 transaction types.
///
/// Implements Deserialize so can be used with serde.
/// Unknown transaction types will deserialize to Unknown - deserialization should not ever fail.
#[derive(Debug, PartialEq)]
pub enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
    Unknown(String),
}

impl<'de> Deserialize<'de> for TransactionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de>
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
    pub tx: u32, // This can be used to detect duplicate processing but is ignored.
    pub(crate) amount: Option<f32>,
}

// impl Transaction {
    // pub fn new(typ: TransactionType, client: u16, tx: u32, amount: f32) -> Transaction {
    //     Transaction { typ, client, tx, amount }
    // }
// }
