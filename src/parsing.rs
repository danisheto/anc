use std::{io::{self, BufRead}, fs::File};

use html_escape::encode_text;

use crate::{cards::Card, Frontmatter};

// TODO: remove knowledge about types
// pull from anki and check types
pub fn parse_card(filename: &String) -> Result<(String, Card), String> {
    let file = File::open(filename.clone())
        .map_err(|_| format!("Could not open {}", filename))?;
    let mut reader = io::BufReader::new(file);
    let mut buf = String::new();

    // TODO: write helper method for this
    if reader.read_line(&mut buf).is_ok() && buf.len() > 0 && buf.trim() != "---" {
        return Err("missing frontmatter".to_string());
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
        .map_err(|e| format!("error parsing frontmatter: {}", e) )?;

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
            return Err("The back of the card is missing".to_string());
        }
        return Ok((
            frontmatter.deck,
            Card::new(
                String::from("basic"),
                vec![filename.to_string(), plaintext(front), plaintext(back)]
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
            Card::new(
                String::from("cloze"),
                vec![filename.to_string(), value]
            )
        ))
    } else {
        // run hooks
        Err("Only cloze and basic cards are allowed".to_string())
    }
}

fn plaintext(text: String) -> String {
    let stripped = text.trim();
    let encoded = encode_text(stripped);
    encoded.replace("\n", "<br/>")
}

#[test]
fn basic() {
    let result = parse_card(&"test_files/basic.qz".to_string());

    assert!(result.is_ok(), "{}", result.unwrap_err());

    let (deck, card) = result.unwrap();
    assert_eq!(deck, "example");
    assert_eq!(
        card,
        Card {
            model: "basic".to_string(),
            fields: vec!["test_files/basic.qz".to_string(), "Question".to_string(), "Answer".to_string()]
        }
    )
}

#[test]
fn nonexistent() {
    let result = parse_card(&"test_files/nonexistent_basic".to_string());

    assert!(result.is_err(), "The nonexistent test case exists");
}
#[test]
fn bad_frontmatter() {
    let result = parse_card(&"test_files/bad_frontmatter".to_string());

    assert!(result.is_err(), "Bad frontmatter is allowed");
}
