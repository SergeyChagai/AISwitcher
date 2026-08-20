#!/usr/bin/env bash
# Подкорпус живой речи: русские субтитры из OPUS OpenSubtitles v2018.
#
# Зачем: собственная документация даёт только письменный технический русский.
# Разговорной речи, коротких реплик и сленга в ней нет, а продукт работает
# в первую очередь в переписке.
#
# Полный ru.txt.gz — 655 МБ (около 2.5 ГБ текста). Столько не нужно: модель
# насыщается на порядки меньшем объёме. Поэтому берётся диапазонный запрос
# первых нескольких мегабайт, а хвост обрезанного gzip-потока отбрасывается.
#
# Результат НЕ коммитится (см. .gitignore): это чужой текст под своей лицензией.
# В репозитории живёт только этот скрипт — корпус воспроизводится командой.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
URL="https://object.pouta.csc.fi/OPUS-OpenSubtitles/v2018/mono/ru.txt.gz"
BYTES="${1:-8000000}"

DEST_DIR="$ROOT/corpus/ru-speech"
mkdir -p "$DEST_DIR"
RAW="$DEST_DIR/opensubtitles.txt"

echo "Источник: $URL"
echo "Забираем первые $BYTES байт сжатого потока..."

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

curl -sS --fail --max-time 300 -r "0-$((BYTES - 1))" "$URL" -o "$TMP"

# gunzip упрётся в обрыв потока и вернёт ненулевой код — это ожидаемо.
# Всё, что он успел распаковать до обрыва, корректно; последнюю строку
# отбрасываем, она может быть оборвана на середине символа.
gunzip -c "$TMP" 2>/dev/null | head -n -1 > "$RAW" || true

if [ ! -s "$RAW" ]; then
    echo "не удалось распаковать ни одной строки" >&2
    exit 1
fi

lines="$(wc -l < "$RAW")"
size="$(wc -c < "$RAW")"
printf 'corpus/ru-speech/opensubtitles.txt: %s строк, %s байт\n' "$lines" "$size"

if ! iconv -f UTF-8 -t UTF-8 "$RAW" >/dev/null 2>&1; then
    echo "предупреждение: файл содержит невалидный UTF-8" >&2
fi
