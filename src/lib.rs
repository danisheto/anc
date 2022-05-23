use std::{fs, collections::HashMap, time::SystemTime};

use anki::{notes::NoteId, collection::CollectionBuilder, timestamp::TimestampSecs, decks::{DeckKindContainer, DeckKind, DeckId}, prelude::DeckConfigId, deckconfig::NewCardInsertOrder};
use itertools::{Either, Itertools};
use prost::Message;
use rusqlite::params;
use uuid::Uuid;

pub mod cards;
pub mod parsing;

use parsing::BatchReader;
use cards::Deck;

pub fn run(directory: &str, path: String) {
    // TODO: accept list of files instead of a directory
    let paths: Vec<_> = fs::read_dir(directory).unwrap().into_iter()
        .map(|p| p.unwrap().path().canonicalize().unwrap())
        .filter(|p| p.is_file() && 
            Some("qz") == p.extension()
                .map(|e| e.to_str())
                .flatten()
        )
        .collect();


    let cards = match BatchReader::from_files(paths).parse() {
        Err(errors) => {
            for (r, p) in errors {
                eprintln!("{}: {}", p, r);
            }
            std::process::exit(65);
        },
        Ok(c) => c,
    };

    // add/update from collection
    process_cards(&path, cards);

    // TODO: log
}

// TODO:
// - Check for duplicates
pub fn process_cards(path: &str, decks: Vec<Deck>) {
    let mut note_ids: Vec<NoteId> = vec![];
    let mut collection = CollectionBuilder::new(path).build().unwrap();
    {
        collection.storage.db.prepare("savepoint anc").unwrap().execute([]).unwrap();
    }
    for d in decks {
        for g in d.groups {
            let deck_id: i64;
            let config_id: i64;
            {
                let mut type_ids = HashMap::new();
                let mut type_query = collection.storage.db.prepare(
                    "
                        SELECT nt.id, count(*)
                        FROM notetypes nt
                        join fields fd on fd.ntid = nt.id
                        WHERE nt.name like ?
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
                let mut insert_note = collection.storage.db.prepare("insert or replace into notes values (?, ?, ?, ?, ?, ?, ?, ?, 0, 0, '')").unwrap();
                let mut update_note = collection.storage.db.prepare(
                    "update notes set mod = ?, usn = ?, tags = ?, flds = ?, sfld = ?
                     where id = ? and flds != ?"
                ).unwrap();
                let mut get_deck = collection.storage.db.prepare("select id from decks where name like ?").unwrap();
                let mut get_deck_kind = collection.storage.db.prepare("select kind from decks where id = ?").unwrap();
                let mut set_config = collection.storage.db.prepare("insert or replace into config (key, usn, mtime_secs, val) values (?, ?, ?, ?)").unwrap();

                let (type_id, field_count) = if let Some(&(id, amount)) = type_ids.get(&g.model) {
                    (id, amount)
                } else {
                    let (id, amount) = type_query.query(params![g.model])
                        .unwrap()
                        .mapped(|row| Ok((
                            row.get::<usize, i64>(0).expect("Can't find card model"),
                            row.get::<usize, usize>(1).unwrap(),
                        )))
                        .next()
                        .unwrap()
                        .expect("Can't find card model");
                    type_ids.insert(g.model, (id, amount));
                    (id, amount)
                };

                // split into adds and updates
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
                    let fieldstr = build_field_str(&n.fields, field_count, n.fields.len());
                    let uuid: &str = Uuid::new_v4().to_simple().encode_lower(&mut encode_buffer);
                    let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as i64;

                    let added_count = insert_note.execute(params![
                        next_note_id,
                        uuid,
                        type_id,
                        time,
                        usn,
                        n.tags.as_ref().map(|t| format!(" {} ", t.trim())).unwrap_or(" ".to_string()),
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
                        let fieldstr = build_field_str(&n.fields, field_count, n.fields.len());
                        let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as i64;

                        let changed_count = update_note.execute(params![
                            time,
                            usn,
                            n.tags.as_ref().map(|t| format!(" {} ", t.trim())).unwrap_or(" ".to_string()),
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
    {
        collection.storage.db.prepare("release anc").unwrap().execute([]).unwrap(); // commit
    }
}

fn build_field_str(fields: &Vec<String>, model_field_count: usize, fields_entered_count: usize) -> String {
    let pad = model_field_count.checked_sub(fields_entered_count).unwrap_or(0);
    format!("{}{}", fields.join("\u{1f}"), "\u{1f}".repeat(pad))
}
