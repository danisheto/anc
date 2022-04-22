use std::{env, time::SystemTime, fs};
use anki::{collection::CollectionBuilder, notes::NoteId};
use itertools::{Itertools, Either};

use cards::{Deck, TypeGroup};
use parsing::parse_card;
use rusqlite::params;
use uuid::Uuid;

pub mod cards;
pub mod parsing;

fn main() {
    // read/parse from files
    let args: Vec<String> = env::args().collect();
    let dir = &args[1];

    // TODO: accept list of files instead of a directory
    let paths: Vec<_> = fs::read_dir(dir).unwrap().into_iter()
        .map(|p| p.unwrap().path())
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
            // TODO: exit code 1
            println!("{}: {}", p, r);
        }
        return;
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
    {
        let connection = rusqlite::Connection::open(path)
            .expect("Test collection does not exist");
        // get fields of each notetype - this will only be used in processFields
        // for simplicity, make new request per 
        let mut type_id = connection.prepare(
            "
                SELECT id
                FROM notetypes
                WHERE name like ?
                order by name collate nocase
            ").unwrap();
        let basic_id: i64 = if let Some(row) = type_id.query(params!["basic"]).unwrap().next().unwrap() {
            row.get(0).unwrap()
        } else {
            panic!("Can't find card model");
        };
        // let _cloze_id = if let State::Row = statement.next().unwrap() {
        //     statement.read::<i64>(0).unwrap()
        // } else {
        //     panic!("Can't find card model");
        // };

        let mut nid_by_field = connection.prepare(
            "
                SELECT id
                FROM notes
                WHERE SUBSTR(flds, 0, INSTR(flds, char(31))) like ?
                limit 1
            ").unwrap();
        let mut check_time = connection.prepare("SELECT ifnull(max(id), 0) FROM notes").unwrap();
        let mut usn_statement = connection.prepare("select usn from col").unwrap();
        // TODO: try named parameters instead
        let mut insert_note = connection.prepare("insert or replace into notes values (?, ?, ?, ?, ?, '', ?, ?, 0, 0, '')").unwrap();
        let mut update_note = connection.prepare(
            "update notes set mod = ?, usn = ?, flds = ?, sfld = ?
             where id = ? and flds != ?"
        ).unwrap();

        // TODO: better error handling
        // only import if all work
        for d in decks {
            // compute
            // split into adds and updates
            // TODO: calculating the first field probably requires a full table scan
            // get around this by computing checksums to do a range scan first
            let (to_add, to_update): (Vec<_>, Vec<_>) = d
                .groups.iter()
                    .find(|&g| g.model == "basic")
                    .unwrap()
                    .cards
                .iter()
                .partition_map(|card| {
                    if let Some(row) = nid_by_field.query(params![card.fields.get(0).unwrap()]).unwrap().next().unwrap() {
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
                    basic_id,
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
                })
        }
    }
    // create cards
    let mut collection = CollectionBuilder::new(path).build().unwrap();
    collection.after_note_updates(&*note_ids, true, false).unwrap();
}

fn build_field_str(fields: &Vec<String>) -> String {
    fields.join("\u{1f}")
}
