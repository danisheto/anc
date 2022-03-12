pub struct Deck {
    pub name: String,
    pub basic: Vec<BasicCard>,
    pub cloze: Vec<ClozeCard>,
}

impl Deck {
    pub fn new(name: String, basic: Vec<BasicCard>, cloze: Vec<ClozeCard>) -> Deck {
        Deck {
            name,
            basic,
            cloze,
        }
    }
}

#[derive(Debug)]
pub struct BasicCard {
    pub id: String,
    pub front: String,
    pub back: String,
}

impl BasicCard {
    pub fn new(filename: String, front: String, back: String) -> BasicCard {
        BasicCard {
            id: filename, // TODO: remove .qz at the end
            front,
            back,
        }
    }
}

#[derive(Debug)]
pub struct ClozeCard {
    id: String,
    value: String,
}

impl ClozeCard {
    pub fn new(filename: String, value: String) -> ClozeCard {
        ClozeCard {
            id: filename, // TODO: remove .qz at the end
            value,
        }
    }
}
