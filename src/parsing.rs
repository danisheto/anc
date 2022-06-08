use std::{io::{self, BufRead, Read, Write}, fs::File, path::PathBuf, process::{Command, Stdio, ChildStdout}, thread};

use itertools::Itertools;
use html_escape::encode_text;
use serde::Deserialize;

use crate::cards::{Card, TypeGroup, Deck};

pub struct BatchReader<T> where T: Read {
    readers: Vec<(Option<String>, io::BufReader<T>)>,
}

impl<T> BatchReader<T> where T: Read {
    pub fn parse(self) -> Result<Vec<Deck>, Vec<String>> {
        let (cards, card_errors): (Vec<_>, Vec<_>) = self.readers.into_iter()
            .map(|(id, p)| parse(p, id))
            .partition(|result| result.is_ok());

        let errors: Vec<_> = card_errors.into_iter()
            .map(|r| r.unwrap_err())
            .collect();

        if errors.len() > 0 {
            return Err(errors.into_iter().flatten().collect());
        }

        Ok(cards.into_iter()
            .map(|r| r.unwrap())
            .flatten()
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
                        .map(|(id, card)| (id, card.as_bytes()))
                        .map(|(id, bytes)| (Some(id), io::BufReader::new(bytes)))
                        .collect(),
        }
    }
}

impl BatchReader<ChildStdout> {
    pub fn from_stdout(input: ChildStdout) -> BatchReader<ChildStdout> {
        BatchReader {
            readers: vec![(None, io::BufReader::new(input))],
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
                            (Some(p.display().to_string()), io::BufReader::new(file))
                        })
                        .collect(),
        }
    }
}

pub fn parse_files(config_dir: PathBuf, paths: Vec<PathBuf>) -> Result<Vec<Deck>, Vec<String>> {
    let path = config_dir.join("hooks/pre-parse");
    if path.exists() {
        let mut process = Command::new(path.display().to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        thread::spawn(move || {
            let mut stdin = process.stdin.take().unwrap();
            for p in paths {
                stdin.write(p.display().to_string().as_bytes()).unwrap();
            }
        });
        let output = process.stdout.take().unwrap();
        BatchReader::from_stdout(output)
            .parse()
    } else {
        BatchReader::from_files(paths)
            .parse()
    }
}

pub fn parse_from_file(filename: &str) -> Result<Vec<(String, Card)>, Vec<String>> {
    let file = File::open(filename)
        .map_err(|_| vec![format!("Could not open {}", filename)])?;
    let reader = io::BufReader::new(file);
    parse(reader, Some(filename.to_string()))
}

#[derive(Deserialize)]
pub struct Frontmatter {
    deck: String,
    r#type: String,
    id: Option<String>,
    tags: Option<String>,
    html: Option<bool>,
}

pub fn parse<T>(
    reader: io::BufReader<T>,
    id: Option<String>
) -> Result<Vec<(String, Card)>, Vec<String>>
where T: Read
{
    let lines = reader.lines();

    let (cards, errors) = lines.into_iter()
        .map(|l| l.unwrap())
        .fold(vec![vec![]], |mut cards: Vec<Vec<String>>, l| {
            if l.trim() == "---" {
                let last_note = cards.last_mut().unwrap();
                last_note.push("".to_string());
            } else if l.trim() == "###" {
                cards.push(Vec::new());
            } else {
                let last_note = cards.last_mut().unwrap();
                let last_part = last_note.last_mut().unwrap();
                *last_part += &l;
                *last_part += "\n";
            }
            cards
        })
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            if n.len() == 0 { return Err("empty card".to_string()); }
            let frontmatter: Frontmatter = {
                let val = n.get(0).unwrap();
                let yaml = serde_yaml::from_str(val);
                if let Err(e) = yaml { return Err(format!("error parsing frontmatter: {}", e)) };
                yaml.unwrap()
            };
            let note_id = {
                let i = frontmatter.id.or(id.clone().map(|f| format!("{}#{}", f, i + 1)));
                if i.is_none() { return Err("An id is required as part of the frontmatter".to_string())}
                i.unwrap()
            };
            let mut parts = {
                if frontmatter.html.unwrap_or(false) {
                    n.into_iter().skip(1)
                        .map(|p| plaintext(p))
                        .collect()
                } else {
                    n.into_iter().skip(1)
                        .map(|p| p.trim().to_string())
                        .collect()
                }
            };
            let mut fields = vec![note_id];
            fields.append(&mut parts);
            Ok((
                frontmatter.deck,
                Card::new(
                    frontmatter.r#type,
                    fields,
                    frontmatter.tags,
                )
            ))
        })
        .partition::<Vec<Result<(String, Card), String>>, _>(Result::is_ok);

    if errors.is_empty() {
        Ok(
            cards.into_iter()
                .map(|c| c.unwrap())
                .collect()
        )
    } else {
        Err(
            errors.into_iter()
                .map(|e| e.unwrap_err())
                .collect()
        )
    }
}

fn plaintext(text: String) -> String {
    let stripped = text.trim();
    let encoded = encode_text(stripped);
    encoded.replace("\n", "<br/>")
}

#[test]
fn basic() {
    let result = parse_from_file("test_files/good/basic.qz");

    assert!(result.is_ok(), "Errors: {:?}", result.unwrap_err());
    let mut output = result.unwrap();
    assert!(output.len() == 1, "More than one card found");

    let (deck, card) = output.remove(0);
    assert_eq!(deck, "example");
    assert_eq!(
        card,
        Card {
            model: "basic".to_string(),
            fields: vec!["test_files/good/basic.qz#1".to_string(), "Question".to_string(), "Answer".to_string()],
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
