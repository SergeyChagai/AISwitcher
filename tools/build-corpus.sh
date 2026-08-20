#!/usr/bin/env bash
# Сборка корпуса для Sprint 1 (docs/research/model-evaluation.md).
#
# Источники — markdown-документация собственных проектов. Из неё извлекается
# поток токенов, разделённый по алфавиту и по признаку «код или проза»:
#
#   corpus/ru/harvested.txt    кириллические токены из прозы
#   corpus/en/harvested.txt    латинские токены из прозы
#   corpus/tech/harvested.txt  латинские токены из кода и inline-code
#
# corpus/terminal/ ведётся вручную: команды, а не документация.
#
# Прозой считается всё вне ``` -фенсов; inline `code` вырезается в поток кода.
# Прозаический текст не копируется — сохраняется только поток слов.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

SOURCES=(
    "$ROOT/docs"
    "C:/Users/freim/source/repos/meronq/docs"
    "F:/Unity/Savannah/docs"
    "F:/Unity/AbilityArena/docs"
    "F:/Roblox/IdleEconomy/docs"
    "F:/Unity/Savannah/Assets/Scripts/TechArt/PapaRomaTools/Documentation"
    "F:/Unity/Savannah/Assets/Scripts/CustomElements/UIEffects/Docs"
)

PROSE="$(mktemp)"
CODE="$(mktemp)"
trap 'rm -f "$PROSE" "$CODE"' EXIT

for dir in "${SOURCES[@]}"; do
    [ -d "$dir" ] || { echo "пропуск (нет каталога): $dir" >&2; continue; }
    while IFS= read -r file; do
        awk -v prose="$PROSE" -v code="$CODE" '
            /^[[:space:]]*```/ { fence = !fence; next }
            {
                line = $0
                if (fence) { print line >> code; next }
                # inline `code` уходит в поток кода, остальное — в прозу
                while (match(line, /`[^`]*`/)) {
                    span = substr(line, RSTART + 1, RLENGTH - 2)
                    print span >> code
                    line = substr(line, 1, RSTART - 1) " " substr(line, RSTART + RLENGTH)
                }
                print line >> prose
            }
        ' "$file"
    done < <(find "$dir" -name '*.md' -not -path '*/node_modules/*')
done

mkdir -p "$ROOT/corpus/ru" "$ROOT/corpus/en" "$ROOT/corpus/tech"

# Извлечение токенов делает perl с явным UTF-8 (-CSD), а не grep/tr.
# В C-локали и grep, и tr работают побайтово: класс [а-яёА-ЯЁ] превращается в
# набор байтов и рвёт многобайтовую кириллицу на куски. Результат — файл,
# который не является валидным UTF-8.
# Регистр не понижается здесь: это делает токенизатор в crates/ngram.
perl -CSD -ne 'print "$1\n" while /(\p{Cyrillic}+)/g' "$PROSE" > "$ROOT/corpus/ru/harvested.txt"
perl -CSD -ne 'print "$1\n" while /([A-Za-z]+)/g'     "$PROSE" > "$ROOT/corpus/en/harvested.txt"
perl -CSD -ne 'print "$1\n" while /([A-Za-z]+)/g'     "$CODE"  > "$ROOT/corpus/tech/harvested.txt"

for f in ru/harvested.txt en/harvested.txt tech/harvested.txt; do
    printf '%-24s %8d токенов\n' "$f" "$(wc -l < "$ROOT/corpus/$f")"
done
