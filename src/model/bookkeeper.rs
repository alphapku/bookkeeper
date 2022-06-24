use std::{
    collections::{hash_map::Entry, HashMap},
    io::{self, Read},
};

use log::*;

use super::{Account, Transaction, TxError, TxType};

const DEFAULT_ACCOUNT_COUNT: usize = 4086;

pub struct Bookkeeper {
    pub accounts: HashMap<u16, Account>,
}

impl Bookkeeper {
    pub fn new() -> Bookkeeper {
        Bookkeeper {
            accounts: HashMap::with_capacity(DEFAULT_ACCOUNT_COUNT),
        }
    }

    pub fn process_reader<R>(&mut self, r: R) -> Result<(), csv::Error>
    where
        R: Read,
    {
        let mut reader = csv::ReaderBuilder::new().trim(csv::Trim::All).from_reader(r);
        let mut raw_record = csv::StringRecord::new();
        let headers = reader.headers()?.clone();
        let trimed_headers = trim_string_record(&headers);

        while reader.read_record(&mut raw_record)? {
            let trimed_raw_record = trim_string_record(&raw_record);
            match trimed_raw_record.deserialize(Some(&trimed_headers)) {
                Ok(tx) => {
                    if let Some(e) = self.on_tx(&tx).err() {
                        error!("failed to process transaction({:?}): {:?}", tx, e);
                    }
                }
                Err(e) => error!("failed to deserialize transaction({:?}): {:?}", trimed_raw_record, e),
            }
        }

        Ok(())
    }

    pub fn report_balance(&self) -> Result<(), csv::Error> {
        info!("{} account(s)", self.accounts.len());

        let mut writer = csv::Writer::from_writer(io::stdout());

        for acct in self.accounts.values() {
            writer.serialize(acct)?;
        }

        writer.flush()?;

        Ok(())
    }

    fn on_tx(&mut self, tx: &Transaction) -> Result<(), TxError> {
        self.accounts.entry(tx.client_id).or_insert(Account::new(tx.client_id)).on_tx(tx)
    }
}

impl Default for Bookkeeper {
    fn default() -> Self {
        Self::new()
    }
}

/// trim_string_record removes all the spaces in the input
fn trim_string_record(s: &csv::StringRecord) -> csv::StringRecord {
    let mut trimed_string_record = csv::StringRecord::new();
    for field in s {
        let mut f = field.to_string();
        f.retain(|c| !c.is_whitespace());
        trimed_string_record.push_field(&f[..]);
    }
    trimed_string_record
}

#[cfg(test)]
mod test {
    use crate::model::{Bookkeeper, Transaction, TxError, TxType};

    #[test]
    fn test_client_invalid() {
        let client_id = 1;

        let dispute = Transaction {
            r#type: TxType::Dispute,
            client_id,
            tx_id: 1,
            amount: None,
        };

        let mut bkeeper = Bookkeeper::new();
        assert!(bkeeper.on_tx(&dispute).err().unwrap() == TxError::InvalidClientError);
    }
}
