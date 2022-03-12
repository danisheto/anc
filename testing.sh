#!/bin/zsh
# Add hooks throughout?

# check that deck exists, otherwise prompt
if test -z $1; then
  echo "Provide a deck"
  exit 1
fi
# check that type is cloze or basic
if test -z $2; then
  echo "Provide a type: cloze or basic"
  exit 1
fi
# [ $2 != "cloze" ]
# IS_CLOZE=$?
# [ $2 != "basic" ]
# IS_BASIC=$?
# echo $IS_CLOZE
# echo $IS_BASIC
if test $2 != "cloze" && test $2 != "basic"; then
  echo "Only basic or cloze types are allowed"
  exit 1
fi
# if name isn't provided, create name based on deck name

# check for existence of file
FILE=$3
INDEX=0
while test -z $FILE; do
  INDEX=$(($INDEX+1))
  if ! test -f "$1$INDEX.qz"; then
    FILE="$1$INDEX.qz"
  fi
done
if test ${FILE##*.} != "qz"; then
  FILE="${FILE%.*}.qz"
fi

TEMPLATE=$(
cat <<- EOF
---
Deck: $1
Type: $2
---
$(
if test $2 = "cloze"; then
  echo "{{c1::}}"
fi
)$(
if test $2 = "basic"; then
cat <<-BSC
Front
---
Basic
BSC
fi
)
EOF
)

# TEMPLATE=env DECK_NAME=$1 DECK_TYPE=$2 IS_CLOZE=$IS_CLOZE IS_BASIC=$IS_BASIC mo new_card.mo
echo "$TEMPLATE" > $FILE

${EDITOR:-vi} $FILE +5

# test to make sure file still exists
if ! test -f $FILE; then
  echo "Card wasn't created, don't delete the new file in ${EDITOR:-vi}"
  exit 1
fi

# compare value with diff, otherwise
# difference=$(diff $FILE <(echo $TEMPLATE))
if test -z "$(diff $FILE <(echo "$TEMPLATE"))"; then
  echo "Card wasn't created since no changes were detected"
  rm $FILE
  exit 1
fi

# parse frontmatter
DELIMITERS=$(grep -n -- "---" $FILE | cut -d: -f1 | head -2)
if test $(wc -l <(echo "$DELIMITERS") | cut -d' ' -f1) -lt 2; then
  echo "Frontmatter is missing"
  # reopen
  exit 1
fi
INDEX1=$(($(echo "$DELIMITERS" | sed "1q;d")+1))
INDEX2=$(($(echo "$DELIMITERS" | sed "2q;d")-1))
# sed "${INDEX1},${INDEX2}p;$(($INDEX2+1))" $FILE
# echo "front"
FRONTMATTER=$(sed -n "${INDEX1},${INDEX2}p;$(($INDEX2+1))q" $FILE)
# echo "$FRONTMATTER"
TYPE=$(echo "$FRONTMATTER" | yq .Type)
DECK=$(echo "$FRONTMATTER" | yq .Deck)
# parse card

# cat $FILE
