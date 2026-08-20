//! Поиск коллизий раскладки в имеющихся корпусах.
//!
//! Коллизия — набор нажатий, осмысленный в обеих раскладках: русское слово, чья
//! транслитерация является английским словом или техническим токеном. Ровно на них
//! Tier 0 не может решить без контекста (семейства E и H контекстного подкорпуса).
//!
//! Подбирать такие пары руками ненадёжно — легко заявить пару, которой не существует.
//! Здесь они выводятся из словарей: `corpus/ru` + `corpus/ru-speech` против
//! `corpus/en` + `corpus/tech` + `corpus/terminal`.
//!
//! Запуск: `cargo run --release -p eval --bin find-collisions [макс_длина]`

use std::collections::{HashMap, HashSet};
use std::path::Path;

use layout_map::{transliterate, Direction};

/// Слово считается известным только начиная с этой частоты. Отсекает опечатки и
/// случайный мусор, из-за которого коллизией объявлялось бы что попало.
const MIN_FREQ: u32 = 3;

fn load(dir: &Path) -> HashMap<String, u32> {
    let mut freq: HashMap<String, u32> = HashMap::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return freq,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            for token in ngram::tokenize(&text) {
                *freq.entry(token).or_insert(0) += 1;
            }
        }
    }
    freq
}

fn merge(into: &mut HashMap<String, u32>, from: HashMap<String, u32>) {
    for (k, v) in from {
        *into.entry(k).or_insert(0) += v;
    }
}

fn main() {
    let max_len: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(4);

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    let corpus = root.join("corpus");

    let mut ru = load(&corpus.join("ru"));
    merge(&mut ru, load(&corpus.join("ru-speech")));

    let mut en = load(&corpus.join("en"));
    merge(&mut en, load(&corpus.join("tech")));
    merge(&mut en, load(&corpus.join("terminal")));

    if ru.is_empty() || en.is_empty() {
        eprintln!("корпус пуст: соберите его tools/build-corpus.sh");
        std::process::exit(1);
    }

    // Пара считается коллизией, если оба написания известны и достаточно частотны.
    // Отношение симметрично, поэтому достаточно пройти по одному словарю.
    let mut found: Vec<(String, String, u32, u32)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (word, &ru_freq) in &ru {
        if ru_freq < MIN_FREQ || word.chars().count() > max_len {
            continue;
        }
        let latin = match transliterate(word, Direction::RuToEn) {
            Some(l) => l,
            None => continue,
        };
        let en_freq = match en.get(&latin) {
            Some(&f) if f >= MIN_FREQ => f,
            _ => continue,
        };
        if seen.insert(latin.clone()) {
            found.push((word.clone(), latin, ru_freq, en_freq));
        }
    }

    // Сортировка по редкости более слабой стороны: коллизия тем интереснее, чем
    // ближе частоты, — там у Tier 0 меньше всего оснований для выбора.
    found.sort_by_key(|(_, _, rf, ef)| std::cmp::Reverse(*rf.min(ef)));

    println!("Коллизии раскладки, длина ≤ {max_len}, частота ≥ {MIN_FREQ}");
    println!("Словари: ru {} слов, en {} слов\n", ru.len(), en.len());
    println!("| RU | EN | частота RU | частота EN |");
    println!("|---|---|---|---|");
    for (word, latin, rf, ef) in found.iter().take(60) {
        println!("| {word} | {latin} | {rf} | {ef} |");
    }
    println!("\nВсего найдено: {}", found.len());
}
