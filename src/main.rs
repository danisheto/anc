use std::{env, time::SystemTime, fs, collections::HashMap};
use anki::{collection::CollectionBuilder, notes::NoteId, timestamp::TimestampSecs, decks::{DeckKindContainer, DeckKind, DeckId}, prelude::DeckConfigId, deckconfig::NewCardInsertOrder};
use itertools::{Itertools, Either};

use cards::{Deck, TypeGroup};
use parsing::parse_card;
use rusqlite::params;
use uuid::Uuid;
use prost::Message;

pub mod cards;
pub mod parsing;

fn main() {
    // read/parse from files
    let args: Vec<String> = env::args().collect();
    let dir = &args[1];

    // TODO: accept list of files instead of a directory
    let paths: Vec<_> = fs::read_dir(dir).unwrap().into_iter()
        .map(|p| p.unwrap().path().canonicalize().unwrap())
        .filter(|p| p.is_file() && 
            Some("qz") == p.extension()
                .map(|e| e.to_str())
                .flatten()
        )
        .map(|p| p.display().to_string())
        .collect();

    let card_results: Vec<_> = paths.iter()
        .map(|p| parse_card(p))
        .collect();

    let (error_res, card_res): (Vec<_>, Vec<_>) = card_results.into_iter()
        .zip(paths)
        .partition(|(c, _)| c.is_err());

    let errors: Vec<_> = error_res.into_iter()
        .map(|(r, p)| (r.unwrap_err(), p))
        .collect();

    if errors.len() > 0 {
        for (r, p) in errors {
            eprintln!("{}: {}", p, r);
        }
        std::process::exit(65);
    }

    let cards: Vec<_> = card_res.into_iter()
        .map(|(r, _)| r.unwrap())
        .group_by(|(deck_name, _)| deck_name.to_string())
        .into_iter()
        .map(|(deck_name, group)| {
            let types: Vec<_> = group
                .into_iter()
                .map(|(_, card)| card)
                .group_by(|c| c.model.clone())
                .into_iter()
                .map(|(model, cards)| {
                    TypeGroup {
                        model: model.to_string(),
                        cards: cards.into_iter().collect(),
                    }
                })
                .collect();

            Deck {
                name: deck_name,
                groups: types,
            }
        })
        .collect();

    // add/update from collection
    let path = env::var("TEST_ANKI").expect("For testing, need a $TEST_ANKI");
    process_cards(&path, cards);

    // TODO: log
}

// TODO:
// - Check for duplicates
// - tags
fn process_cards(path: &str, decks: Vec<Deck>) {
    let mut note_ids: Vec<NoteId> = vec![];
    let mut collection = CollectionBuilder::new(path).build().unwrap();
    for d in decks {
        // TODO: better error handling
        // only import if all work
        for g in d.groups {
            let deck_id: i64;
            let config_id: i64;
            {
                let mut type_ids = HashMap::new();
                // TODO: check number of required fields and fill fields with empty string
                let mut type_id_query = collection.storage.db.prepare(
                    "
                        SELECT id
                        FROM notetypes
                        WHERE name like ?
                        order by name collate nocase
                    ").unwrap();

                let mut nid_by_first_field = collection.storage.db.prepare(
                    "
                        SELECT id
                        FROM notes
                        WHERE SUBSTR(flds, 0, INSTR(flds, char(31))) like ?
                        limit 1
                    ").unwrap();
                let mut check_time = collection.storage.db.prepare("SELECT ifnull(max(id), 0) FROM notes").unwrap();
                let mut usn_statement = collection.storage.db.prepare("select usn from col").unwrap();
                // TODO: try named parameters instead
                let mut insert_note = collection.storage.db.prepare("insert or replace into notes values (?, ?, ?, ?, ?, '', ?, ?, 0, 0, '')").unwrap();
                let mut update_note = collection.storage.db.prepare(
                    "update notes set mod = ?, usn = ?, flds = ?, sfld = ?
                     where id = ? and flds != ?"
                ).unwrap();
                let mut get_deck = collection.storage.db.prepare("select id from decks where name like ?").unwrap();
                let mut get_deck_kind = collection.storage.db.prepare("select kind from decks where id = ?").unwrap();
                let mut set_config = collection.storage.db.prepare("insert or replace into config (key, usn, mtime_secs, val) values (?, ?, ?, ?)").unwrap();

                let type_id = if let Some(id) = type_ids.get(&g.model) {
                    *id
                } else {
                    let id = type_id_query.query(params![g.model]).unwrap()
                        .next()
                        .unwrap()
                        .expect("Can't find card model")
                        .get::<usize, i64>(0) // TODO: handle if there's multiple
                                // request user to pick the correct one and rename accordingly
                        .expect("Can't find card model");
                    type_ids.insert(g.model, id);
                    id
                };
                // compute
                // split into adds and updates
                // TODO: calculating the first field probably requires a full table scan
                // get around this by computing checksums to do a range scan first
                let (to_add, to_update): (Vec<_>, Vec<_>) = g.cards
                    .iter()
                    .partition_map(|card| {
                        if let Ok(Some(row)) = nid_by_first_field.query(params![card.fields.get(0).unwrap()]).unwrap().next() {
                            let note_id: i64 = row.get(0).unwrap();
                            Either::Right((note_id, card))
                        } else {
                            Either::Left(card)
                        }
                    });
                // note id
                let current = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64;
                let mut next_note_id = if let Some(row) = check_time.query([]).unwrap().next().unwrap() {
                    let max: i64 = row.get(0).unwrap();
                    if max > current {
                        max + 1
                    } else {
                        current
                    }
                } else {
                    current
                };
                // grab usn - no idea what this is
                let usn: i64 = if let Some(row) = usn_statement.query([]).unwrap().next().unwrap() {
                    row.get(0).unwrap()
                } else {
                    panic!("col table missing");
                };
                // add new
                let mut encode_buffer = Uuid::encode_buffer();
                for n in to_add {
                    // map to field string, nothing else is used
                    let fieldstr = build_field_str(&n.fields);
                    let uuid: &str = Uuid::new_v4().to_simple().encode_lower(&mut encode_buffer);
                    let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as i64;

                    let added_count = insert_note.execute(params![
                        next_note_id,
                        uuid,
                        type_id,
                        time,
                        usn,
                        fieldstr.as_str(),
                        n.fields.get(0).unwrap().as_str(),
                    ]).unwrap();
                    
                    // has to be either 0 or one
                    if added_count > 0 {
                        note_ids.push(NoteId::from(next_note_id));
                    }

                    next_note_id += 1;
                }

                // add updates
                to_update.into_iter()
                    .for_each(|(note_id, n)| {
                        let first_field = n.fields.get(0).unwrap().clone();
                        let fieldstr = build_field_str(&n.fields);
                        let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as i64;

                        let changed_count = update_note.execute(params![
                            time,
                            usn,
                            fieldstr,
                            first_field.as_str(),
                            note_id,
                            fieldstr,
                        ]).unwrap();
                        
                        // has to be either 0 or one
                        if changed_count > 0 {
                            note_ids.push(NoteId::from(note_id));
                        }
                    });
                // these config values are used by after_note_updates
                deck_id = get_deck.query(params![d.name]).unwrap().next()
                    .expect(&format!("Deck {} does not exist", d.name))
                    .expect(&format!("Deck {} does not exist", d.name))
                    .get(0).unwrap();
                set_config.execute(params![
                    format!("_nt_{0}_lastDeck", type_id),
                    usn,
                    TimestampSecs::now(),
                    serde_json::to_vec(&deck_id).unwrap(),
                ]).unwrap();
                let kind_blob: Vec<u8> = get_deck_kind.query(params![deck_id]).unwrap().next()
                    .unwrap()
                    .unwrap()
                    .get(0)
                    .unwrap();
                let kind = DeckKindContainer::decode(kind_blob.as_slice()).unwrap();
                config_id = if let Some(DeckKind::Normal(ref normal)) = kind.kind {
                    Some(normal.config_id)
                } else {
                    None
                }.unwrap();
            }
            // create cards
            collection.after_note_updates(&*note_ids, true, false).unwrap();
            let config = collection.get_deck_config(DeckConfigId::from(config_id), true).unwrap().unwrap();
            if config.inner.new_card_insert_order == NewCardInsertOrder::Random as i32 {
                collection.sort_deck_legacy(DeckId::from(deck_id), true).unwrap();
            }
        }
    }
}

fn build_field_str(fields: &Vec<String>) -> String {
    fields.join("\u{1f}")
}

// TODO: end-to-end tests
