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

    let mut keeper = Bookkeeper::default();
    keeper.process_reader(BufReader::new(f))?;
    keeper.report_balance()?;

    Ok(())
}

fn print_usage() {
    info!("bkeeper transactions.csv\nor\ncargo run -- transactions.csv > accounts.csv")
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
