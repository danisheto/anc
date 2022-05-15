use std::env;

use anc::run;

fn main() {
    let args: Vec<String> = env::args().collect();
    let dir = &args[1];

    let path = env::var("TEST_ANKI").expect("For testing, need a $TEST_ANKI");

    run(dir, path);
}
