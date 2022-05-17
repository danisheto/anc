use std::{io::{self, BufRead, Read}, fs::File, path::PathBuf};

use itertools::Itertools;
use html_escape::encode_text;
use serde::Deserialize;

use crate::cards::{Card, TypeGroup, Deck};

pub struct BatchReader<T> where T: Read {
    readers: Vec<(String, io::BufReader<T>)>,
}

impl<T> BatchReader<T> where T: Read {
    pub fn parse(self) -> Result<Vec<Deck>, Vec<(String, String)>> {
        let (cards, card_errors): (Vec<_>, Vec<_>) = self.readers.into_iter()
            .map(|(id, p)| (id.clone(), parse_card(p, &id)))
            .partition(|(_id, result)| result.is_ok());

        let errors: Vec<_> = card_errors.into_iter()
            .map(|(id, r)| (id, r.unwrap_err()))
            .collect();

        if errors.len() > 0 {
            return Err(errors);
        }

        Ok(cards.into_iter()
            .map(|(_, r)| r.unwrap())
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
            .collect())
    }
}

impl BatchReader<&[u8]> {
    pub fn from_string(inputs: Vec<(String, &str)>) -> BatchReader<&[u8]> {
        BatchReader {
            readers: inputs.into_iter()
                        .map(|(id, input)| (id, input.as_bytes()))
                        .map(|(id, bytes)| (id, io::BufReader::new(bytes)))
                        .collect(),
        }
    }
}

impl BatchReader<File> {
    pub fn from_files(paths: Vec<PathBuf>) -> BatchReader<File> {
        BatchReader {
            readers: paths.into_iter()
                        .map(|p| {
                            let file = File::open(p.clone())
                                .expect(format!("Could not open {:?}", p).as_str());
                            (p.display().to_string(), io::BufReader::new(file))
                        })
                        .collect(),
        }
    }
}

pub fn parse_files(paths: Vec<PathBuf>) -> Result<Vec<Deck>, Vec<(String, String)>> {
    BatchReader::from_files(paths)
        .parse()
}

pub fn parse_from_file(filename: &str) -> Result<(String, Card), String> {
    let file = File::open(filename)
        .map_err(|_| format!("Could not open {}", filename))?;
    let reader = io::BufReader::new(file);
    parse_card(reader, filename)
}

#[derive(Deserialize)]
pub struct Frontmatter {
    deck: String,
    r#type: String,
    tags: Option<String>,
}

// TODO: allow for more than one question per file
pub fn parse_card<T>(
    reader: io::BufReader<T>,
    id: &str
) -> Result<(String, Card), String> 
where T: Read
{
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

        parts.insert(0, id.to_string());
        parts
    };

    Ok((
        frontmatter.deck,
        Card::new(
            frontmatter.r#type,
            fields,
            frontmatter.tags,
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
    let result = parse_from_file("test_files/good/basic.qz");

    assert!(result.is_ok(), "{}", result.unwrap_err());

    let (deck, card) = result.unwrap();
    assert_eq!(deck, "example");
    assert_eq!(
        card,
        Card {
            model: "basic".to_string(),
            fields: vec!["test_files/good/basic.qz".to_string(), "Question".to_string(), "Answer".to_string()],
            tags: Some("example2 example3".to_string())
        }
    )
}

#[test]
fn nonexistent() {
    let result = parse_from_file("test_files/nonexistent_basic");

    assert!(result.is_err(), "The nonexistent test case exists");
}
#[test]
fn bad_frontmatter() {
    let result = parse_from_file("test_files/bad_frontmatter");

    assert!(result.is_err(), "Bad frontmatter is allowed");
}
