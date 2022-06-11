use core::result::Result::Ok;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};

use anyhow::*;
use bookkeeper::model::Bookkeeper;
use csv::Reader;
use log::*;

use bookkeeper::model;

fn main() -> Result<()> {
    env_logger::builder().format_timestamp_nanos().target(env_logger::Target::Stdout).init();

    let f = File::open("tx0.csv")?;
    let mut reader = csv::ReaderBuilder::new().trim(csv::Trim::All).from_reader(BufReader::new(f));
    let mut raw_record = csv::StringRecord::new();
    let headers = reader.headers()?.clone();

    let mut keeper = Bookkeeper::new();
    while reader.read_record(&mut raw_record)? {
        match raw_record.deserialize(Some(&headers)) {
            Ok(tx) => {
                if let Some(e) = keeper.on_tx(&tx).err() {
                    error!("failed to process transaction({:?}): {:?}", tx, e);
                }
            }
            Err(e) => error!("failed to deserialize transaction({:?}): {:?}", raw_record, e),
        }
    }

    keeper.report_balance()?;

    info!("Hello, world!");

    Ok(())
}
