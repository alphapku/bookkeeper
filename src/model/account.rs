use std::collections::HashMap;

// use anyhow::*;
use log::*;
use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;

use super::{Transaction, TxType};

const DEFAULT_COUNT: usize = 8096;
const MAX_DECIMAL_PLACES: u32 = 4;

#[derive(Error, Debug, PartialEq)]
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

    /// Happens when ID is not found, or duplicate for dispute/resolve/chargeback
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
    withdrawal_history: HashMap<u32, Transaction>, // TODO: basically we should store deposit_history/withdrawal_history in database in Prod
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
            withdrawal_history: HashMap::with_capacity(DEFAULT_COUNT),
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

                        self.withdrawal_history.insert(tx.tx_id, tx.clone());
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
                return Ok(());
            }
        }

        Err(TxError::InvalidAmountError)
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_deposit(&self, tx: &Transaction) -> Result<Decimal, TxError> {
        debug_assert!(tx.r#type == TxType::Deposit);

        let amount = Self::validate_amount(tx)?;

        if self.deposit_history.contains_key(&tx.tx_id) {
            return Err(TxError::InvalidTxIdError);
        }

        debug!("checking {} for {}", tx.client_id, tx.tx_id);

        Ok(amount)
    }

    /// For simplicity, we dont check if it's duplciate or not. In prod, this could be done through a database.
    fn validate_withdraw(&self, tx: &Transaction) -> Result<Decimal, TxError> {
        debug_assert!(tx.r#type == TxType::Withdrawal);

        let amount = Self::validate_amount(tx)?;

        if amount > self.available_amount {
            return Err(TxError::InvalidAmountError);
        }

        // available_amount is alwayas <= total_amount, so we don't need to check total

        if self.withdrawal_history.contains_key(&tx.tx_id) {
            return Err(TxError::InvalidTxIdError);
        }

        Ok(amount)
    }

    fn validate_account(&self) -> Result<(), TxError> {
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

    use crate::model::{Account, Transaction, TxError, TxType};

    /// Check a flow: deposit(ok) -> withdraw(ok) -> withdraw (failed)
    #[test]
    fn test_deposit_withdraw_invalida_withdraw() {
        let client_id = 1;
        let amount = Decimal::from(2i16);
        let mut deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(amount),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());
        assert!(acct.held_amount == Decimal::ZERO);
        assert!(acct.available_amount == amount);
        assert!(acct.total_amount == amount);

        let amount2 = Decimal::new(9, 1);
        deposit.tx_id = 2;
        deposit.amount = Some(amount2);
        assert!(acct.on_tx(&deposit).is_ok());

        let total_amount = amount + amount2;
        assert!(acct.held_amount == Decimal::ZERO);
        assert!(acct.available_amount == total_amount);
        assert!(acct.total_amount == total_amount);

        let withdrawal_amount = Decimal::new(15, 1);
        let balance = total_amount - withdrawal_amount;
        let mut withdrawal = Transaction {
            r#type: TxType::Withdrawal,
            client_id,
            tx_id: 3,
            amount: Some(withdrawal_amount),
        };

        assert!(acct.on_tx(&withdrawal).is_ok());
        assert!(acct.held_amount == Decimal::ZERO);
        assert!(acct.available_amount == balance);
        assert!(acct.total_amount == balance);

        withdrawal.tx_id = 3;
        assert!(acct.on_tx(&withdrawal).err().unwrap() == TxError::InvalidAmountError);

        // amounts are not changed after an insufficient withdrawal
        assert!(acct.held_amount == Decimal::ZERO);
        assert!(acct.available_amount == balance);
        assert!(acct.total_amount == balance);
    }

    /// Check a normal flow: deposit(ok) -> dispute(ok) -> resolve (ok)
    #[test]
    fn test_dispute_resolve() {
        let client_id = 1;
        let amount = Decimal::from(2i16);

        let deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(amount),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());

        let dispute = Transaction {
            r#type: TxType::Dispute,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&dispute).is_ok());
        assert!(acct.held_amount == amount);
        assert!(acct.available_amount == amount - amount);
        assert!(acct.total_amount == amount);

        let resolve = Transaction {
            r#type: TxType::Resolve,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&resolve).is_ok());
        assert!(acct.held_amount == Decimal::ZERO);
        assert!(acct.available_amount == amount);
        assert!(acct.total_amount == amount);
    }

    /// Check a normal flow: deposit(ok) -> dispute(ok) -> chargeback (ok)
    #[test]
    fn test_dispute_chargeback() {
        let client_id = 1;
        let amount = Decimal::from(2i16);

        let deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(amount),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());

        let dispute = Transaction {
            r#type: TxType::Dispute,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&dispute).is_ok());

        let chargeback = Transaction {
            r#type: TxType::ChargeBack,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&chargeback).is_ok());
        assert!(acct.held_amount == Decimal::ZERO);
        assert!(acct.available_amount == Decimal::ZERO);
        assert!(acct.total_amount == Decimal::ZERO);
    }

    /// Check a flow: deposit(failed)
    #[test]
    fn test_deposit_invalid_amount() {
        let client_id = 1;

        let mut deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(0i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_err());

        deposit.amount = Some(Decimal::from_str("-0.001").unwrap());
        assert!(acct.on_tx(&deposit).err().unwrap() == TxError::InvalidAmountError);
    }

    /// Check a flow: deposit(ok) -> duplicate deposit(failed)
    #[test]
    fn test_deposit_duplicate() {
        let client_id = 1;

        let deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(1i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());
        assert!(acct.on_tx(&deposit).err().unwrap() == TxError::InvalidTxIdError);
    }

    /// Check a flow: deposit(ok) -> duplicate withdrawal(failed)
    #[test]
    fn test_withdraw_duplicate() {
        let client_id = 1;

        let deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(10i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());

        let withdrawal = Transaction {
            r#type: TxType::Withdrawal,
            client_id,
            tx_id: 2,
            amount: Some(Decimal::from(1i16)),
        };
        assert!(acct.on_tx(&withdrawal).is_ok());

        assert!(acct.on_tx(&withdrawal).err().unwrap() == TxError::InvalidTxIdError);
    }

    /// Check a flow: deposit(failed)/withdrawal(failed)
    #[test]
    fn test_missing_amount() {
        let client_id = 1;

        let deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: None,
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).err().unwrap() == TxError::MissingAmountError);

        let withdrawal = Transaction {
            r#type: TxType::Withdrawal,
            client_id,
            tx_id: 2,
            amount: None,
        };

        assert!(acct.on_tx(&withdrawal).err().unwrap() == TxError::MissingAmountError);
    }

    /// Check a flow: locked account -> can't operate on locked account
    #[test]
    fn test_locked_account() {
        let client_id = 1;

        let mut deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(1i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());

        let dispute = Transaction {
            r#type: TxType::Dispute,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&dispute).is_ok());

        let chargeback = Transaction {
            r#type: TxType::ChargeBack,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&chargeback).is_ok());

        assert!(acct.locked);

        deposit.tx_id = 2;
        assert!(acct.on_tx(&deposit).err().unwrap() == TxError::LockedAccountError);
    }

    /// Check a flow: try to resolve/chargeback on a non-disputed transaction
    #[test]
    fn test_invalid_operation() {
        let client_id = 1;

        let deposit = Transaction {
            r#type: TxType::Deposit,
            client_id,
            tx_id: 1,
            amount: Some(Decimal::from(1i16)),
        };

        let mut acct = Account::new(client_id);

        assert!(acct.on_tx(&deposit).is_ok());

        let mut invalid_op = Transaction {
            r#type: TxType::Resolve,
            client_id,
            tx_id: 1,
            amount: None,
        };

        assert!(acct.on_tx(&invalid_op).err().unwrap() == TxError::InvalidOperatioonError);

        invalid_op.r#type = TxType::ChargeBack;
        assert!(acct.on_tx(&invalid_op).err().unwrap() == TxError::InvalidOperatioonError);
    }
}
