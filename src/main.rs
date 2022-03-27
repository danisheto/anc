use std::{env, fs::{File, self}, io::{self, BufRead}, time::SystemTime};
use anki::{collection::CollectionBuilder, notes::NoteId}; use html_escape::encode_text;
use itertools::{Itertools, Either};

use cards::Deck;
use rusqlite::params;
use serde::Deserialize;
use uuid::Uuid;

use crate::cards::{BasicCard, ClozeCard};

pub mod cards;

fn main() {
    // read/parse from files
    let args: Vec<String> = env::args().collect();
    let dir = &args[1];

    // TODO: filter out non-.qz files
    // TODO: accept list of files instead of a directory
    let paths: Vec<_> = fs::read_dir(dir).unwrap().into_iter()
        .map(|p| p.unwrap().path().display().to_string())
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
            let (basic, cloze): (Vec<Card>, Vec<Card>) = group
                .into_iter()
                .map(|(_, card)| card)
                .partition(|card| card.is_basic());

            Deck::new(
                deck_name.to_string(),
                basic.into_iter().map(|b| b.basic()).collect(),
                cloze.into_iter().map(|c| c.cloze()).collect(),
            )
        })
        .collect();

    // add/update from collection
    process_cards(cards);
}

// TODO:
// - Check for duplicates
// - tags
fn process_cards(decks: Vec<Deck>) {
    let path = "/home/ethans/.local/share/Anki2/Test/collection.anki2";
    let mut note_ids: Vec<NoteId> = vec![];
    {
        let connection = rusqlite::Connection::open(path).unwrap();
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
            let (to_add, to_update): (Vec<_>, Vec<_>) = d.basic.into_iter()
                .partition_map(|card| {
                    if let Some(row) = nid_by_field.query(params![card.id]).unwrap().next().unwrap() {
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
                let fieldstr = format!("{}\u{1f}{}\u{1f}{}", n.id.as_str(), &n.front, &n.back);
                // let fieldstr = buildFieldStr(vec![n.id, n.front, n.back]);
                let uuid: &str = Uuid::new_v4().to_simple().encode_lower(&mut encode_buffer);
                let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as i64;

                let added_count = insert_note.execute(params![
                    next_note_id,
                    uuid,
                    basic_id,
                    time,
                    usn,
                    fieldstr.as_str(),
                    n.id.as_str(),
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
                    let first_field = n.id.clone();
                    let fld = build_field_str(vec![n.id, n.front, n.back]);
                    let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos() as i64;

                    let changed_count = update_note.execute(params![
                        time,
                        usn,
                        fld.as_str(),
                        first_field.as_str(),
                        note_id,
                        fld.as_str(),
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

    // TODO: log
}

fn build_field_str(fields: Vec<String>) -> String {
    fields.join("\u{1f}")
}

#[derive(Deserialize)]
pub struct Frontmatter {
    deck: String,
    r#type: String,
}

// TODO: Instead of explicity mentioning type, reduce into hashmap by notetype
// and apply each field the normal way
#[derive(Debug)]
pub enum Card {
    Basic(BasicCard),
    Cloze(ClozeCard),
}

impl Card {
    pub fn is_basic(&self) -> bool {
        match self {
            Card::Basic(_) => true,
            Card::Cloze(_) => false,
        }
    }
    pub fn is_cloze(&self) -> bool {
        match self {
            Card::Basic(_) => false,
            Card::Cloze(_) => true,
        }
    }
    pub fn basic(self) -> BasicCard {
        match self {
            Card::Basic(b) => b,
            Card::Cloze(_) => panic!("Not a Basic Card!"),
        }
    }
    pub fn cloze(self) -> ClozeCard {
        match self {
            Card::Basic(_) => panic!("Not a Cloze Card!"),
            Card::Cloze(c) => c,
        }
    }
}

fn parse_card(filename: &String) -> Result<(String, Card), &'static str> {
    let file = File::open(filename.clone()).unwrap();
    let mut reader = io::BufReader::new(file);
    let mut buf = String::new();

    // TODO: write helper method for this
    if reader.read_line(&mut buf).is_ok() && buf.len() > 0 && buf.trim() != "---" {
        return Err("missing frontmatter");
    }
    buf.clear();

    let mut yaml: String = "".to_string();
    while reader.read_line(&mut buf).is_ok() && buf.len() > 0 {
        if buf.trim() == "---" {
            buf.clear();
            break;
        }

        yaml += buf.as_str().clone();

        buf.clear();
    }

    let frontmatter: Frontmatter = serde_yaml::from_str(&yaml)
        .map_err(|_| "error parsing frontmatter, only valid yaml is accepted")?;

    if frontmatter.r#type == "basic" {
        let mut front: String = "".to_string();
        while reader.read_line(&mut buf).is_ok() && buf.len() > 0 {
            if buf.trim() == "---" {
                buf.clear();
                break;
            }

            front += buf.as_str().clone();

            buf.clear();
        }
        let mut back: String = "".to_string();
        while reader.read_line(&mut buf).is_ok() && buf.len() > 0 {
            if buf.trim() == "---" {
                buf.clear();
                break;
            }

            back += buf.as_str().clone();

            buf.clear();
        }
        if back.as_str() == "" {
            return Err("The back of the card is missing");
        }
        return Ok((
            frontmatter.deck,
            Card::Basic(
                BasicCard::new(filename.to_string(), plaintext(front), plaintext(back))
            )
        ))
    } else if frontmatter.r#type == "cloze" {
        let mut value: String = "".to_string();
        while reader.read_line(&mut buf).is_ok() && buf.len() > 0 {
            value += buf.as_str().clone();

            buf.clear();
        }
        Ok((
            frontmatter.deck,
            Card::Cloze(
                ClozeCard::new(filename.to_string(), value)
            )
        ))
    } else {
        // run hooks
        Err("Only cloze and basic cards are allowed")
    }
}

fn plaintext(text: String) -> String {
    let stripped = text.trim();
    let encoded = encode_text(stripped);
    let newlines = encoded.replace("\n", "<br/>");
    newlines
}
