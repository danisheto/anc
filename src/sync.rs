use std::{path::PathBuf, io::Cursor};

use anki::{sync::SyncAuth, collection::CollectionBuilder};
use rusqlite::params;
use serde_pickle::{Deserializer, DeOptions, Value, HashableValue};

use crate::get_config;

pub async fn sync() {
    let config = get_config().unwrap();
    let auth = get_auth(&config.anki_dir)
        .map(|(hkey, host_number)| {
            SyncAuth {
                hkey,
                host_number: host_number.unwrap_or(0) as u32,
            }
        }).unwrap();
    let mut collection = CollectionBuilder::new(config.anki_dir.join("collection.anki2")).build().unwrap();
    collection.normal_sync(auth, |_progress, _done| { }).await.unwrap();
}

pub fn get_auth(path: &PathBuf) -> Option<(String, Option<i64>)> {
    let conn = rusqlite::Connection::open(path.parent().unwrap().join("prefs21.db")).unwrap();
    let mut get_profile = conn.prepare("select cast(data as blob) from profiles where name = ?").unwrap();
    let profile_bytes: Vec<u8> = get_profile.query(params!["Test"]).unwrap().next().unwrap().unwrap().get(0).unwrap();
    let profile = Deserializer::new(Cursor::new(profile_bytes.as_slice()), DeOptions::new()).deserialize_value().unwrap();
    // TODO: error that user needs to sync or login first
    if let Value::Dict(mut v) = profile {
        let hkey = v.remove(&HashableValue::String("syncKey".to_string()))
            .map(|k| {
                if let Value::String(key) = k {
                    Some(key)
                } else {
                    None
                }
            }).flatten();
        let host_number = v.remove(&HashableValue::String("hostNum".to_string()))
            .map(|k| {
                if let Value::I64(n) = k {
                    Some(n)
                } else {
                    None
                }
            }).flatten();
        hkey.map(|h| (h, host_number))
    } else { None }
}
