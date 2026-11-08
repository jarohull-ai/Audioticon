//! Polski normalizator tekstu (Verbalizer) dla syntezy mowy.
//! Konwertuje liczby całkowite, godziny (np. 15:47) oraz daty (np. 24.02.2024)
//! na w pełni odmienione słowa w języku polskim.

use regex::{Captures, Regex};
use std::sync::OnceLock;

const ONES: [&str; 10] = [
    "",
    "jeden",
    "dwa",
    "trzy",
    "cztery",
    "pięć",
    "sześć",
    "siedem",
    "osiem",
    "dziewięć",
];

const TEENS: [&str; 10] = [
    "dziesięć",
    "jedenaście",
    "dwanaście",
    "trzynaście",
    "czternaście",
    "piętnaście",
    "szesnaście",
    "siedemnaście",
    "osiemnaście",
    "dziewiętnaście",
];

const TENS: [&str; 10] = [
    "",
    "dziesięć",
    "dwadzieścia",
    "trzydzieści",
    "czterdzieści",
    "pięćdziesiąt",
    "sześćdziesiąt",
    "siedemdziesiąt",
    "osiemdziesiąt",
    "dziewięćdziesiąt",
];

const HUNDREDS: [&str; 10] = [
    "",
    "sto",
    "dwieście",
    "trzysta",
    "czterysta",
    "pięćset",
    "sześćset",
    "siedemset",
    "osiemset",
    "dziewięćset",
];

const ORDINAL_HOURS: [&str; 25] = [
    "północ",
    "pierwsza",
    "druga",
    "trzecia",
    "czwarta",
    "piąta",
    "szósta",
    "siódma",
    "ósma",
    "dziewiąta",
    "dziesiąta",
    "jedenasta",
    "dwunasta",
    "trzynasta",
    "czternasta",
    "piętnasta",
    "szesnasta",
    "siedemnasta",
    "osiemnasta",
    "dziewiętnasta",
    "dwudziesta",
    "dwudziesta pierwsza",
    "dwudziesta druga",
    "dwudziesta trzecia",
    "dwudziesta czwarta",
];

const ORDINAL_HOURS_LOCATIVE: [&str; 25] = [
    "północy",
    "pierwszej",
    "drugiej",
    "trzeciej",
    "czwartej",
    "piątej",
    "szóstej",
    "siódmej",
    "ósmej",
    "dziewiątej",
    "dziesiątej",
    "jedenastej",
    "dwunastej",
    "trzynastej",
    "czternastej",
    "piętnastej",
    "szesnastej",
    "siedemnastej",
    "osiemnastej",
    "dziewiętnastej",
    "dwudziestej",
    "dwudziestej pierwszej",
    "dwudziestej drugiej",
    "dwudziestej trzeciej",
    "dwudziestej czwartej",
];

const ORDINAL_DAYS_GENITIVE: [&str; 32] = [
    "",
    "pierwszego",
    "drugiego",
    "trzeciego",
    "czwartego",
    "piątego",
    "szóstego",
    "siódmego",
    "ósmego",
    "dziewiątego",
    "dziesiątego",
    "jedenastego",
    "dwunastego",
    "trzynastego",
    "czternastego",
    "piętnastego",
    "szesnastego",
    "siedemnastego",
    "osiemnastego",
    "dziewiętnastego",
    "dwudziestego",
    "dwudziestego pierwszego",
    "dwudziestego drugiego",
    "dwudziestego trzeciego",
    "dwudziestego czwartego",
    "dwudziestego piątego",
    "dwudziestego szóstego",
    "dwudziestego siódmego",
    "dwudziestego ósmego",
    "dwudziestego dziewiątego",
    "trzydziestego",
    "trzydziestego pierwszego",
];

const MONTHS_GENITIVE: [&str; 13] = [
    "",
    "stycznia",
    "lutego",
    "marca",
    "kwietnia",
    "maja",
    "czerwca",
    "lipca",
    "sierpnia",
    "września",
    "października",
    "listopada",
    "grudnia",
];

/// Formy gramatyczne skali wielkich liczb: [jeden, 2-4, reszta]
const SCALES: [(&str, &str, &str); 4] = [
    ("", "", ""),
    ("tysiąc", "tysiące", "tysięcy"),
    ("milion", "miliony", "milionów"),
    ("miliard", "miliardy", "miliardów"),
];

/// Zamienia liczbę od 0 do 999 na słowa.
fn triplet_to_words(n: u32) -> String {
    if n == 0 {
        return String::new();
    }
    let h = (n / 100) as usize;
    let t = ((n % 100) / 10) as usize;
    let o = (n % 10) as usize;

    let mut parts = Vec::new();
    if h > 0 {
        parts.push(HUNDREDS[h]);
    }

    if t == 1 {
        parts.push(TEENS[o]);
    } else {
        if t > 1 {
            parts.push(TENS[t]);
        }
        if o > 0 {
            parts.push(ONES[o]);
        }
    }

    parts.join(" ")
}

/// Zamienia liczbę całkowitą nieujemną na słowa w mianowniku.
pub fn number_to_words(mut n: u64) -> String {
    if n == 0 {
        return "zero".to_string();
    }

    let mut scale_idx = 0;
    let mut parts = Vec::new();

    while n > 0 && scale_idx < SCALES.len() {
        let triplet = (n % 1000) as u32;
        if triplet > 0 {
            let words = triplet_to_words(triplet);
            let (singular, few, many) = SCALES[scale_idx];

            if scale_idx == 0 {
                parts.push(words);
            } else {
                let suffix = if triplet == 1 {
                    singular
                } else {
                    let last_two = triplet % 100;
                    let last_one = triplet % 10;
                    if (11..=19).contains(&last_two) {
                        many
                    } else if (2..=4).contains(&last_one) {
                        few
                    } else {
                        many
                    }
                };
                if triplet == 1 && scale_idx == 1 {
                    // Dla 1000 mówimy "tysiąc", a nie "jeden tysiąc"
                    parts.push(suffix.to_string());
                } else {
                    parts.push(format!("{} {}", words, suffix));
                }
            }
        }
        n /= 1000;
        scale_idx += 1;
    }

    parts.reverse();
    parts.join(" ")
}

/// Zamienia rok na słowa w dopełniaczu (np. 2024 -> "dwa tysiące dwudziestego czwartego roku")
fn year_to_words_genitive(year: u32) -> String {
    if (2000..2100).contains(&year) {
        let remainder = year - 2000;
        if remainder == 0 {
            "dwa tysiące roku".to_string()
        } else if remainder <= 31 {
            format!(
                "dwa tysiące {} roku",
                ORDINAL_DAYS_GENITIVE[remainder as usize]
            )
        } else {
            // np. 2045 -> dwa tysiące czterdziestego piątego roku
            let t = (remainder / 10) as usize;
            let o = (remainder % 10) as usize;
            let tens_gen = match t {
                2 => "dwudziestego",
                3 => "trzydziestego",
                4 => "czterdziestego",
                5 => "pięćdziesiątego",
                6 => "sześćdziesiątego",
                7 => "siedemdziesiątego",
                8 => "osiemdziesiątego",
                9 => "dziewięćdziesiątego",
                _ => "",
            };
            let ones_gen = ORDINAL_DAYS_GENITIVE[o];
            if o == 0 {
                format!("dwa tysiące {} roku", tens_gen)
            } else {
                format!("dwa tysiące {} {} roku", tens_gen, ones_gen)
            }
        }
    } else if (1900..2000).contains(&year) {
        let remainder = year - 1900;
        if remainder <= 31 {
            format!(
                "tysiąc dziewięćset {} roku",
                ORDINAL_DAYS_GENITIVE[remainder as usize]
            )
        } else {
            let t = (remainder / 10) as usize;
            let o = (remainder % 10) as usize;
            let tens_gen = match t {
                3 => "trzydziestego",
                4 => "czterdziestego",
                5 => "pięćdziesiątego",
                6 => "sześćdziesiątego",
                7 => "siedemdziesiątego",
                8 => "osiemdziesiątego",
                9 => "dziewięćdziesiątego",
                _ => "",
            };
            let ones_gen = ORDINAL_DAYS_GENITIVE[o];
            if o == 0 {
                format!("tysiąc dziewięćset {} roku", tens_gen)
            } else {
                format!("tysiąc dziewięćset {} {} roku", tens_gen, ones_gen)
            }
        }
    } else {
        format!("{} roku", number_to_words(year as u64))
    }
}

/// Normalizuje daty w formacie DD.MM.YYYY lub DD-MM-YYYY (oraz zjada opcjonalne 'roku' lub 'r.' po dacie)
fn normalize_dates(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\b([0-3]?[0-9])[.\-/]([0-1]?[0-9])[.\-/]([1-2][0-9]{3})(?:\s*(?:roku|r\.))?\b")
            .expect("Valid date regex")
    });

    re.replace_all(text, |caps: &Captures| {
        let day: u32 = caps[1].parse().unwrap_or(0);
        let month: u32 = caps[2].parse().unwrap_or(0);
        let year: u32 = caps[3].parse().unwrap_or(0);

        if (1..=31).contains(&day) && (1..=12).contains(&month) && year >= 1000 {
            let day_word = ORDINAL_DAYS_GENITIVE[day as usize];
            let month_word = MONTHS_GENITIVE[month as usize];
            let year_words = year_to_words_genitive(year);
            format!("{} {} {}", day_word, month_word, year_words)
        } else {
            caps[0].to_string()
        }
    })
    .to_string()
}

/// Normalizuje daty słowne w dopełniaczu (np. "24 lutego 2024" lub "dwudziestego czwartego lutego 2026 roku")
fn normalize_textual_dates(text: &str) -> String {
    static RE_MONTH_YEAR: OnceLock<Regex> = OnceLock::new();
    let re_month_year = RE_MONTH_YEAR.get_or_init(|| {
        Regex::new(r"(?i)\b(stycznia|lutego|marca|kwietnia|maja|czerwca|lipca|sierpnia|września|października|listopada|grudnia)\s+([1-2][0-9]{3})(?:\s*(?:roku|r\.?))?\b")
            .expect("Valid month + year regex")
    });

    static RE_DAY_MONTH: OnceLock<Regex> = OnceLock::new();
    let re_day_month = RE_DAY_MONTH.get_or_init(|| {
        Regex::new(r"(?i)\b([0-3]?[0-9])\s+(stycznia|lutego|marca|kwietnia|maja|czerwca|lipca|sierpnia|września|października|listopada|grudnia)\b")
            .expect("Valid day + month regex")
    });

    // 1. Najpierw zamień dzień przed nazwą miesiąca (np. "24 lutego" -> "dwudziestego czwartego lutego")
    let with_days = re_day_month.replace_all(text, |caps: &Captures| {
        let day: u32 = caps[1].parse().unwrap_or(0);
        let month_str = &caps[2];
        if (1..=31).contains(&day) {
            format!("{} {}", ORDINAL_DAYS_GENITIVE[day as usize], month_str)
        } else {
            caps[0].to_string()
        }
    });

    // 2. Następnie zamień każdy rok po nazwie miesiąca w dopełniaczu (niezależnie czy dzień był cyfrą czy słowem!)
    re_month_year
        .replace_all(&with_days, |caps: &Captures| {
            let month_str = &caps[1];
            let year: u32 = caps[2].parse().unwrap_or(0);
            if year >= 1000 {
                format!("{} {}", month_str, year_to_words_genitive(year))
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
}

/// Normalizuje godziny w formacie HH:MM (z uwzględnieniem przyimków miejscownika np. "o 15:47" -> "o piętnastej czterdzieści siedem")
fn normalize_times(text: &str) -> String {
    static RE_PREP: OnceLock<Regex> = OnceLock::new();
    let re_prep = RE_PREP.get_or_init(|| {
        Regex::new(r"(?i)\b(o|od|do|około|godz\.|godzinie)\s+([0-2]?[0-9]):([0-5][0-9])\b")
            .expect("Valid time with preposition regex")
    });

    let with_prep = re_prep
        .replace_all(text, |caps: &Captures| {
            let prep = &caps[1];
            let hour: usize = caps[2].parse().unwrap_or(99);
            let minute: u32 = caps[3].parse().unwrap_or(99);

            if hour <= 24 && minute < 60 {
                let hour_str = ORDINAL_HOURS_LOCATIVE[hour];
                let min_str = if minute == 0 {
                    String::new()
                } else if minute < 10 {
                    format!("zero {}", number_to_words(minute as u64))
                } else {
                    number_to_words(minute as u64)
                };

                if min_str.is_empty() {
                    format!("{} {}", prep, hour_str)
                } else {
                    format!("{} {} {}", prep, hour_str, min_str)
                }
            } else {
                caps[0].to_string()
            }
        })
        .to_string();

    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"\b([0-2]?[0-9]):([0-5][0-9])\b").expect("Valid time regex"));

    re.replace_all(&with_prep, |caps: &Captures| {
        let hour: usize = caps[1].parse().unwrap_or(99);
        let minute: u32 = caps[2].parse().unwrap_or(99);

        if hour <= 24 && minute < 60 {
            let hour_str = ORDINAL_HOURS[hour];
            let min_str = if minute == 0 {
                String::new()
            } else if minute < 10 {
                format!("zero {}", number_to_words(minute as u64))
            } else {
                number_to_words(minute as u64)
            };

            if min_str.is_empty() {
                hour_str.to_string()
            } else {
                format!("{} {}", hour_str, min_str)
            }
        } else {
            caps[0].to_string()
        }
    })
    .to_string()
}

/// Odmiana liczebników głównych w dopełniaczu (po przyimkach: z, ze, od, do, dla, bez itp.)
fn number_to_genitive(n: u64) -> String {
    match n {
        1 => "jednego".to_string(),
        2 => "dwóch".to_string(),
        3 => "trzech".to_string(),
        4 => "czterech".to_string(),
        5 => "pięciu".to_string(),
        6 => "sześciu".to_string(),
        7 => "siedmiu".to_string(),
        8 => "ośmiu".to_string(),
        9 => "dziewięciu".to_string(),
        10 => "dziesięciu".to_string(),
        11 => "jedenastu".to_string(),
        12 => "dwunastu".to_string(),
        13 => "trzynastu".to_string(),
        14 => "czternastu".to_string(),
        15 => "piętnastu".to_string(),
        16 => "szesnastu".to_string(),
        17 => "siedemnastu".to_string(),
        18 => "osiemnastu".to_string(),
        19 => "dziewiętnastu".to_string(),
        20 => "dwudziestu".to_string(),
        30 => "trzydziestu".to_string(),
        40 => "czterdziestu".to_string(),
        50 => "pięćdziesięciu".to_string(),
        60 => "sześćdziesięciu".to_string(),
        70 => "siedemdziesięciu".to_string(),
        80 => "osiemdziesięciu".to_string(),
        90 => "dziewięćdziesięciu".to_string(),
        100 => "stu".to_string(),
        200 => "dwustu".to_string(),
        300 => "trzystu".to_string(),
        400 => "czterystu".to_string(),
        500 => "pięciuset".to_string(),
        600 => "sześciuset".to_string(),
        700 => "siedmiuset".to_string(),
        800 => "ośmiuset".to_string(),
        900 => "dziewięciuset".to_string(),
        1000 => "tysiąca".to_string(),
        _ => {
            if (21..100).contains(&n) {
                let tens = (n / 10) * 10;
                let ones = n % 10;
                if ones == 0 {
                    number_to_genitive(tens)
                } else {
                    format!("{} {}", number_to_genitive(tens), number_to_genitive(ones))
                }
            } else {
                number_to_words(n)
            }
        }
    }
}

/// Normalizuje liczebniki po przyimkach rządzących dopełniaczem (np. "z 5 domów" -> "z pięciu domów")
fn normalize_genitive_prepositions(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b(z|ze|od|do|dla|bez|około|koło|blisko)\s+(\d+)\b")
            .expect("Valid preposition regex")
    });

    re.replace_all(text, |caps: &Captures| {
        let prep = &caps[1];
        if let Ok(n) = caps[2].parse::<u64>() {
            format!("{} {}", prep, number_to_genitive(n))
        } else {
            caps[0].to_string()
        }
    })
    .to_string()
}

/// Normalizuje ułamki dziesiętne z przecinkiem (np. "2,5 zł" -> "dwa i pół złotego", "2,5" -> "dwa i pół")
fn normalize_decimals(text: &str) -> String {
    static RE_CURRENCY: OnceLock<Regex> = OnceLock::new();
    let re_curr = RE_CURRENCY.get_or_init(|| {
        Regex::new(r"\b(\d+),(\d+)\s*(zł|złotych|złote)\b").expect("Valid decimal currency regex")
    });
    let step1 = re_curr.replace_all(text, |caps: &Captures| {
        let int_val: u64 = caps[1].parse().unwrap_or(0);
        let frac_str = &caps[2];
        if frac_str == "5" {
            format!("{} i pół złotego", number_to_words(int_val))
        } else if let Ok(frac_val) = frac_str.parse::<u64>() {
            format!(
                "{} przecinek {} złotego",
                number_to_words(int_val),
                number_to_words(frac_val)
            )
        } else {
            caps[0].to_string()
        }
    });

    static RE_DEC: OnceLock<Regex> = OnceLock::new();
    let re_dec =
        RE_DEC.get_or_init(|| Regex::new(r"\b(\d+),(\d+)\b").expect("Valid decimal regex"));

    re_dec
        .replace_all(&step1, |caps: &Captures| {
            let int_val: u64 = caps[1].parse().unwrap_or(0);
            let frac_str = &caps[2];
            if frac_str == "5" {
                if int_val == 0 {
                    "pół".to_string()
                } else {
                    format!("{} i pół", number_to_words(int_val))
                }
            } else if let Ok(frac_val) = frac_str.parse::<u64>() {
                format!(
                    "{} przecinek {}",
                    number_to_words(int_val),
                    number_to_words(frac_val)
                )
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
}

const ORDINAL_CHAPTERS_MASCULINE: [&str; 32] = [
    "zerowy",
    "pierwszy",
    "drugi",
    "trzeci",
    "czwarty",
    "piąty",
    "szósty",
    "siódmy",
    "ósmy",
    "dziewiąty",
    "dziesiąty",
    "jedenasty",
    "dwunasty",
    "trzynasty",
    "czternasty",
    "piętnasty",
    "szesnasty",
    "siedemnasty",
    "osiemnasty",
    "dziewiętnasty",
    "dwudziesty",
    "dwudziesty pierwszy",
    "dwudziesty drugi",
    "dwudziesty trzeci",
    "dwudziesty czwarty",
    "dwudziesty piąty",
    "dwudziesty szósty",
    "dwudziesty siódmy",
    "dwudziesty ósmy",
    "dwudziesty dziewiąty",
    "trzydziesty",
    "trzydziesty pierwszy",
];

const ORDINAL_CHAPTERS_FEMININE: [&str; 32] = [
    "zerowa",
    "pierwsza",
    "druga",
    "trzecia",
    "czwarta",
    "piąta",
    "szósta",
    "siódma",
    "ósma",
    "dziewiąta",
    "dziesiąta",
    "jedenasta",
    "dwunasta",
    "trzynasta",
    "czternasta",
    "piętnasta",
    "szesnasta",
    "siedemnasta",
    "osiemnasta",
    "dziewiętnasta",
    "dwudziesta",
    "dwudziesta pierwsza",
    "dwudziesta druga",
    "dwudziesta trzecia",
    "dwudziesta czwarta",
    "dwudziesta piąta",
    "dwudziesta szósta",
    "dwudziesta siódma",
    "dwudziesta ósma",
    "dwudziesta dziewiąta",
    "trzydziesta",
    "trzydziesta pierwsza",
];

fn roman_to_u32(roman: &str) -> Option<u32> {
    match roman.to_uppercase().as_str() {
        "I" => Some(1),
        "II" => Some(2),
        "III" => Some(3),
        "IV" => Some(4),
        "V" => Some(5),
        "VI" => Some(6),
        "VII" => Some(7),
        "VIII" => Some(8),
        "IX" => Some(9),
        "X" => Some(10),
        "XI" => Some(11),
        "XII" => Some(12),
        "XIII" => Some(13),
        "XIV" => Some(14),
        "XV" => Some(15),
        "XVI" => Some(16),
        "XVII" => Some(17),
        "XVIII" => Some(18),
        "XIX" => Some(19),
        "XX" => Some(20),
        _ => None,
    }
}

/// Normalizuje nagłówki strukturalne: "Rozdział 1" / "Rozdział I" -> "Rozdział pierwszy",
/// "Część 2" / "Scena 3" -> "Część druga" / "Scena trzecia"
fn normalize_headings_and_ordinals(text: &str) -> String {
    static RE_MASC: OnceLock<Regex> = OnceLock::new();
    let re_masc = RE_MASC.get_or_init(|| {
        Regex::new(r"(?i)\b(Rozdział|Tom|Akt|Odcinek|Etap|Punkt|Krok)\s+([0-9]{1,2}|[IVX]{1,5})\b")
            .expect("Valid masculine heading regex")
    });

    let step1 = re_masc.replace_all(text, |caps: &Captures| {
        let noun = &caps[1];
        let num_str = &caps[2];
        let num_opt = num_str
            .parse::<u32>()
            .ok()
            .or_else(|| roman_to_u32(num_str));
        if let Some(n) = num_opt {
            if (1..=31).contains(&n) {
                return format!("{} {}", noun, ORDINAL_CHAPTERS_MASCULINE[n as usize]);
            }
        }
        caps[0].to_string()
    });

    static RE_FEM: OnceLock<Regex> = OnceLock::new();
    let re_fem = RE_FEM.get_or_init(|| {
        Regex::new(r"(?i)\b(Część|Scena|Księga|Faza|Sekcja|Próba)\s+([0-9]{1,2}|[IVX]{1,5})\b")
            .expect("Valid feminine heading regex")
    });

    re_fem
        .replace_all(&step1, |caps: &Captures| {
            let noun = &caps[1];
            let num_str = &caps[2];
            let num_opt = num_str
                .parse::<u32>()
                .ok()
                .or_else(|| roman_to_u32(num_str));
            if let Some(n) = num_opt {
                if (1..=31).contains(&n) {
                    return format!("{} {}", noun, ORDINAL_CHAPTERS_FEMININE[n as usize]);
                }
            }
            caps[0].to_string()
        })
        .to_string()
}

/// Normalizuje skrót 'r.' lub słowo 'roku' po czterocyfrowym roku (np. "w 2026 roku" -> "w dwa tysiące dwudziestym szóstym roku", "2026 roku" -> "dwa tysiące dwudziestego szóstego roku")
fn normalize_year_abbreviations(text: &str) -> String {
    // 1. Miejscownik po przyimku "w" / "we": "w 2026 roku" lub "w 2026 r."
    static RE_LOC: OnceLock<Regex> = OnceLock::new();
    let re_loc = RE_LOC.get_or_init(|| {
        Regex::new(r"(?i)\b(w|we)\s+([1-2][0-9]{3})\s*(?:roku|r\.)")
            .expect("Valid year locative regex")
    });
    let step0 = re_loc.replace_all(text, |caps: &Captures| {
        let prep = &caps[1];
        let year: u32 = caps[2].parse().unwrap_or(0);
        let gen = year_to_words_genitive(year);
        // Zamień końcówkę dopełniacza "-ego roku" na miejscownik "-ym roku" (lub "-im roku" dla trzeciego -> trzecim, drugiego -> drugim)
        let loc = gen
            .replace("pierwszego", "pierwszym")
            .replace("drugiego", "drugim")
            .replace("trzeciego", "trzecim")
            .replace("ego ", "ym ");
        format!("{} {}", prep, loc)
    });

    // 2. Dopełniacz przed słowem "roku": np. "z 2026 roku", "od 1999 roku", "dwudziestego czwartego lutego 2026 roku"
    static RE_ROKU: OnceLock<Regex> = OnceLock::new();
    let re_roku = RE_ROKU.get_or_init(|| {
        Regex::new(r"(?i)\b([1-2][0-9]{3})\s+roku\b").expect("Valid year + roku regex")
    });
    let step0b = re_roku.replace_all(&step0, |caps: &Captures| {
        let year: u32 = caps[1].parse().unwrap_or(0);
        year_to_words_genitive(year)
    });

    static RE_MID: OnceLock<Regex> = OnceLock::new();
    let re_mid = RE_MID.get_or_init(|| {
        Regex::new(r"\b([1-2][0-9]{3})\s*r\.(\s+[a-ząćęłńóśźż,;])").expect("Valid year mid regex")
    });
    let step1 = re_mid.replace_all(&step0b, |caps: &Captures| {
        let year: u32 = caps[1].parse().unwrap_or(0);
        let trailing = &caps[2];
        format!(
            "{} roku{}",
            year_to_words_genitive(year).trim_end_matches(" roku"),
            trailing
        )
    });

    static RE_END: OnceLock<Regex> = OnceLock::new();
    let re_end = RE_END.get_or_init(|| {
        Regex::new(r"\b([1-2][0-9]{3})\s*r\.(?:\s*$)").expect("Valid year end regex")
    });
    re_end
        .replace_all(&step1, |caps: &Captures| {
            let year: u32 = caps[1].parse().unwrap_or(0);
            format!("{}.", year_to_words_genitive(year))
        })
        .to_string()
}

/// Normalizuje wolnostojące liczby całkowite
fn normalize_standalone_numbers(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b\d+\b").expect("Valid number regex"));

    re.replace_all(text, |caps: &Captures| {
        if let Ok(n) = caps[0].parse::<u64>() {
            number_to_words(n)
        } else {
            caps[0].to_string()
        }
    })
    .to_string()
}

/// Główna funkcja normalizująca tekst w języku polskim:
/// 1. Konwertuje daty i godziny (np. 24.02.2024 -> "dwudziestego czwartego lutego dwa tysiące dwudziestego czwartego roku")
/// 2. Usuwa kolizje skrótu r. w środku zdania (np. "1999 r. i ..." -> "...roku i ...")
/// 3. Normalizuje ułamki dziesiętne (np. "2,5 zł" -> "dwa i pół złotego")
/// 4. Odmienia liczebniki po przyimkach dopełniacza (np. "z 5 domów" -> "z pięciu domów")
/// 5. Konwertuje pozostałe samodzielne liczby (np. 1500 -> "tysiąc pięćset")
pub fn normalize_polish_text(raw_text: &str) -> String {
    let step0 = normalize_headings_and_ordinals(raw_text);
    let step1 = normalize_dates(&step0);
    let step1b = normalize_textual_dates(&step1);
    let step1c = normalize_year_abbreviations(&step1b);
    let step2 = normalize_times(&step1c);
    let step3 = normalize_decimals(&step2);
    let step4 = normalize_genitive_prepositions(&step3);
    normalize_standalone_numbers(&step4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_textual_date() {
        assert_eq!(
            normalize_polish_text("Kijów, 24 lutego 2024."),
            "Kijów, dwudziestego czwartego lutego dwa tysiące dwudziestego czwartego roku."
        );
        assert_eq!(
            normalize_polish_text("Spotkanie 15 stycznia 2026 r."),
            "Spotkanie piętnastego stycznia dwa tysiące dwudziestego szóstego roku."
        );
        assert_eq!(
            normalize_polish_text("Dwudziestego czwartego lutego 2026 roku o godzinie 15:45"),
            "Dwudziestego czwartego lutego dwa tysiące dwudziestego szóstego roku o godzinie piętnastej czterdzieści pięć"
        );
        assert_eq!(
            normalize_polish_text("Rozdział 1: Próba Generalna. Rozdział II: Finał. Część 3."),
            "Rozdział pierwszy: Próba Generalna. Rozdział drugi: Finał. Część trzecia."
        );
        assert_eq!(
            normalize_polish_text("Święto 1 maja"),
            "Święto pierwszego maja"
        );
    }

    #[test]
    fn test_number_to_words() {
        assert_eq!(number_to_words(0), "zero");
        assert_eq!(number_to_words(7), "siedem");
        assert_eq!(number_to_words(14), "czternaście");
        assert_eq!(number_to_words(42), "czterdzieści dwa");
        assert_eq!(number_to_words(100), "sto");
        assert_eq!(number_to_words(315), "trzysta piętnaście");
        assert_eq!(number_to_words(1000), "tysiąc");
        assert_eq!(number_to_words(2024), "dwa tysiące dwadzieścia cztery");
        assert_eq!(number_to_words(1547), "tysiąc pięćset czterdzieści siedem");
    }

    #[test]
    fn test_normalize_time() {
        assert_eq!(
            normalize_times("Godzina: 15:47."),
            "Godzina: piętnasta czterdzieści siedem."
        );
        assert_eq!(normalize_times("O 08:05 rano."), "O ósmej zero pięć rano.");
        assert_eq!(
            normalize_times("Spotkanie o 12:00."),
            "Spotkanie o dwunastej."
        );
    }

    #[test]
    fn test_normalize_date() {
        assert_eq!(
            normalize_dates("Data: 24.02.2022."),
            "Data: dwudziestego czwartego lutego dwa tysiące dwudziestego drugiego roku."
        );
        assert_eq!(
            normalize_dates("Dnia 15.03.2024 roku"),
            "Dnia piętnastego marca dwa tysiące dwudziestego czwartego roku"
        );
    }

    #[test]
    fn test_normalize_full_sentence() {
        let input = "Data: 24.02.2022. Godzina: 15:47. Przeszło 450 osób.";
        let result = normalize_polish_text(input);
        assert_eq!(
            result,
            "Data: dwudziestego czwartego lutego dwa tysiące dwudziestego drugiego roku. Godzina: piętnasta czterdzieści siedem. Przeszło czterysta pięćdziesiąt osób."
        );
    }

    #[test]
    fn test_normalize_prepositions_decimals_and_years() {
        assert_eq!(normalize_polish_text("z 5 domów"), "z pięciu domów");
        assert_eq!(normalize_polish_text("od 3 dni"), "od trzech dni");
        assert_eq!(normalize_polish_text("2,5 zł"), "dwa i pół złotego");
        assert_eq!(normalize_polish_text("3,14"), "trzy przecinek czternaście");

        // Skrót r. w środku zdania nie może mieć kropki dzielącej zdanie:
        let sentence = "Wydarzenie miało miejsce w 1999 r. i zmieniło wszystko.";
        let res = normalize_polish_text(sentence);
        assert!(res.contains("roku i"));
        assert!(!res.contains("roku. i"));
    }
}
