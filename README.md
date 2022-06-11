# Anc

`anc` allows adding and updating anki notes from local text files.

## Features
* Sync anki from the command line
* Create and update notes without opening anki
* Hook into card creation/updates

## How to Use
Note that `anc` uses the first field of each card in anki to uniquely identify it.
Initialize an existing directory with `anc init`. Then set `$ANKI_DIR` to the Anki directory containing the `collection.anki2`. Alternatively, set `directory` in the newly created `.anc/config` file.
Then create a new file `test.qz` in the directory containing `.anc`:
```
---
# YAML Frontmatter
deck: example
type: basic
tags: test test2 test3 # Optional, whitespace delimited
html: true # Optional, defaults to false
---
Chemical Symbol for <b>Oxygen</b>?
---
<b>O</b>
```
Running `anc save` in this directory or any subdirectories will add a new basic card with three fields: `test.qz`, `Chemical Symbol for <b>Oxygen</b>` and `<b>O</b>`.
To sync anki to ankiweb, run `anc sync`. This requires already having signed in and synced at least once.

## Hooks
To change how files are saved to anki, a `pre-parse` script can be placed in `.anc/hooks`. It accepts as stdin a newline-delimited list of absolute paths and should returns as stdout multiple notes as above with `\n###\n` in between. Once difference in the card format is a new `id` field is required. It's expected that it looks like `$path#1`, but as long as it's creation is the same every time and unique between notes, anything goes.
