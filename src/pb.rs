pub mod pb {
    pub mod generic {
        include!(concat!(env!("OUT_DIR"), "/anki.generic.rs"));
    }
    pub mod links {
        include!(concat!(env!("OUT_DIR"), "/anki.links.rs"));
    }
    pub mod backend {
        include!(concat!(env!("OUT_DIR"), "/anki.backend.rs"));
    }
    pub mod sync {
        include!(concat!(env!("OUT_DIR"), "/anki.sync.rs"));
    }
}
