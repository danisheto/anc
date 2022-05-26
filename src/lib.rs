use std::{fs, collections::HashMap, time::SystemTime, path::{PathBuf, Path}, env};

use anki::{notes::NoteId, collection::CollectionBuilder, timestamp::TimestampSecs, decks::{DeckKindContainer, DeckKind, DeckId}, prelude::DeckConfigId, deckconfig::NewCardInsertOrder};
use itertools::{Either, Itertools};
use prost::Message;
use rusqlite::params;
use serde::Deserialize;
use tfio::{Transaction, RollbackableOperation};
use uuid::Uuid;

pub mod cards;
pub mod parsing;

use parsing::parse_files;
use cards::Deck;

pub fn init() -> Result<(), ()> {
    let mut tran = Transaction::new()
        .create_dir("./.anc")
        .create_dir("./.anc/hooks")
        .create_file("./.anc/config")
        .write_file("./.anc/config", "/tmp", b"# anki_dir = \"~/.local/share/Anki2/User 1\"\n".to_vec());
    match tran.execute() {
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("Error creating .anc directory");
            if let Err(_) = tran.rollback() {
                eprintln!("Error undoing failure");
            }
            Err(())
        },
        Ok(_) => {
            Ok(())
        }
    }
}

#[derive(Deserialize)]
struct Config {
    anki_dir: Option<PathBuf>,
}

struct AllConfiguration {
    config_dir: PathBuf,
    anki_dir: PathBuf,
}

fn get_config() -> Result<AllConfiguration, &'static str> {
    let config_dir = search_for_config();
    if config_dir.is_none() {
        return Err("Not an anc directory. Initialize first.");
    }

    let anki_dir = fs::read_to_string(config_dir.as_ref().unwrap().join("config"))
        .map_or(None, |c| {
            let config: Config = toml::from_str(&c).unwrap();
            config.anki_dir
        })
        .or({
            env::var("ANKI_DIR") 
                .map_or(None, |ad| Some(PathBuf::from(ad)))
        })
        .expect("Set anki_dir in .anc/config or set $ANKI_DIR");

    Ok(AllConfiguration {
        config_dir: config_dir.unwrap(),
        anki_dir,
    })
}

fn search_for_config() -> Option<PathBuf> {
    find_config(Path::new(".").to_path_buf().canonicalize().unwrap())
}

fn find_config(mut path: PathBuf) -> Option<PathBuf> {
    let target = path.join(".anc");
    if target.is_dir() {
        Some(target)
    } else if path.pop() {
        find_config(path)
    } else {
        None
    }
}

pub fn run() {
    let config = get_config().unwrap();

    let paths = find_files(&config.config_dir, "qz");

    let cards = match parse_files(config.config_dir, paths) {
        Err(errors) => {
            for (r, p) in errors {
                eprintln!("{}: {}", p, r);
            }
            std::process::exit(65);
        },
        Ok(c) => c,
    };

    // add/update from collection
    process_cards(config.anki_dir.join("collection.anki2"), cards);

    // TODO: log
}
fn find_files(config_dir: &PathBuf, extension: &str) -> Vec<PathBuf> {
    let base_dir = config_dir.parent().unwrap().to_path_buf();
    let mut to_check = vec![base_dir];
    let mut paths = vec![];
    // TODO: use .gitignore
    while let Some(dir) = to_check.pop() {
        for pr in fs::read_dir(dir).unwrap().into_iter() {
            if let Ok(p) = pr {
                let canon = p.path().canonicalize().unwrap();
                if canon.is_dir() && canon.file_name().map(|e| e.to_str()).flatten() != Some(".anc") {
                    to_check.push(p.path());
                } else if canon.is_file() && canon.extension().map(|e| e.to_str()).flatten() == Some(extension) {
                    paths.push(canon);
                }
            }
        }
    }
    paths
}

// TODO:
// - Check for duplicates
pub fn process_cards(path: PathBuf, decks: Vec<Deck>) {
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
                     where id = ?"
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
