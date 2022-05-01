pub struct Deck {
    pub name: String,
    pub groups: Vec<TypeGroup>,
}

impl Deck {
    pub fn new(name: String, groups: Vec<TypeGroup>) -> Deck {
        Deck {
            name,
            groups,
        }
    }
}

pub struct TypeGroup {
    pub model: String,
    pub cards: Vec<Card>,
}

#[derive(Debug, PartialEq)]
pub struct Card {
    pub model: String,
    pub fields: Vec<String>, // The first is assumed to be the id
}

impl Card {
    pub fn new(model: String, fields: Vec<String>) -> Card {
        Card {
            model,
            fields
        }
    }
}

