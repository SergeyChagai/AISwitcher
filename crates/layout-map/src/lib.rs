//! Обратимое отображение раскладок ЙЦУКЕН <-> QWERTY.
//!
//! Основа Tier 0 (ADR-0001): по набранному токену однозначно восстанавливается его
//! вариант в другой раскладке. Генерация кандидата бесплатна, дорога только оценка.

/// Пары «клавиша в QWERTY» -> «символ в ЙЦУКЕН» для одного и того же физического нажатия.
const PAIRS: &[(char, char)] = &[
    // верхний ряд
    ('q', 'й'), ('w', 'ц'), ('e', 'у'), ('r', 'к'), ('t', 'е'), ('y', 'н'),
    ('u', 'г'), ('i', 'ш'), ('o', 'щ'), ('p', 'з'), ('[', 'х'), (']', 'ъ'),
    // домашний ряд
    ('a', 'ф'), ('s', 'ы'), ('d', 'в'), ('f', 'а'), ('g', 'п'), ('h', 'р'),
    ('j', 'о'), ('k', 'л'), ('l', 'д'), (';', 'ж'), ('\'', 'э'),
    // нижний ряд
    ('z', 'я'), ('x', 'ч'), ('c', 'с'), ('v', 'м'), ('b', 'и'), ('n', 'т'),
    ('m', 'ь'), (',', 'б'), ('.', 'ю'), ('/', '.'),
    // ряд цифр
    ('`', 'ё'),
    // верхний регистр
    ('Q', 'Й'), ('W', 'Ц'), ('E', 'У'), ('R', 'К'), ('T', 'Е'), ('Y', 'Н'),
    ('U', 'Г'), ('I', 'Ш'), ('O', 'Щ'), ('P', 'З'), ('{', 'Х'), ('}', 'Ъ'),
    ('A', 'Ф'), ('S', 'Ы'), ('D', 'В'), ('F', 'А'), ('G', 'П'), ('H', 'Р'),
    ('J', 'О'), ('K', 'Л'), ('L', 'Д'), (':', 'Ж'), ('"', 'Э'),
    ('Z', 'Я'), ('X', 'Ч'), ('C', 'С'), ('V', 'М'), ('B', 'И'), ('N', 'Т'),
    ('M', 'Ь'), ('<', 'Б'), ('>', 'Ю'), ('?', ','),
    ('~', 'Ё'),
    // различающиеся символы ряда цифр
    ('@', '"'), ('#', '№'), ('$', ';'), ('^', ':'), ('&', '?'),
];

/// Символ, набранный в QWERTY -> что получилось бы в ЙЦУКЕН.
pub fn en_to_ru_char(c: char) -> Option<char> {
    PAIRS.iter().find(|(en, _)| *en == c).map(|(_, ru)| *ru)
}

/// Символ, набранный в ЙЦУКЕН -> что получилось бы в QWERTY.
pub fn ru_to_en_char(c: char) -> Option<char> {
    PAIRS.iter().find(|(_, ru)| *ru == c).map(|(en, _)| *en)
}

/// Направление, в котором токен был бы перенабран.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Набрано в QWERTY, имелось в виду ЙЦУКЕН.
    EnToRu,
    /// Набрано в ЙЦУКЕН, имелось в виду QWERTY.
    RuToEn,
}

/// Альтернативная гипотеза для токена: тот же набор нажатий в другой раскладке.
///
/// Возвращает `None`, если токен не отображается целиком — например, содержит цифры
/// или символы, общие для обеих раскладок. Такие токены Tier 0 не трогает.
pub fn transliterate(token: &str, dir: Direction) -> Option<String> {
    let map = match dir {
        Direction::EnToRu => en_to_ru_char,
        Direction::RuToEn => ru_to_en_char,
    };
    token.chars().map(map).collect()
}

/// Определяет, в какой раскладке набран токен, по преобладанию алфавита.
///
/// `None` — токен смешанный, пустой или не содержит букв: решение не принимается.
pub fn detect_direction(token: &str) -> Option<Direction> {
    let mut latin = 0usize;
    let mut cyrillic = 0usize;
    for c in token.chars() {
        if c.is_ascii_alphabetic() {
            latin += 1;
        } else if is_cyrillic(c) {
            cyrillic += 1;
        }
    }
    match (latin, cyrillic) {
        (0, 0) => None,
        (l, 0) if l > 0 => Some(Direction::EnToRu),
        (0, c) if c > 0 => Some(Direction::RuToEn),
        _ => None,
    }
}

pub fn is_cyrillic(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_cases() {
        assert_eq!(transliterate("ghbdtn", Direction::EnToRu).unwrap(), "привет");
        assert_eq!(transliterate("руддщ", Direction::RuToEn).unwrap(), "hello");
    }

    #[test]
    fn mapping_is_reversible() {
        for (en, ru) in PAIRS {
            assert_eq!(ru_to_en_char(*ru), Some(*en), "ru->en broken for {ru}");
            assert_eq!(en_to_ru_char(*en), Some(*ru), "en->ru broken for {en}");
        }
    }

    #[test]
    fn round_trip() {
        let token = "ghbdtn";
        let ru = transliterate(token, Direction::EnToRu).unwrap();
        assert_eq!(transliterate(&ru, Direction::RuToEn).unwrap(), token);
    }

    /// Пары, где обе интерпретации осмысленны: латинский токен встречается в
    /// техническом тексте, а его кириллический двойник — настоящее русское слово.
    /// Ровно на них Tier 0 не может решить без контекста, поэтому список
    /// зафиксирован тестом: он используется в `corpus/context/sentences.txt`.
    #[test]
    fn ambiguous_pairs_are_what_we_claim() {
        let pairs = [
            ("vs", "мы"),
            ("ns", "ты"),
            ("he", "ру"),
            ("ds", "вы"),
            ("z", "я"),
            ("b", "и"),
            ("d", "в"),
            ("c", "с"),
            // Регистр решает: Vue — фреймворк, МГУ — университет,
            // а нажатия одни и те же.
            ("VUE", "МГУ"),
            // Найдены прогоном find-collisions по корпусам, а не подобраны руками.
            ("here", "руку"),
            ("keys", "луны"),
            ("of", "ща"),
            ("in", "шт"),
            ("us", "гы"),
            ("exe", "учу"),
            ("ids", "швы"),
            ("by", "ин"),
            ("cd", "св"),
            ("db", "ви"),
            // Худший случай из всех: строка совпадает целиком, включая регистр.
            // NP-полная задача против технического задания — различает только смысл.
            ("NP", "ТЗ"),
        ];
        for (en, ru) in pairs {
            assert_eq!(
                transliterate(en, Direction::EnToRu).as_deref(),
                Some(ru),
                "пара {en} / {ru} заявлена неверно"
            );
        }
    }

    #[test]
    fn digits_are_not_mapped() {
        assert_eq!(transliterate("test1", Direction::EnToRu), None);
    }

    #[test]
    fn direction_detection() {
        assert_eq!(detect_direction("hello"), Some(Direction::EnToRu));
        assert_eq!(detect_direction("привет"), Some(Direction::RuToEn));
        assert_eq!(detect_direction("при"), Some(Direction::RuToEn));
        assert_eq!(detect_direction("123"), None);
        assert_eq!(detect_direction("миrror"), None);
    }
}
