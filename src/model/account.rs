use std::collections::{HashMap, HashSet};

// use anyhow::*;
use log::*;
use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;

use super::{Transaction, TxType};

const DEFAULT_COUNT: usize = 8096;
const MAX_DECIMAL_PLACES: u32 = 4;

#[derive(Error, Debug)]
pub enum TxError {
    /// Happens when a non-deposit comes, but the client is unexisted
    #[error("invalid client")]
    InvalidClientError,

    /// Happens when Amount is required not is abscent
    #[error("invalid amount")]
    MissingAmountError,

    /// Happens when failing to process Amount, e.g, too big, or too small
    #[error("invalid amount")]
    InvalidAmountError,

    /// Happens when ID is not found for dispute/resolve/chargeback
    #[error("invalid Tx ID")]
    InvalidTxIdError,

    /// Happens when failing to read a csv record
    #[error("invalid format transaction")]
    InvaidFormatError,

    /// Happens when trying to do transactions on a locked account
    #[error("locked account")]
    LockedAccountError,

    /// Happens when trying to do transactions on transactions with unexpected statuses, e.g., resolving on an non-disputed transaction
    #[error("invalid operation")]
    InvalidOperatioonError,

    /// Happens when failing to deserialize a transaction from csv record
    #[error("invalid transaction")]
    InvalidTxError(#[from] csv::Error),

    /// Happens when failing to process I/O
    #[error("I/O error")]
    TxIoError(#[from] std::io::Error),
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub struct Account {
    #[serde(rename(serialize = "client"))]
    pub client_id: u16,
    #[serde(rename(serialize = "available"))]
    pub available_amount: Decimal,
    #[serde(rename(serialize = "held"))]
    pub held_amount: Decimal,
    #[serde(rename(serialize = "total"))]
    pub total_amount: Decimal,
    pub locked: bool,

    #[serde(skip_serializing)]
    deposit_history: HashMap<u32, Deposit>,

    #[serde(skip_serializing)]
    tx_history: HashSet<Transaction>, // TODO: basically we should store deposit_history/tx_history in database in Prod
}

impl Account {
    pub fn new(client_id: u16) -> Account {
        Account {
            client_id,
            held_amount: Decimal::ZERO,
            available_amount: Decimal::ZERO,
            total_amount: Decimal::ZERO,
            locked: false,
            deposit_history: HashMap::with_capacity(DEFAULT_COUNT),
            tx_history: HashSet::with_capacity(DEFAULT_COUNT),
        }
    }

    pub fn on_tx(&mut self, tx: &Transaction) -> Result<(), TxError> {
        match tx.r#type {
            TxType::Deposit => self.on_deposit(tx)?,
            TxType::Withdrawal => self.on_withdraw(tx)?,
            TxType::Dispute => self.on_dispute(tx)?,
            TxType::Resolve => self.on_resolve(tx)?,
            TxType::ChargeBack => self.on_chargeback(tx)?,
        }

        Ok(())
    }

    fn on_deposit(&mut self, tx: &Transaction) -> Result<(), TxError> {
        debug!("{:?}", tx);

        self.validate_account()?;

        let amount = Self::adjust_scale(&self.validate_deposit(tx)?);

        if let Some(new_available) = self.available_amount.checked_add(amount) {
            if let Some(new_total) = self.total_amount.checked_add(amount) {
                self.available_amount = new_available;
                self.total_amount = new_total;

                self.deposit_history.insert(
                    tx.tx_id,
                    Deposit {
                        amount,
                        status: DepositStatus::Normal,
                    },
                );

                return Ok(());
            }
        }

        Err(TxError::InvalidAmountError)
    }

    fn on_withdraw(&mut self, tx: &Transaction) -> Result<(), TxError> {
        debug!("{:?}", tx);

        self.validate_account()?;

        let amount = Self::adjust_scale(&self.validate_withdraw(tx)?);

        if let Some(new_available) = self.available_amount.checked_sub(amount) {
            if new_available >= Decimal::ZERO {
                if let Some(new_total) = self.total_amount.checked_sub(amount) {
                    if new_total >= Decimal::ZERO {
                        self.available_amount = new_available;
                        self.total_amount = new_total;
                        return Ok(());
                    }
                }
            }
        }

        Err(TxError::InvalidAmountError)
    }

    fn on_dispute(&mut self, tx: &Transaction) -> Result<(), TxError> {
        debug!("{:?}", tx);

        self.validate_account()?;

        let deposit = Self::validate_dispute(&mut self.deposit_history, tx)?;
        let amount = deposit.amount;

        if let Some(new_held) = self.held_amount.checked_add(amount) {
            if let Some(new_available) = self.available_amount.checked_sub(amount) {
                deposit.status = DepositStatus::Disputed;
                self.held_amount = new_held;
                self.available_amount = new_available;
                return Ok(());
            }
        }

        Err(TxError::InvalidAmountError)
    }

    fn on_resolve(&mut self, tx: &Transaction) -> Result<(), TxError> {
        debug!("{:?}", tx);

        self.validate_account()?;

        let deposit = Self::validate_resolve(&mut self.deposit_history, tx)?;
        let amount = deposit.amount;

        if let Some(new_held) = self.held_amount.checked_sub(amount) {
            if let Some(new_available) = self.available_amount.checked_add(amount) {
                self.held_amount = new_held;
                self.available_amount = new_available;
                deposit.status = DepositStatus::Resolved;
                return Ok(());
            }
        }

        Err(TxError::InvalidAmountError)
    }

    fn on_chargeback(&mut self, tx: &Transaction) -> Result<(), TxError> {
        debug!("{:?}", tx);

        self.validate_account()?;

        let deposit = Self::validate_chargeback(&mut self.deposit_history, tx)?;
        let amount = deposit.amount;

        if let Some(new_held) = self.held_amount.checked_sub(amount) {
            if let Some(new_total) = self.total_amount.checked_sub(amount) {
                self.held_amount = new_held;
                self.total_amount = new_total;
                deposit.status = DepositStatus::ChargedBack;
                self.locked = true; // TODO, how to unlock?
            }
        }

        Err(TxError::InvalidAmountError)
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_deposit(&mut self, tx: &Transaction) -> Result<Decimal, TxError> {
        debug_assert!(tx.r#type == TxType::Deposit);

        let amount = Self::validate_amount(tx)?;

        if self.deposit_history.contains_key(&tx.tx_id) {
            return Err(TxError::InvalidTxIdError);
        }

        debug!("checking {} for {}", tx.client_id, tx.tx_id);

        Ok(amount)
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_withdraw(&mut self, tx: &Transaction) -> Result<Decimal, TxError> {
        debug_assert!(tx.r#type == TxType::Withdrawal);

        let amount = Self::validate_amount(tx)?;

        if amount < self.available_amount {
            return Err(TxError::InvalidAmountError);
        }

        // available_amount is alwayas <= total_amount, so we don't need to check total

        Ok(amount)
    }

    fn validate_account(&mut self) -> Result<(), TxError> {
        if self.locked {
            return Err(TxError::LockedAccountError);
        }

        Ok(())
    }

    fn validate_amount(tx: &Transaction) -> Result<Decimal, TxError> {
        debug_assert!(tx.r#type == TxType::Deposit || tx.r#type == TxType::Withdrawal);

        if let Some(amount) = tx.amount {
            if amount <= Decimal::ZERO {
                return Err(TxError::InvalidAmountError);
            }

            return Ok(amount);
        }

        Err(TxError::MissingAmountError)
    }

    fn adjust_scale(amt: &Decimal) -> Decimal {
        // for simplity, we adjust for all, without checking if its decimal palces are great than 4 or not
        let mut ret = *amt;
        ret.rescale(MAX_DECIMAL_PLACES);
        ret
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_dispute<'a>(history: &'a mut HashMap<u32, Deposit>, tx: &Transaction) -> Result<&'a mut Deposit, TxError> {
        debug_assert!(tx.r#type == TxType::Dispute);

        if let Some(deposit) = history.get_mut(&tx.tx_id) {
            if deposit.status != DepositStatus::Normal {
                return Err(TxError::InvalidOperatioonError);
            }

            return Ok(deposit);
        }

        Err(TxError::InvalidTxIdError)
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_resolve<'a>(history: &'a mut HashMap<u32, Deposit>, tx: &Transaction) -> Result<&'a mut Deposit, TxError> {
        debug_assert!(tx.r#type == TxType::Resolve);

        if let Some(deposit) = history.get_mut(&tx.tx_id) {
            if deposit.status != DepositStatus::Disputed {
                return Err(TxError::InvalidOperatioonError);
            }

            return Ok(deposit);
        }

        Err(TxError::InvalidTxIdError)
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_chargeback<'a>(history: &'a mut HashMap<u32, Deposit>, tx: &Transaction) -> Result<&'a mut Deposit, TxError> {
        debug_assert!(tx.r#type == TxType::ChargeBack);

        if let Some(deposit) = history.get_mut(&tx.tx_id) {
            if deposit.status != DepositStatus::Disputed {
                return Err(TxError::InvalidOperatioonError);
            }

            return Ok(deposit);
        }

        Err(TxError::InvalidTxIdError)
    }
}

#[derive(PartialEq)]
enum DepositStatus {
    Normal,
    Disputed,
    Resolved,
    ChargedBack,
}

struct Deposit {
    amount: Decimal,
    status: DepositStatus,
}

#[cfg(test)]
mod test {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    use crate::model::{Account, Transaction};

    #[test]
    fn test_deposit_invalid_amount() {
        let client_id = 1;

        let mut deposit = Transaction {
            r#type: crate::model::TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(0i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_err());

        deposit.amount = Some(Decimal::from_str("-0.001").unwrap());
        assert!(acct.on_tx(&deposit).is_err());
    }

    #[test]
    fn test_deposit_duplicate() {
        let client_id = 1;

        let deposit = Transaction {
            r#type: crate::model::TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(1i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());
        assert!(acct.on_tx(&deposit).is_err());
    }
}
