#[macro_use]
extern crate clap;
#[macro_use]
extern crate log;
extern crate common;
extern crate ctrlc;
extern crate database;
extern crate protobuf;
extern crate regex;
extern crate sawtooth_sdk;
extern crate simple_logger;
extern crate uuid;

#[macro_use]
mod transformer;
mod errors;

pub mod event_handler;
pub mod subscriber;

use database::data_manager::DataManager;
use event_handler::EventHandler;
use log::LogLevel;
use std::sync::atomic::Ordering;
use subscriber::Subscriber;

/// Entry point for the subscriber
/// Establish a connection with the reporting database and fetches
/// all known block ids that are already in the Database
/// Starts the subscriber passing the know blocks ids
#[cfg_attr(tarpaulin, skip)]
fn main() {
    let matches = clap_app!(creg_subscriber =>
        (version: crate_version!())
        (about: "Cert Registry Subscriber")
        (@arg connect: default_value("tcp://localhost:4004") -C --connect +takes_value
           "connection endpoint for validator")
        (@arg verbose: -v --verbose +multiple
           "increase output verbosity")
        (@arg dbname: default_value("cert-registry") --dbname +takes_value
           "the name of the database")
        (@arg dbhost: default_value("localhost") --dbhost +takes_value
            "the host of the database")
        (@arg dbport: default_value("5432") --dbport +takes_value
            "the port of the database")
        (@arg dbuser: default_value("cert-registry") --dbuser +takes_value
            "the authorized user of the database")
        (@arg dbpass: default_value("cert-registry") --dbpass +takes_value
            "the authorized user's password for database access"))
    .get_matches();

    let _logger = match matches.occurrences_of("verbose") {
        1 => simple_logger::init_with_level(LogLevel::Info),
        2 => simple_logger::init_with_level(LogLevel::Debug),
        0 | _ => simple_logger::init_with_level(LogLevel::Warn),
    };

    let dsn = format!(
        "{}:{}@{}:{}/{}",
        matches.value_of("dbuser").unwrap(),
        matches.value_of("dbpass").unwrap(),
        matches.value_of("dbhost").unwrap(),
        matches.value_of("dbport").unwrap(),
        matches.value_of("dbname").unwrap()
    );

    let manager = DataManager::new(&dsn).expect("Failed to connect to database");
    let last_blocks = manager
        .fetch_known_blocks()
        .expect("Error fetching known blocks");
    let known_block_ids: Vec<String> = last_blocks
        .into_iter()
        .map(|block| block.block_id)
        .collect();
    let event_handler = EventHandler::new(manager);
    let mut subscriber = Subscriber::new(matches.value_of("connect").unwrap(), event_handler);

    let active = subscriber.active.clone();
    ctrlc::set_handler(move || {
        active.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    subscriber
        .start(&known_block_ids, 0)
        .expect("Error subscribing to validator");
}
