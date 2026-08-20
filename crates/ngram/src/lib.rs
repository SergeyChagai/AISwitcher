//! Символьная n-граммная модель языка — baseline Tier 0 (ADR-0001, ADR-0002).
//!
//! Намеренно простая и объяснимая: интерполяция порядков 1..=N с фиксированными весами.
//! Это опорная точка, которую любой кандидат из `docs/research/model-evaluation.md`
//! обязан превзойти.

use std::collections::HashMap;

/// Максимальный порядок модели.
pub const ORDER: usize = 4;

/// Маркер границы токена. Не встречается в тексте, поэтому не конфликтует с данными.
const BOUNDARY: char = '\u{2}';

/// Веса интерполяции для порядков 1..=ORDER. Сумма равна 1.
const LAMBDAS: [f64; ORDER] = [0.05, 0.15, 0.30, 0.50];

/// Вероятностный пол: не даём невиданной n-грамме обнулить всю оценку.
const FLOOR: f64 = 1e-7;

#[derive(Default)]
pub struct CharNgram {
    /// Счётчики n-грамм всех порядков 1..=ORDER.
    grams: HashMap<String, u32>,
    /// Счётчики контекстов (префиксов длины 0..ORDER-1).
    contexts: HashMap<String, u32>,
    /// Размер алфавита — знаменатель равномерного пола.
    alphabet: usize,
}

impl CharNgram {
    pub fn new() -> Self {
        Self::default()
    }

    /// Обучение на тексте. Вызывается многократно, счётчики накапливаются.
    pub fn train(&mut self, text: &str) {
        let mut alphabet: HashMap<char, ()> = HashMap::new();
        for word in tokenize(text) {
            let padded: Vec<char> = std::iter::repeat(BOUNDARY)
                .take(ORDER - 1)
                .chain(word.chars())
                .chain(std::iter::once(BOUNDARY))
                .collect();

            for c in word.chars() {
                alphabet.insert(c, ());
            }

            for i in (ORDER - 1)..padded.len() {
                for k in 1..=ORDER {
                    let gram: String = padded[i + 1 - k..=i].iter().collect();
                    let ctx: String = padded[i + 1 - k..i].iter().collect();
                    *self.grams.entry(gram).or_insert(0) += 1;
                    *self.contexts.entry(ctx).or_insert(0) += 1;
                }
            }
        }
        self.alphabet = self.alphabet.max(alphabet.len());
    }

    pub fn is_trained(&self) -> bool {
        !self.grams.is_empty()
    }

    /// Средний логарифм вероятности символа. Нормировано по длине, поэтому
    /// оценки токенов разной длины сравнимы между собой.
    pub fn score(&self, token: &str) -> f64 {
        let word = normalize(token);
        if word.is_empty() {
            return f64::NEG_INFINITY;
        }

        let padded: Vec<char> = std::iter::repeat(BOUNDARY)
            .take(ORDER - 1)
            .chain(word.chars())
            .chain(std::iter::once(BOUNDARY))
            .collect();

        let uniform = 1.0 / (self.alphabet.max(1) as f64);
        let mut total = 0.0;
        let mut count = 0usize;

        for i in (ORDER - 1)..padded.len() {
            let mut p = FLOOR * uniform;
            for k in 1..=ORDER {
                let gram: String = padded[i + 1 - k..=i].iter().collect();
                let ctx: String = padded[i + 1 - k..i].iter().collect();
                let ctx_count = *self.contexts.get(&ctx).unwrap_or(&0);
                if ctx_count > 0 {
                    let gram_count = *self.grams.get(&gram).unwrap_or(&0);
                    p += LAMBDAS[k - 1] * (gram_count as f64 / ctx_count as f64);
                }
            }
            total += p.max(FLOOR * uniform).ln();
            count += 1;
        }

        total / count as f64
    }
}

/// Разбиение на токены: последовательности букв. Всё остальное — разделители.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_alphabetic() {
            current.push(c.to_lowercase().next().unwrap_or(c));
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn normalize(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trained_text_scores_higher_than_noise() {
        let mut m = CharNgram::new();
        m.train("the quick brown fox jumps over the lazy dog the the there then");
        assert!(m.score("then") > m.score("qxzjv"));
    }

    #[test]
    fn empty_token_is_rejected() {
        let m = CharNgram::new();
        assert_eq!(m.score("123"), f64::NEG_INFINITY);
    }

    #[test]
    fn tokenizer_splits_on_non_letters() {
        assert_eq!(tokenize("git push --force"), vec!["git", "push", "force"]);
    }

    #[test]
    fn untrained_model_is_flagged() {
        assert!(!CharNgram::new().is_trained());
    }
}
