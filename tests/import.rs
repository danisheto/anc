use anc::run;
use rusqlite::params;

use std::{sync::Once, fs::{copy, remove_file}, panic};

use anki::{collection::CollectionBuilder, decks::NativeDeckName, notetype::{NoteField, NoteFieldConfig}};

#[macro_use]
extern crate macro_rules_attribute;

// 1. create new .anki2 ONCE
// 2. copy new
// 3. teardown

static BEGIN: Once = Once::new();
static END: Once = Once::new();

macro_rules! import_test {(
    fn $fname:ident ()
    $body: block
) => {
    #[test]
    fn $fname () {
        fn __original_func__ ()
        $body

        BEGIN.call_once(|| {
            let mut collection = CollectionBuilder::new("temp.anki2").build().unwrap();
            let mut deck = anki::decks::Deck::new_normal();
            deck.name = NativeDeckName::from_human_name("example");
            collection.add_deck(&mut deck).unwrap();
            let mut basic_notetype = (*collection.get_notetype_by_name("basic").unwrap().unwrap()).clone();
            let basic_id_field = NoteField {
                ord: None,
                name: "Id".to_string(),
                config: NoteFieldConfig { 
                    sticky: false,
                    rtl: false,
                    font_name: "Liberation Sans".to_string(),
                    font_size: 20,
                    description: "".to_string(),
                    other: vec![],
                }
            };
            basic_notetype.fields.insert(0, basic_id_field);
            collection.update_notetype(&mut basic_notetype, false).unwrap();

            let mut cloze_notetype = (*collection.get_notetype_by_name("cloze").unwrap().unwrap()).clone();
            let cloze_id_field = NoteField {
                ord: None,
                name: "Id".to_string(),
                config: NoteFieldConfig { 
                    sticky: false,
                    rtl: false,
                    font_name: "Liberation Sans".to_string(),
                    font_size: 20,
                    description: "".to_string(),
                    other: vec![],
                }
            };
            cloze_notetype.fields.insert(0, cloze_id_field);
            collection.update_notetype(&mut cloze_notetype, false).unwrap();
        });
        let file_name = format!("{}.anki2", stringify!($fname));
        copy("temp.anki2", &file_name).unwrap();
        let result = panic::catch_unwind(|| {
            __original_func__();
        });
        remove_file(&file_name).unwrap();
        END.call_once(|| {
            remove_file("temp.anki2").unwrap();
        });
        if let Err(e) = result {
            panic!("{:?}", e);
        }
    }
}}

#[macro_rules_attribute(import_test)]
fn basic() {
    run("test_files/good", "basic.anki2".to_string());

    let collection = CollectionBuilder::new("basic.anki2").build().unwrap();
    let conn = collection.storage.db;
    let mut count_query = conn.prepare("
        select count()
        from cards c
        join decks d
        on c.did = d.id
        where d.name like ?
    ").unwrap();

    assert_eq!(count_query.query(params!["example"]).unwrap().next().unwrap().unwrap().get::<usize, i32>(0).unwrap(), 4);
}
