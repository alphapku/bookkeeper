use core::result::Result::Ok;
use std::env;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind};

use anyhow::*;

use log::*;

use bkeeper::model::Bookkeeper;

fn main() -> Result<()> {
    env_logger::builder().format_timestamp_nanos().target(env_logger::Target::Stdout).init();
    if env::args().count() != 2 {
        print_usage();
        return Ok(());
    }

    let f = File::open(get_first_arg()?)?;
    let mut reader = csv::ReaderBuilder::new().trim(csv::Trim::All).from_reader(BufReader::new(f));
    let mut raw_record = csv::StringRecord::new();
    let headers = reader.headers()?.clone();

    let mut keeper = Bookkeeper::default();
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

    Ok(())
}

fn print_usage() {
    info!("bookkeeper input.csv\nor\ncargo run -- input.csv")
}

fn get_first_arg() -> Result<OsString> {
    match env::args_os().nth(1) {
        None => Err(anyhow::Error::new(Error::new(
            ErrorKind::InvalidData,
            "the input file is not represented",
        ))),
        Some(file_path) => Ok(file_path),
    }
}
