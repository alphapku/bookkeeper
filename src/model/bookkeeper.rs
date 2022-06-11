use std::{
    collections::{hash_map::Entry, HashMap},
    io,
};

use log::*;

use crate::model::TxType;

use super::{Account, Transaction, TxError};

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

    pub fn report_balance(&self) -> Result<(), TxError> {
        info!("{} account(s)", self.accounts.len());

        let mut writer = csv::Writer::from_writer(io::stdout());

        for (_, acct) in &self.accounts {
            writer.serialize(acct).map_err(|e| TxError::InvalidTxError(e))?;
        }

        writer.flush().map_err(|e| TxError::TxIoError(e))?;

        Ok(())
    }
}
