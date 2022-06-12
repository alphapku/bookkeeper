use std::{
    collections::{hash_map::Entry, HashMap},
    io,
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

    pub fn on_tx(&mut self, tx: &Transaction) -> Result<(), TxError> {
        match self.accounts.entry(tx.client_id) {
            Entry::Occupied(mut entry) => entry.get_mut().on_tx(tx)?,
            Entry::Vacant(entry) => {
                if tx.r#type == TxType::Deposit {
                    let mut acct = Account::new(tx.client_id);
                    acct.on_tx(tx)?;
                    entry.insert(acct);
                    info!("a new accout({}) is created", tx.client_id);
                } else {
                    return Err(TxError::InvalidClientError);
                }
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
}

impl Default for Bookkeeper {
    fn default() -> Self {
        Self::new()
    }
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
