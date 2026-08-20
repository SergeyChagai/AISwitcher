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

mod register;
use register::RegisterModel;

/// Вес признака регистра текста при вызове Tier 1. Подбирается развёрткой.
const REGISTER_WEIGHT: f64 = 1.0;

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

/// Во сколько раз ослабляется порог, если соседний токен уверенно переключён.
///
/// Раскладку забывают не на одно слово: если сосед уже опознан как набранный не в
/// той раскладке, текущий токен почти наверняка из той же серии. Это единственный
/// контекстный признак, который не требует ни модели, ни словаря.
const NEIGHBOUR_RELIEF: f64 = 0.3;

/// Ширина зоны неуверенности вокруг порога. Решение внутри неё Tier 0 не принимает
/// сам, а поднимает на Tier 1 (ADR-0001).
/// Значение выбрано по кривой «цена против охвата» (см. вывод программы):
/// на 2.0 модели становятся доступны 11 ошибок из 12 ценой 22% поднятых решений.
/// Более узкая полоса дешевле, но отдаёт наверх меньше половины ошибок, то есть
/// делает Tier 1 бессмысленным.
const ESCALATION_BAND: f64 = 2.0;

/// Разбор одного токена без принятия решения: отрыв гипотез и признаки,
/// по которым решается, годится ли этот токен для n-граммной статистики вообще.
struct Analysis {
    /// Насколько альтернатива лучше исходной гипотезы. Отрицательное — хуже.
    diff: f64,
    /// Порог, который требуется превысить.
    required: f64,
    /// Токен непригоден для n-граммной оценки: аббревиатура или слишком короткий.
    unreliable: bool,
    /// Токен вообще не поддаётся анализу (цифры, смешанный алфавит).
    inert: bool,
}

impl Analysis {
    fn decision(&self) -> Decision {
        if !self.inert && self.diff > self.required {
            Decision::Switch
        } else {
            Decision::Keep
        }
    }

    /// Решение не принимается на Tier 0: либо отрыв в зоне неуверенности, либо
    /// статистика к токену неприменима.
    ///
    /// Ненадёжность токена сама по себе поводом не является. Русский текст состоит
    /// из коротких слов («не», «на», «что»), и правило «длина ≤3 → наверх» отправляло
    /// бы на Tier 1 треть всех решений. Поэтому ненадёжный токен поднимается только
    /// тогда, когда альтернатива не отвергнута уверенно.
    fn escalates(&self) -> bool {
        self.escalates_with_band(ESCALATION_BAND)
    }

    fn escalates_with_band(&self, band: f64) -> bool {
        if self.inert {
            return false;
        }
        if self.unreliable {
            return self.diff > -band;
        }
        (self.diff - self.required).abs() < band
    }
}

impl Tier0 {
    fn decide(&self, token: &str) -> Decision {
        self.decide_with_margin(token, self.margin)
    }

    fn analyze(&self, token: &str, margin: f64) -> Analysis {
        let inert = Analysis {
            diff: 0.0,
            required: margin,
            unreliable: false,
            inert: true,
        };

        let dir = match detect_direction(token) {
            Some(d) => d,
            None => return inert,
        };
        let alt = match transliterate(token, dir) {
            Some(a) => a,
            None => return inert,
        };

        let (as_typed, as_alt) = match dir {
            Direction::EnToRu => (self.en.score(token), self.ru.score(&alt)),
            Direction::RuToEn => (self.ru.score(token), self.en.score(&alt)),
        };

        let len = token.chars().count();
        let all_caps = token.chars().any(|c| c.is_alphabetic())
            && token.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());

        Analysis {
            diff: as_alt - as_typed,
            required: if len < SHORT_TOKEN_LEN { margin * 2.0 } else { margin },
            // Аббревиатура не подчиняется буквосочетаемости языка, а у совсем
            // коротких токенов статистики просто нет.
            unreliable: all_caps || len <= 3,
            inert: false,
        }
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

/// Разбор контекстного подкорпуса `corpus/context/cases.txt`.
///
/// Возвращает случаи, сгруппированные по семейству: `(семейство, токен, ожидание)`.
/// Токен в `{фигурных скобках}` набран не в той раскладке и ожидает переключения.
///
/// Здесь Tier 0 намеренно оценивается вне контекста — он контекста и не видит.
/// Смысл замера в том, чтобы показать, сколько именно он теряет там, где решение
/// без окружения невозможно: это и есть бюджет, который может отыграть Tier 1.
fn read_context_cases(path: &Path) -> Vec<Sentence> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let mut cases: Vec<Sentence> = Vec::new();
    let mut family = String::from("(без семейства)");

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(header) = trimmed.strip_prefix("# --- ") {
            // «A. Русская фраза ... ---» -> «A»
            family = header
                .split('.')
                .next()
                .unwrap_or(header)
                .trim()
                .to_string();
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }

        let tokens = trimmed
            .split_whitespace()
            .map(|raw| match raw.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                Some(token) => (token.to_string(), Decision::Switch),
                None => (raw.to_string(), Decision::Keep),
            })
            .collect();

        cases.push(Sentence {
            family: family.clone(),
            tokens,
        });
    }
    cases
}

/// Предложение контекстного подкорпуса: токены хранятся вместе, потому что
/// признаку соседства нужны соседи.
struct Sentence {
    family: String,
    tokens: Vec<(String, Decision)>,
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

    // Контекстный подкорпус: та же модель, но на предложениях, размеченных вручную.
    let context_cases = read_context_cases(&corpus.join("context").join("cases.txt"));
    if !context_cases.is_empty() {
        let mut per_family: HashMap<String, Counts> = HashMap::new();
        let mut ctx_overall = Counts::default();
        let mut ctx_errors: Vec<(String, String, Decision)> = Vec::new();
        let mut escalated = 0usize;
        let mut total_tokens = 0usize;
        let mut neighbour_fixed: Vec<String> = Vec::new();
        // Ключевая диагностика правила эскалации: ошибка, не поднятая наверх,
        // недостижима для Tier 1 в принципе.
        let mut errors_escalated = 0usize;

        for sentence in &context_cases {
            // Проход 1: Tier 0 сам по себе.
            let analyses: Vec<Analysis> = sentence
                .tokens
                .iter()
                .map(|(t, _)| tier0.analyze(t, tier0.margin))
                .collect();
            let base: Vec<Decision> = analyses.iter().map(|a| a.decision()).collect();

            // Проход 2: признак соседства. Порог ослабляется, если соседний токен
            // уверенно переключён и сам при этом не поднимался на Tier 1.
            for (i, (token, expected)) in sentence.tokens.iter().enumerate() {
                total_tokens += 1;
                let a = &analyses[i];
                if a.escalates() {
                    escalated += 1;
                }

                let neighbour_switched = [i.checked_sub(1), i.checked_add(1)]
                    .into_iter()
                    .flatten()
                    .any(|j| {
                        j < base.len()
                            && base[j] == Decision::Switch
                            && !analyses[j].escalates()
                    });

                let got = if !a.inert && neighbour_switched && a.diff > a.required * NEIGHBOUR_RELIEF
                {
                    Decision::Switch
                } else {
                    base[i]
                };

                if got != base[i] && got == *expected {
                    neighbour_fixed.push(token.clone());
                }

                ctx_overall.record(*expected, got);
                per_family
                    .entry(sentence.family.clone())
                    .or_default()
                    .record(*expected, got);
                if got != *expected {
                    ctx_errors.push((sentence.family.clone(), token.clone(), *expected));
                    if a.escalates() {
                        errors_escalated += 1;
                    }
                }
            }
        }

        println!("## Контекстный подкорпус\n");
        println!(
            "{} токенов, precision {:.4}, recall {:.4}, FP {}, FN {}",
            ctx_overall.total(),
            ctx_overall.precision(),
            ctx_overall.recall(),
            ctx_overall.fp,
            ctx_overall.fn_
        );
        println!(
            "Поднято на Tier 1: {} из {} ({:.1}%)",
            escalated,
            total_tokens,
            100.0 * escalated as f64 / total_tokens as f64
        );
        println!(
            "Ошибок внутри поднятого множества: {} из {}",
            errors_escalated,
            ctx_errors.len()
        );
        println!(
            "Исправлено признаком соседства: {}\n",
            if neighbour_fixed.is_empty() {
                "нет".to_string()
            } else {
                neighbour_fixed.join(", ")
            }
        );

        println!("| Семейство | Токенов | FP | FN | Доля верных |");
        println!("|---|---|---|---|---|");
        let mut families: Vec<&String> = per_family.keys().collect();
        families.sort();
        for f in families {
            let c = &per_family[f];
            let acc = 1.0 - (c.fp + c.fn_) as f64 / c.total() as f64;
            println!("| {} | {} | {} | {} | {:.4} |", f, c.total(), c.fp, c.fn_, acc);
        }
        println!();

        // Список ошибок печатается целиком: их немного, и каждая — конкретный
        // случай, который должен либо чиниться, либо осознанно приниматься.
        // Главный компромисс правила эскалации: чем шире полоса, тем больше ошибок
        // доступно Tier 1 — и тем чаще вызывается модель. Одной точкой это не
        // описывается, поэтому печатается кривая.
        println!("Цена эскалации против охвата ошибок:\n");
        println!("| band | Поднято | Доля | Ошибок доступно |");
        println!("|---|---|---|---|");
        // Знаменатель — ошибки чистого Tier 0, до признака соседства: именно их
        // могла бы забрать модель.
        let base_errors: usize = context_cases
            .iter()
            .flat_map(|s| s.tokens.iter())
            .filter(|(t, e)| tier0.analyze(t, tier0.margin).decision() != *e)
            .count();

        for band in [0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 10.0] {
            let mut up = 0usize;
            let mut covered = 0usize;
            for sentence in &context_cases {
                for (token, expected) in &sentence.tokens {
                    let a = tier0.analyze(token, tier0.margin);
                    let esc = a.escalates_with_band(band);
                    if esc {
                        up += 1;
                    }
                    if a.decision() != *expected && esc {
                        covered += 1;
                    }
                }
            }
            println!(
                "| {:.2} | {} | {:.1}% | {} из {} |",
                band,
                up,
                100.0 * up as f64 / total_tokens as f64,
                covered,
                base_errors
            );
        }
        println!();

        // Поведение продукта до появления Tier 1: раз решение поднято наверх, а
        // наверху пусто — действие по умолчанию не переключать (ADR-0001).
        // Это не вариант настройки, а проверка того, что принцип соблюдается.
        let mut deferred = Counts::default();
        for sentence in &context_cases {
            for (token, expected) in &sentence.tokens {
                let a = tier0.analyze(token, tier0.margin);
                let got = if a.escalates() {
                    Decision::Keep
                } else {
                    a.decision()
                };
                deferred.record(*expected, got);
            }
        }
        println!(
            "Если поднятое наверх не переключать: precision {:.4}, recall {:.4}, FP {}, FN {}\n",
            deferred.precision(),
            deferred.recall(),
            deferred.fp,
            deferred.fn_
        );

        // --- Кандидат Tier 1 №1: регистр текста ---
        //
        // Вызывается только на поднятых решениях. Технический контекст сдвигает выбор
        // в сторону латинского чтения токена, бытовой — в сторону русского.
        let register = RegisterModel::new(&ru_train, &speech_train);

        println!("## Кандидат Tier 1: регистр текста\n");
        let (tech_vocab, common_vocab) = register.vocab_sizes();
        println!(
            "Словари: технический русский {} слов, бытовой {} слов",
            tech_vocab, common_vocab
        );

        // Покрытие считается один раз: оно не зависит от веса.
        let (mut known, mut seen_words) = (0usize, 0usize);
        for sentence in &context_cases {
            let analyses: Vec<Analysis> = sentence
                .tokens
                .iter()
                .map(|(t, _)| tier0.analyze(t, tier0.margin))
                .collect();
            for i in 0..sentence.tokens.len() {
                if !analyses[i].escalates() {
                    continue;
                }
                let context: Vec<String> = sentence
                    .tokens
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i && !analyses[*j].escalates())
                    .map(|(_, (t, _))| t.to_lowercase())
                    .filter(|t| t.chars().all(layout_map::is_cyrillic))
                    .collect();
                let (k, n) = register.coverage(&context);
                known += k;
                seen_words += n;
            }
        }
        println!(
            "Покрытие контекста: {} известных слов из {} ({:.1}%)\n",
            known,
            seen_words,
            100.0 * known as f64 / seen_words.max(1) as f64
        );

        println!("| вес | Precision | Recall | FP | FN | Вызовов признака |");
        println!("|---|---|---|---|---|---|");

        for weight in [0.0, 0.25, 0.5, 1.0, 2.0, 4.0] {
            let mut c = Counts::default();
            let mut used = 0usize;
            for sentence in &context_cases {
                let analyses: Vec<Analysis> = sentence
                    .tokens
                    .iter()
                    .map(|(t, _)| tier0.analyze(t, tier0.margin))
                    .collect();

                for (i, (token, expected)) in sentence.tokens.iter().enumerate() {
                    let a = &analyses[i];
                    if !a.escalates() {
                        c.record(*expected, a.decision());
                        continue;
                    }

                    // Контекст — соседние кириллические токены, решение по которым
                    // Tier 0 принял уверенно. Сам оцениваемый токен исключён.
                    let context: Vec<String> = sentence
                        .tokens
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i && !analyses[*j].escalates())
                        .map(|(_, (t, _))| t.to_lowercase())
                        .filter(|t| t.chars().all(layout_map::is_cyrillic))
                        .collect();

                    let got = match register.context_score(&context) {
                        Some(ctx) => {
                            used += 1;
                            // Технический контекст (ctx > 0) удешевляет переключение
                            // кириллицы в латиницу и удорожает обратное.
                            let shift = match detect_direction(token) {
                                Some(Direction::RuToEn) => -ctx * weight,
                                Some(Direction::EnToRu) => ctx * weight,
                                None => 0.0,
                            };
                            if a.diff > a.required + shift {
                                Decision::Switch
                            } else {
                                Decision::Keep
                            }
                        }
                        None => a.decision(),
                    };
                    c.record(*expected, got);
                }
            }
            println!(
                "| {:.2} | {:.4} | {:.4} | {} | {} | {} |",
                weight,
                c.precision(),
                c.recall(),
                c.fp,
                c.fn_,
                used
            );
        }
        println!();

        println!("Ошибки ({}):\n", ctx_errors.len());
        for (family, token, expected) in &ctx_errors {
            let what = match expected {
                Decision::Switch => "не переключил",
                Decision::Keep => "ЛОЖНОЕ переключение",
            };
            println!("- [{family}] {token} — {what}");
        }
        println!();
    }

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
