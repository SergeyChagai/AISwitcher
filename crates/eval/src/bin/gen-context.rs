//! Генератор контекстного подкорпуса из `corpus/context/sentences.txt`.
//!
//! Исходник записан в правильном виде; токены в `[скобках]` перенабираются в
//! противоположной раскладке средствами `layout-map`. Порчу делает код, а не человек,
//! поэтому опечатка в самой транслитерации невозможна.
//!
//! Выход — `corpus/context/cases.txt`: текст в том виде, в каком его «набрал»
//! пользователь, с `{фигурными скобками}` вокруг токенов, где ожидается переключение.
//! Формат выбран так, чтобы его можно было вычитать глазами.

use std::path::Path;

use layout_map::{detect_direction, transliterate};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let dir = root.join("corpus").join("context");

    let source = std::fs::read_to_string(dir.join("sentences.txt"))
        .expect("не прочитан corpus/context/sentences.txt");

    let mut out = String::new();
    out.push_str("# Контекстный подкорпус. СГЕНЕРИРОВАН из sentences.txt — не править вручную.\n");
    out.push_str("# Перегенерация: cargo run -p eval --bin gen-context\n#\n");
    out.push_str("# Текст показан так, как его набрал пользователь.\n");
    out.push_str("# {фигурные скобки} — токен набран не в той раскладке, ожидается переключение.\n");
    out.push_str("# Всё остальное переключать не нужно.\n\n");

    let mut sentences = 0usize;
    let mut switch_tokens = 0usize;
    let mut keep_tokens = 0usize;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            // В результат переносятся только заголовки семейств (`# --- ... ---`):
            // по ним читают корпус. Служебная шапка исходника осталась бы здесь
            // ложной инструкцией — сгенерированный файл правят не так, как исходник.
            if trimmed.starts_with("# ---") {
                out.push('\n');
                out.push_str(trimmed);
                out.push('\n');
            }
            continue;
        }

        let mut rendered = String::new();
        for (i, raw) in trimmed.split_whitespace().enumerate() {
            if i > 0 {
                rendered.push(' ');
            }
            match raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
                Some(token) => {
                    let dir = detect_direction(token).unwrap_or_else(|| {
                        panic!("не определено направление для [{token}] в строке: {trimmed}")
                    });
                    let mistyped = transliterate(token, dir).unwrap_or_else(|| {
                        panic!("[{token}] не отображается целиком: уберите скобки или токен")
                    });
                    rendered.push('{');
                    rendered.push_str(&mistyped);
                    rendered.push('}');
                    switch_tokens += 1;
                }
                None => {
                    rendered.push_str(raw);
                    keep_tokens += 1;
                }
            }
        }

        out.push_str(&rendered);
        out.push('\n');
        sentences += 1;
    }

    let path = dir.join("cases.txt");
    std::fs::write(&path, out).expect("не записан cases.txt");

    println!("{}", path.display());
    println!(
        "предложений: {sentences}, ожидается switch: {switch_tokens}, keep: {keep_tokens}"
    );
}
