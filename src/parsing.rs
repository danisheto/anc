use std::{io::{self, BufRead}, fs::File};

use html_escape::encode_text;
use serde::Deserialize;

use crate::cards::Card;

#[derive(Deserialize)]
pub struct Frontmatter {
    deck: String,
    r#type: String,
}

// TODO: remove knowledge about types
// pull from anki and check types
// TODO: allow for more than one question per file
pub fn parse_card(filename: &String) -> Result<(String, Card), String> {
    let file = File::open(filename.clone())
        .map_err(|_| format!("Could not open {}", filename))?;
    let reader = io::BufReader::new(file);
    let mut lines = reader.lines();

    if let Some(Ok(l)) = lines.next() {
        if l.trim() != "---" {
            return Err("missing frontmatter".to_string());
        }
    }

    let mut yaml: String = "".to_string();
    while let Some(Ok(l)) = lines.next() {
        if l.trim() == "---" {
            break;
        }

        yaml += l.as_str();
        yaml += "\n";
    }

    let frontmatter: Frontmatter = serde_yaml::from_str(&yaml)
        .map_err(|e| format!("error parsing frontmatter: {}", e) )?;

    let fields = {
        let mut parts = vec!("".to_string());
        let mut index = 0;
        let mut last = parts.get_mut(0).unwrap();
        while let Some(Ok(l)) = lines.next() {
            if l.trim() == "---" {
                parts.push("".to_string());
                index += 1;
                last = parts.get_mut(index).unwrap();
                continue;
            }
            *last += l.as_str();
            *last += "\n";
        }

        parts = parts.into_iter()
            .map(|p| plaintext(p))
            .collect();

        parts.insert(0, filename.to_string());
        parts
    };

    return Ok((
        frontmatter.deck,
        Card::new(
            frontmatter.r#type,
            fields,
        )
    ))
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
