use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TxType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    ChargeBack,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "lowercase")]
pub struct Transaction {
    pub r#type: TxType,
    #[serde(rename(deserialize = "client"))]
    pub client_id: u16,
    #[serde(rename(deserialize = "tx"))]
    pub tx_id: u32,
    pub amount: Option<Decimal>,
}
