//! Кандидат Tier 1 №2: линейная модель над признаками Tier 0.
//!
//! Единственный кандидат из `docs/research/model-evaluation.md`, чья целевая функция
//! совпадает с задачей: он обучается прямо на бинарном решении «переключать или нет»,
//! а не оценивает правдоподобие текста в надежде, что разность оценок окажется полезной.
//!
//! Форма выбрана под требование [ADR-0005](../../../docs/adr/ADR-0005-personalization-and-online-learning.md):
//! модель должна дообучаться на пользователе по одному примеру. Отсюда десятки весов
//! вместо миллионов, обновление за один шаг градиента и интерпретируемость — по весам
//! видно, чему модель научилась.
//!
//! Признаки берутся у Tier 0, который их уже посчитал, поэтому вызов Tier 1 не стоит
//! почти ничего.

/// Названия признаков в том же порядке, что и веса. Существуют ради интерпретируемости:
/// веса без имён отлаживать невозможно.
pub const FEATURE_NAMES: [&str; 9] = [
    "смещение",
    "отрыв гипотез",
    "отрыв минус порог",
    "длина токена",
    "короткий (<=3)",
    "капслок",
    "сосед переключён",
    "регистр контекста",
    "заглавная в середине",
];

pub const FEATURE_COUNT: usize = FEATURE_NAMES.len();

pub type Features = [f64; FEATURE_COUNT];

#[derive(Clone)]
pub struct LinearModel {
    weights: Features,
    /// Скорость обучения. Для персонализации она намеренно ниже, чем для обучения
    /// с нуля: одна случайная отмена не должна ломать поведение (ADR-0005).
    lr: f64,
    /// L2-регуляризация. Держит веса маленькими, что при 612 примерах существенно.
    l2: f64,
}

impl LinearModel {
    pub fn new(lr: f64, l2: f64) -> Self {
        Self {
            weights: [0.0; FEATURE_COUNT],
            lr,
            l2,
        }
    }

    fn raw(&self, x: &Features) -> f64 {
        self.weights.iter().zip(x).map(|(w, v)| w * v).sum()
    }

    /// Вероятность того, что переключение требуется.
    pub fn probability(&self, x: &Features) -> f64 {
        1.0 / (1.0 + (-self.raw(x)).exp())
    }

    /// Один шаг градиента по одному примеру.
    ///
    /// Это и есть точка dogfood: отмена пользователя — размеченный пример
    /// `(признаки, label=false)`, и она вносится сюда ровно тем же вызовом,
    /// что и обучающая выборка.
    pub fn update(&mut self, x: &Features, label: bool) {
        let y = if label { 1.0 } else { 0.0 };
        let error = self.probability(x) - y;
        for (w, v) in self.weights.iter_mut().zip(x) {
            *w -= self.lr * (error * v + self.l2 * *w);
        }
    }

    pub fn train(&mut self, data: &[(Features, bool)], epochs: usize) {
        for _ in 0..epochs {
            for (x, label) in data {
                self.update(x, *label);
            }
        }
    }

    pub fn weights(&self) -> &Features {
        &self.weights
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(diff: f64, label: bool) -> (Features, bool) {
        let mut x = [0.0; FEATURE_COUNT];
        x[0] = 1.0;
        x[1] = diff;
        (x, label)
    }

    #[test]
    fn learns_a_separable_signal() {
        let mut m = LinearModel::new(0.3, 0.0);
        let data: Vec<_> = vec![
            sample(2.0, true),
            sample(1.5, true),
            sample(-2.0, false),
            sample(-1.5, false),
        ];
        m.train(&data, 300);
        assert!(m.probability(&sample(2.0, true).0) > 0.5);
        assert!(m.probability(&sample(-2.0, false).0) < 0.5);
    }

    /// Проверка требования ADR-0005: одна правка пользователя сдвигает решение
    /// в нужную сторону, но не переворачивает модель целиком.
    #[test]
    fn a_single_correction_moves_the_decision_without_breaking_it() {
        let mut m = LinearModel::new(0.3, 0.0);
        let data: Vec<_> = vec![sample(2.0, true), sample(-2.0, false)];
        m.train(&data, 300);

        let disputed = sample(1.0, true).0;
        let before = m.probability(&disputed);

        // Пользователь нажал отмену: «здесь переключать не надо».
        m.update(&disputed, false);
        let after = m.probability(&disputed);

        assert!(after < before, "правка не сдвинула решение");
        assert!(
            m.probability(&sample(2.0, true).0) > 0.5,
            "одна правка не должна ломать уверенные случаи"
        );
    }
}
