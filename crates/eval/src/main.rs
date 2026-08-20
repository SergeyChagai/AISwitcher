//! Sprint 1: замер baseline Tier 0 на корпусе.
//!
//! Протокол — `docs/research/model-evaluation.md`. Эталон выводится автоматически:
//! корректно набранный токен размечается как `keep`, его раскладочная транслитерация —
//! как `switch`. Ручная разметка не требуется, но и корпус получается синтетическим:
//! это ограничение зафиксировано в отчёте.

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use layout_map::{detect_direction, transliterate, Direction};
use ngram::CharNgram;

/// Порог: на сколько альтернатива должна опережать исходную гипотезу, чтобы
/// переключение состоялось. Смещён в сторону молчания (ADR-0001).
///
/// Значение выбрано по развёртке (см. вывод программы): на 0.7 число ложных
/// срабатываний падает с 11 до 2 ценой ~1.4 пункта recall. При асимметричной
/// стоимости ошибок это выгодный обмен.
const DEFAULT_MARGIN: f64 = 0.7;

/// Токены короче этого требуют вдвое большего отрыва: статистики почти нет.
const SHORT_TOKEN_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Keep,
    Switch,
}

struct Tier0 {
    ru: CharNgram,
    en: CharNgram,
    margin: f64,
}

impl Tier0 {
    fn decide(&self, token: &str) -> Decision {
        self.decide_with_margin(token, self.margin)
    }

    fn decide_with_margin(&self, token: &str, margin: f64) -> Decision {
        let dir = match detect_direction(token) {
            Some(d) => d,
            None => return Decision::Keep,
        };
        let alt = match transliterate(token, dir) {
            Some(a) => a,
            None => return Decision::Keep,
        };

        let (as_typed, as_alt) = match dir {
            // набрано латиницей: либо это английский, либо русский в неверной раскладке
            Direction::EnToRu => (self.en.score(token), self.ru.score(&alt)),
            // набрано кириллицей: либо это русский, либо английский в неверной раскладке
            Direction::RuToEn => (self.ru.score(token), self.en.score(&alt)),
        };

        let required = if token.chars().count() < SHORT_TOKEN_LEN {
            margin * 2.0
        } else {
            margin
        };

        if as_alt - as_typed > required {
            Decision::Switch
        } else {
            Decision::Keep
        }
    }
}

#[derive(Default)]
struct Counts {
    tp: usize,
    fp: usize,
    fn_: usize,
    tn: usize,
}

impl Counts {
    fn record(&mut self, expected: Decision, got: Decision) {
        match (expected, got) {
            (Decision::Switch, Decision::Switch) => self.tp += 1,
            (Decision::Keep, Decision::Switch) => self.fp += 1,
            (Decision::Switch, Decision::Keep) => self.fn_ += 1,
            (Decision::Keep, Decision::Keep) => self.tn += 1,
        }
    }

    fn total(&self) -> usize {
        self.tp + self.fp + self.fn_ + self.tn
    }

    fn precision(&self) -> f64 {
        let d = self.tp + self.fp;
        if d == 0 {
            f64::NAN
        } else {
            self.tp as f64 / d as f64
        }
    }

    fn recall(&self) -> f64 {
        let d = self.tp + self.fn_;
        if d == 0 {
            f64::NAN
        } else {
            self.tp as f64 / d as f64
        }
    }
}

/// Потолок на подкорпус живой речи. OpenSubtitles даёт миллионы токенов, но
/// n-граммная модель насыщается задолго до этого, а счётчики в HashMap растут
/// линейно. Ограничение держит прогон в секундах.
const SPEECH_TOKEN_LIMIT: usize = 400_000;

fn read_dir_tokens(dir: &Path) -> Vec<String> {
    read_dir_tokens_limited(dir, usize::MAX)
}

fn read_dir_tokens_limited(dir: &Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("txt") {
            continue;
        }
        // Ошибка чтения не глотается: битая кодировка иначе выглядит как пустой
        // корпус, и замер молча считает не то, что кажется.
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                for token in ngram::tokenize(&text) {
                    if out.len() >= limit {
                        return out;
                    }
                    out.push(token);
                }
            }
            Err(e) => {
                eprintln!("не прочитан {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    }
    out
}

/// Разделение на обучение и тест **по типам, а не по вхождениям**.
///
/// Сплит по вхождениям даёт утечку: частотное слово попадает и в обучение, и в тест,
/// и модель оценивается на том, что уже видела. Продукт же ломается ровно на обратном —
/// на внесловарных токенах (`docs/product/vision.md`). Поэтому в тест уходит каждый
/// пятый *уникальный* токен вместе со всеми своими вхождениями, а обучение его не видит.
///
/// Отбор идёт по индексу в отсортированном списке типов, поэтому прогон
/// воспроизводим и не зависит от порядка обхода файлов.
fn split(tokens: &[String]) -> (Vec<String>, Vec<String>) {
    use std::collections::HashSet;

    let mut types: Vec<&String> = {
        let mut seen = HashSet::new();
        tokens.iter().filter(|t| seen.insert(t.as_str())).collect()
    };
    types.sort();

    let held_out: HashSet<&str> = types
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 5 == 4)
        .map(|(_, t)| t.as_str())
        .collect();

    let mut train = Vec::new();
    let mut test = Vec::new();
    for t in tokens {
        if held_out.contains(t.as_str()) {
            test.push(t.clone());
        } else {
            train.push(t.clone());
        }
    }

    // В тесте каждый тип нужен один раз: повторы только раздували бы счётчики
    // в пользу частотных слов.
    let mut seen = HashSet::new();
    test.retain(|t| seen.insert(t.clone()));

    (train, test)
}

fn percentile(sorted_nanos: &[u128], p: f64) -> u128 {
    if sorted_nanos.is_empty() {
        return 0;
    }
    let idx = ((sorted_nanos.len() as f64 - 1.0) * p).round() as usize;
    sorted_nanos[idx]
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let corpus = root.join("corpus");

    let ru_tokens = read_dir_tokens(&corpus.join("ru"));
    // Живая речь: субтитры из OPUS. Каталог необязателен — если корпус не скачан
    // (tools/fetch-opensubtitles.sh), замер просто идёт без этого подкорпуса.
    let speech_tokens = read_dir_tokens_limited(&corpus.join("ru-speech"), SPEECH_TOKEN_LIMIT);
    let en_tokens = read_dir_tokens(&corpus.join("en"));
    let tech_tokens = read_dir_tokens(&corpus.join("tech"));
    let terminal_tokens = read_dir_tokens(&corpus.join("terminal"));

    if ru_tokens.is_empty() || en_tokens.is_empty() {
        eprintln!("корпус пуст: наполните corpus/ru и corpus/en");
        std::process::exit(1);
    }

    let (ru_train, ru_test) = split(&ru_tokens);
    let (en_train, en_test) = split(&en_tokens);
    let (tech_train, tech_test) = split(&tech_tokens);
    let (term_train, term_test) = split(&terminal_tokens);

    let (speech_train, speech_test) = split(&speech_tokens);

    let mut ru = CharNgram::new();
    ru.train(&ru_train.join(" "));
    ru.train(&speech_train.join(" "));

    // Модель «текста в EN-раскладке» обучается и на техническом тексте: npm, ls,
    // пути и идентификаторы набираются в этой раскладке и переключать их не нужно.
    let mut en = CharNgram::new();
    en.train(&en_train.join(" "));
    en.train(&tech_train.join(" "));
    en.train(&term_train.join(" "));

    let tier0 = Tier0 {
        ru,
        en,
        margin: DEFAULT_MARGIN,
    };

    // Построение тест-сета: (подкорпус, токен, ожидаемое решение)
    let mut cases: Vec<(&str, String, Decision)> = Vec::new();
    for t in &ru_test {
        cases.push(("ru-clean", t.clone(), Decision::Keep));
        if let Some(m) = transliterate(t, Direction::RuToEn) {
            cases.push(("mistyped-ru", m, Decision::Switch));
        }
    }
    for t in &en_test {
        cases.push(("en-clean", t.clone(), Decision::Keep));
        if let Some(m) = transliterate(t, Direction::EnToRu) {
            cases.push(("mistyped-en", m, Decision::Switch));
        }
    }
    for t in &speech_test {
        cases.push(("ru-speech", t.clone(), Decision::Keep));
        if let Some(m) = transliterate(t, Direction::RuToEn) {
            cases.push(("mistyped-speech", m, Decision::Switch));
        }
    }
    for t in &tech_test {
        cases.push(("mixed-tech", t.clone(), Decision::Keep));
    }
    for t in &term_test {
        cases.push(("terminal", t.clone(), Decision::Keep));
    }

    let mut overall = Counts::default();
    let mut per_corpus: HashMap<&str, Counts> = HashMap::new();
    let mut short = Counts::default();
    let mut latencies: Vec<u128> = Vec::with_capacity(cases.len());

    for (subcorpus, token, expected) in &cases {
        let start = Instant::now();
        let got = tier0.decide(token);
        latencies.push(start.elapsed().as_nanos());

        overall.record(*expected, got);
        per_corpus.entry(subcorpus).or_default().record(*expected, got);
        if token.chars().count() <= 3 {
            short.record(*expected, got);
        }
    }

    latencies.sort_unstable();

    println!("# Baseline Tier 0 — результаты\n");
    println!(
        "Обучение: ru={} ток., ru-speech={} ток., en={} ток., tech={} ток., terminal={} ток.",
        ru_train.len(),
        speech_train.len(),
        en_train.len(),
        tech_train.len(),
        term_train.len()
    );
    println!(
        "Тест: {} случаев, порог margin={}\n",
        cases.len(),
        DEFAULT_MARGIN
    );

    println!("| Метрика | Значение |");
    println!("|---|---|");
    println!("| Precision (switch) | {:.4} |", overall.precision());
    println!("| Recall (switch)    | {:.4} |", overall.recall());
    println!(
        "| TP / FP / FN / TN  | {} / {} / {} / {} |",
        overall.tp, overall.fp, overall.fn_, overall.tn
    );
    println!("| Латентность p50    | {} нс |", percentile(&latencies, 0.50));
    println!("| Латентность p99    | {} нс |", percentile(&latencies, 0.99));
    println!();

    println!("| Подкорпус | Случаев | Ошибок | Доля верных |");
    println!("|---|---|---|---|");
    let mut names: Vec<&&str> = per_corpus.keys().collect();
    names.sort();
    for name in names {
        let c = &per_corpus[*name];
        let errors = c.fp + c.fn_;
        let acc = 1.0 - errors as f64 / c.total() as f64;
        println!("| {} | {} | {} | {:.4} |", name, c.total(), errors, acc);
    }
    println!();
    println!(
        "Короткие токены (<=3 симв.): {} случаев, {} ошибок",
        short.total(),
        short.fp + short.fn_
    );
    println!();

    // Развёртка по порогу: рабочая точка выбирается под асимметричную стоимость
    // ошибок (ADR-0001), поэтому важна не одна цифра, а форма кривой.
    println!("## Развёртка по порогу\n");
    println!("| margin | Precision | Recall | FP | FN |");
    println!("|---|---|---|---|---|");
    for step in 0..=12 {
        let margin = step as f64 * 0.1;
        let mut c = Counts::default();
        for (_, token, expected) in &cases {
            c.record(*expected, tier0.decide_with_margin(token, margin));
        }
        println!(
            "| {:.1} | {:.4} | {:.4} | {} | {} |",
            margin,
            c.precision(),
            c.recall(),
            c.fp,
            c.fn_
        );
    }
}
