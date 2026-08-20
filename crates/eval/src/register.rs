//! Кандидат Tier 1 №1: регистр текста вместо модели.
//!
//! Наблюдение из разбора ошибок контекстного подкорпуса: почти все они различаются
//! не языком окружения (оно везде русское), а **регистром текста** — технический он
//! или бытовой.
//!
//! | Предложение | Верное чтение |
//! |---|---|
//! | Свет луны падал прямо на воду | `луны` — русское слово |
//! | Все `луны` лежат в переменных окружения | `keys` — набрано не в той раскладке |
//!
//! Окружение в обоих случаях русское, поэтому признак «язык соседей» бесполезен
//! (это уже измерено). Но «переменных окружения» — технический русский, а «свет
//! падал на воду» — бытовой, и эти два регистра различимы по частотным словарям,
//! которые уже собраны: `corpus/ru` (собственная документация) против
//! `corpus/ru-speech` (субтитры).
//!
//! Ни модели, ни обучения: две хэш-таблицы и логарифм отношения частот.

use std::collections::HashMap;

/// Сглаживание: слово, невиданное в одном из корпусов, не должно давать бесконечность.
const SMOOTHING: f64 = 0.5;

/// Сила усадки: при таком числе наблюдений слово получает половину своего веса.
const SHRINKAGE: f64 = 10.0;

/// Слово учитывается в оценке контекста, только если встречалось хотя бы столько раз
/// суммарно. Редкое слово говорит о регистре текста слишком мало.
const MIN_CONTEXT_FREQ: u32 = 2;

pub struct RegisterModel {
    tech: HashMap<String, u32>,
    common: HashMap<String, u32>,
    tech_total: f64,
    common_total: f64,
}

impl RegisterModel {
    pub fn new(tech_tokens: &[String], common_tokens: &[String]) -> Self {
        let mut tech: HashMap<String, u32> = HashMap::new();
        let mut common: HashMap<String, u32> = HashMap::new();
        for t in tech_tokens {
            *tech.entry(t.clone()).or_insert(0) += 1;
        }
        for t in common_tokens {
            *common.entry(t.clone()).or_insert(0) += 1;
        }
        let tech_total = tech_tokens.len().max(1) as f64;
        let common_total = common_tokens.len().max(1) as f64;
        Self {
            tech,
            common,
            tech_total,
            common_total,
        }
    }

    /// Логарифм отношения «насколько слово характернее для технического текста».
    /// Положительное — технический регистр, отрицательное — бытовой.
    ///
    /// Отношение частот берётся с усадкой по объёму свидетельств. Без неё замер
    /// меряет не то, что заявлено: технический корпус на два порядка меньше бытового,
    /// поэтому слово, встретившееся в документации ОДИН раз, оказывается в шестьдесят
    /// раз «частотнее» — арифметически верно, статистически бессмысленно.
    /// Множитель `n / (n + SHRINKAGE)` гасит такие одиночки почти до нуля и
    /// пропускает только слова, за которыми стоят наблюдения.
    fn word_score(&self, word: &str) -> Option<f64> {
        let t = *self.tech.get(word).unwrap_or(&0);
        let c = *self.common.get(word).unwrap_or(&0);
        let n = t + c;
        if n < MIN_CONTEXT_FREQ {
            return None;
        }
        let p_tech = (t as f64 + SMOOTHING) / self.tech_total;
        let p_common = (c as f64 + SMOOTHING) / self.common_total;
        let confidence = n as f64 / (n as f64 + SHRINKAGE);
        Some((p_tech / p_common).ln() * confidence)
    }

    /// Оценка регистра по словам окружения. `None` — слов, о которых есть данные,
    /// не нашлось, и признак молчит.
    pub fn context_score(&self, context: &[String]) -> Option<f64> {
        let scores: Vec<f64> = context.iter().filter_map(|w| self.word_score(w)).collect();
        if scores.is_empty() {
            return None;
        }
        Some(scores.iter().sum::<f64>() / scores.len() as f64)
    }

    /// Сколько слов окружения из скольких оказались известны. Без этого числа
    /// результат замера не интерпретируется: признак, опирающийся на два слова
    /// из десяти, ничего не опровергает и не подтверждает.
    pub fn coverage(&self, context: &[String]) -> (usize, usize) {
        let known = context
            .iter()
            .filter(|w| self.word_score(w).is_some())
            .count();
        (known, context.len())
    }

    pub fn vocab_sizes(&self) -> (usize, usize) {
        (self.tech.len(), self.common.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn technical_context_scores_above_everyday() {
        let model = RegisterModel::new(
            &words("переменная окружения конфиг сборка модуль конфиг сборка модуль"),
            &words("вода свет рука вечер вода свет рука вечер"),
        );
        let tech = model.context_score(&words("конфиг сборка")).unwrap();
        let common = model.context_score(&words("свет вода")).unwrap();
        assert!(tech > common, "tech={tech} common={common}");
    }

    #[test]
    fn unknown_context_is_silent() {
        let model = RegisterModel::new(&words("сборка сборка"), &words("вода вода"));
        assert_eq!(model.context_score(&words("абракадабра")), None);
    }
}
