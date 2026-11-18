// Japanese lyrics in romaji: each line is cut into words by Lindera with the
// IPADIC dictionary built into the app (no network, no file on disk), each
// word's katakana reading becomes Hepburn romaji, and the words are joined
// with spaces, the way romanised lyrics are written.
//
// The dictionary is loaded on the first Japanese line and kept. Readings are
// the dictionary's: a song's own reading of a word (運命 sung "sadame") or a
// rare name comes out as the dictionary reads it.

use std::borrow::Cow;
use std::sync::OnceLock;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

/// True when `text` has kana or kanji: a line worth romanising.
pub fn is_japanese(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{FF66}'..='\u{FF9F}'))
}

fn segmenter() -> Option<&'static Segmenter> {
    static SEGMENTER: OnceLock<Option<Segmenter>> = OnceLock::new();
    SEGMENTER
        .get_or_init(|| match load_dictionary("embedded://ipadic") {
            Ok(dictionary) => Some(Segmenter::new(Mode::Normal, dictionary, None)),
            Err(err) => {
                crate::log(format!("romaji: no dictionary: {err}"));
                None
            }
        })
        .as_ref()
}

/// A line of lyrics in romaji; the line as it is when it has no Japanese or
/// the dictionary is missing.
pub fn line_to_romaji(text: &str) -> String {
    if !is_japanese(text) {
        return text.to_string();
    }
    let text = &strip_readings(text);
    let Some(segmenter) = segmenter() else { return text.to_string() };
    let Ok(mut tokens) = segmenter.segment(Cow::Borrowed(text.as_str())) else { return text.to_string() };
    let mut words: Vec<Word> = Vec::with_capacity(tokens.len());
    // A word ending in a small tsu doubles the next word's consonant (連れてって: tsurete + tte).
    let mut tsu = false;
    for token in tokens.iter_mut() {
        let surface = token.surface.as_ref().to_string();
        let details: Vec<String> = token.details().iter().map(|s| s.to_string()).collect();
        let (mut word, ends_in_tsu) = word_of(&surface, &details, tsu);
        if tsu {
            word.glued = true;
        }
        tsu = ends_in_tsu;
        words.push(word);
    }
    join(&words)
}

/// A word of a line and its furigana: the hiragana reading over its kanji
/// ("" for a word without kanji). Okurigana, the kana written after (or
/// before) the kanji, get none, as in print (走る: はし over 走, る bare).
#[derive(serde::Serialize, Clone, Debug, PartialEq, Default)]
pub struct Ruby {
    pub text: String,
    pub ruby: String,
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '々' | '〆' | 'ヶ')
}

/// True when `text` has a kanji: a line that can take furigana.
pub fn has_kanji(text: &str) -> bool {
    text.chars().any(is_kanji)
}

fn to_hiragana(c: char) -> char {
    match c {
        'ァ'..='ヶ' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

/// A line cut into words with their furigana (what Mini Island showed, from
/// the same IPADIC readings as the romaji). The words put back together are
/// the line as written.
pub fn line_to_ruby(text: &str) -> Vec<Ruby> {
    let Some(segmenter) = segmenter() else { return vec![Ruby { text: text.to_string(), ruby: String::new() }] };
    let Ok(mut tokens) = segmenter.segment(Cow::Borrowed(text)) else { return vec![Ruby { text: text.to_string(), ruby: String::new() }] };
    let mut out: Vec<Ruby> = Vec::new();
    for token in tokens.iter_mut() {
        let surface = token.surface.as_ref().to_string();
        let details: Vec<String> = token.details().iter().map(|s| s.to_string()).collect();
        let reading = details.get(7).filter(|r| !r.is_empty() && r.as_str() != "*").cloned();
        match (has_kanji(&surface), reading) {
            (true, Some(r)) => out.extend(split_okurigana(&surface, &r.chars().map(to_hiragana).collect::<String>())),
            _ => out.push(Ruby { text: surface, ruby: String::new() }),
        }
    }
    out
}

/// A word with kanji and its hiragana reading, each run of kanji given its
/// own reading and the kana around and between them left bare, by lining the
/// kana up with the reading: 走る/はしる → 走(はし) る; お茶 → お 茶(ちゃ);
/// 抜け出/ぬけだ → 抜(ぬ) け 出(だ). When they don't line up, the whole word
/// takes the reading.
fn split_okurigana(surface: &str, reading: &str) -> Vec<Ruby> {
    // Runs of kanji and of everything else.
    let mut runs: Vec<(String, bool)> = Vec::new();
    for c in surface.chars() {
        let k = is_kanji(c);
        match runs.last_mut() {
            Some((t, kk)) if *kk == k => t.push(c),
            _ => runs.push((c.to_string(), k)),
        }
    }
    let r: Vec<char> = reading.chars().collect();
    let mut readings: Vec<String> = Vec::new();
    if fit(&runs, 0, &r, 0, &mut readings) {
        return runs
            .into_iter()
            .zip(readings)
            .map(|((text, kanji), ruby)| Ruby { ruby: if kanji && ruby != text { ruby } else { String::new() }, text })
            .collect();
    }
    vec![Ruby { text: surface.to_string(), ruby: reading.to_string() }]
}

/// Lines `runs` up with the reading `r` from `at`: a kana run must match it
/// there; a kanji run takes one or more of its characters (the fewest that
/// let the rest line up). Fills `out` with each run's part of the reading.
fn fit(runs: &[(String, bool)], i: usize, r: &[char], at: usize, out: &mut Vec<String>) -> bool {
    let Some((text, kanji)) = runs.get(i) else { return at == r.len() };
    if !kanji {
        let k: Vec<char> = text.chars().map(to_hiragana).collect();
        if r.len() >= at + k.len() && r[at..at + k.len()] == k[..] {
            out.push(text.clone());
            if fit(runs, i + 1, r, at + k.len(), out) {
                return true;
            }
            out.pop();
        }
        return false;
    }
    for len in 1..=r.len().saturating_sub(at) {
        out.push(r[at..at + len].iter().collect());
        if fit(runs, i + 1, r, at + len, out) {
            return true;
        }
        out.pop();
    }
    false
}

/// Drops the kana readings some lyrics put in brackets after a word
/// (箱の（ばこの）): romanised, they read twice ("hako no (bako no)"). A sung
/// echo stays: its brackets repeat what came just before
/// (されなくても（されなくても）).
fn strip_readings(text: &str) -> String {
    let kana_only = |s: &str| !s.trim().is_empty() && s.chars().all(|c| matches!(c, '\u{3040}'..='\u{30FF}' | ' ' | '\u{3000}'));
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find(['(', '（']) {
        let (before, from) = rest.split_at(open);
        out.push_str(before);
        let inner_at = from.chars().next().map_or(1, char::len_utf8);
        let Some(close) = from[inner_at..].find([')', '）']) else {
            out.push_str(from);
            return out;
        };
        let inner = &from[inner_at..inner_at + close];
        let end = inner_at + close + from[inner_at + close..].chars().next().map_or(1, char::len_utf8);
        let echo = out.trim_end().ends_with(inner.trim());
        if !kana_only(inner) || echo {
            out.push_str(&from[..end]);
        }
        rest = &from[end..];
    }
    out.push_str(rest);
    out
}

struct Word {
    text: String,
    /// Written onto the word before it, without a space: an inflection, a
    /// suffix, a small tsu waiting for the next consonant.
    glued: bool,
    /// Punctuation or a space in the line.
    mark: bool,
}

/// One token: its kana (the reading; the pronunciation for particles, so は
/// is "wa"), in romaji. A token with no reading (latin, digits, an unknown
/// word in kana) keeps its own spelling, romanised if it is kana. `tsu`: the
/// word before ended in a small tsu. Also says whether this one does.
fn word_of(surface: &str, details: &[String], tsu: bool) -> (Word, bool) {
    let pos = details.first().map(String::as_str).unwrap_or("");
    let pos1 = details.get(1).map(String::as_str).unwrap_or("");
    let usable = |s: Option<&String>| s.filter(|r| !r.is_empty() && r.as_str() != "*").cloned();
    let kana = if pos == "助詞" {
        usable(details.get(8)).or_else(|| usable(details.get(7)))
    } else {
        usable(details.get(7))
    };
    let mark = pos == "記号" || surface.trim().is_empty();
    let mut kana = kana.unwrap_or_else(|| surface.to_string());
    // IPADIC often makes the small tsu a word of its own (連れ・て・っ・て).
    let ends_in_tsu = kana.ends_with(['ッ', 'っ']);
    if ends_in_tsu {
        kana.pop();
    }
    if tsu {
        kana.insert(0, 'ッ');
    }
    let text = kana_to_romaji(&kana);
    let glued = matches!(pos, "助動詞") || pos1 == "接尾" || (pos == "動詞" && pos1 == "非自立") || (pos == "助詞" && pos1 == "接続助詞" && surface == "て");
    (Word { text, glued, mark }, ends_in_tsu)
}

fn join(words: &[Word]) -> String {
    let mut out = String::new();
    for w in words {
        let text = w.text.trim();
        if text.is_empty() {
            if w.mark && !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            continue;
        }
        let space = !out.is_empty() && !out.ends_with(' ') && !w.glued && !(w.mark && is_closing(text));
        if space {
            out.push(' ');
        }
        out.push_str(text);
    }
    out.trim().to_string()
}

fn is_closing(s: &str) -> bool {
    matches!(s, "," | "." | "!" | "?" | ")" | "」" | "』" | "、" | "。" | "…" | "~")
}

/// Hiragana and katakana in Hepburn romaji. Other characters pass through;
/// the full-width punctuation of lyrics becomes its ASCII form.
pub fn kana_to_romaji(text: &str) -> String {
    let chars: Vec<char> = text.chars().map(to_katakana).collect();
    let mut out = String::new();
    let mut i = 0;
    let mut double_next = false;
    while i < chars.len() {
        let c = chars[i];
        // A small tsu doubles the next consonant (ッテ → tte; ッチ → tchi).
        if c == 'ッ' {
            double_next = true;
            i += 1;
            continue;
        }
        // The long-vowel mark repeats the vowel before it.
        if c == 'ー' {
            if let Some(v) = out.chars().last().filter(|v| "aeiou".contains(*v)) {
                out.push(v);
            }
            i += 1;
            continue;
        }
        let pair = chars.get(i + 1).and_then(|n| syllable(&[c, *n]).map(|s| (s, 2)));
        let (syl, used) = match pair.or_else(|| syllable(&[c]).map(|s| (s, 1))) {
            Some(found) => found,
            None => {
                out.push_str(&punct(c));
                double_next = false;
                i += 1;
                continue;
            }
        };
        if double_next {
            if syl.starts_with("ch") {
                out.push('t');
            } else if let Some(first) = syl.chars().next().filter(|f| !"aeiou".contains(*f)) {
                out.push(first);
            }
            double_next = false;
        }
        // ン before a vowel or y keeps the syllables apart: kin'you.
        if syl == "n" {
            let next = chars.get(i + 1).and_then(|n| syllable(&[*n]));
            if next.is_some_and(|s| s.starts_with(['a', 'e', 'i', 'o', 'u', 'y'])) {
                out.push_str("n'");
                i += used;
                continue;
            }
        }
        out.push_str(syl);
        i += used;
    }
    out
}

fn to_katakana(c: char) -> char {
    match c {
        '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
        _ => c,
    }
}

fn punct(c: char) -> String {
    match c {
        '、' => ",".into(),
        '。' => ".".into(),
        '！' => "!".into(),
        '？' => "?".into(),
        '　' => " ".into(),
        '「' | '」' | '『' | '』' => "\"".into(),
        '（' => "(".into(),
        '）' => ")".into(),
        '～' | '〜' => "~".into(),
        '・' => " ".into(),
        _ => c.to_string(),
    }
}

/// One katakana syllable (one or two characters) in romaji.
fn syllable(k: &[char]) -> Option<&'static str> {
    let s: String = k.iter().collect();
    Some(match s.as_str() {
        "キャ" => "kya", "キュ" => "kyu", "キョ" => "kyo",
        "シャ" => "sha", "シュ" => "shu", "ショ" => "sho", "シェ" => "she",
        "チャ" => "cha", "チュ" => "chu", "チョ" => "cho", "チェ" => "che",
        "ニャ" => "nya", "ニュ" => "nyu", "ニョ" => "nyo",
        "ヒャ" => "hya", "ヒュ" => "hyu", "ヒョ" => "hyo",
        "ミャ" => "mya", "ミュ" => "myu", "ミョ" => "myo",
        "リャ" => "rya", "リュ" => "ryu", "リョ" => "ryo",
        "ギャ" => "gya", "ギュ" => "gyu", "ギョ" => "gyo",
        "ジャ" => "ja", "ジュ" => "ju", "ジョ" => "jo", "ジェ" => "je",
        "ヂャ" => "ja", "ヂュ" => "ju", "ヂョ" => "jo",
        "ビャ" => "bya", "ビュ" => "byu", "ビョ" => "byo",
        "ピャ" => "pya", "ピュ" => "pyu", "ピョ" => "pyo",
        "ファ" => "fa", "フィ" => "fi", "フェ" => "fe", "フォ" => "fo",
        "ティ" => "ti", "ディ" => "di", "トゥ" => "tu", "ドゥ" => "du",
        "ウィ" => "wi", "ウェ" => "we", "ウォ" => "wo", "ヴァ" => "va", "ヴィ" => "vi", "ヴェ" => "ve", "ヴォ" => "vo",
        "ア" => "a", "イ" => "i", "ウ" => "u", "エ" => "e", "オ" => "o",
        "ァ" => "a", "ィ" => "i", "ゥ" => "u", "ェ" => "e", "ォ" => "o",
        "カ" => "ka", "キ" => "ki", "ク" => "ku", "ケ" => "ke", "コ" => "ko",
        "サ" => "sa", "シ" => "shi", "ス" => "su", "セ" => "se", "ソ" => "so",
        "タ" => "ta", "チ" => "chi", "ツ" => "tsu", "テ" => "te", "ト" => "to",
        "ナ" => "na", "ニ" => "ni", "ヌ" => "nu", "ネ" => "ne", "ノ" => "no",
        "ハ" => "ha", "ヒ" => "hi", "フ" => "fu", "ヘ" => "he", "ホ" => "ho",
        "マ" => "ma", "ミ" => "mi", "ム" => "mu", "メ" => "me", "モ" => "mo",
        "ヤ" => "ya", "ユ" => "yu", "ヨ" => "yo", "ャ" => "ya", "ュ" => "yu", "ョ" => "yo",
        "ラ" => "ra", "リ" => "ri", "ル" => "ru", "レ" => "re", "ロ" => "ro",
        "ワ" => "wa", "ヰ" => "i", "ヱ" => "e", "ヲ" => "o", "ン" => "n",
        "ガ" => "ga", "ギ" => "gi", "グ" => "gu", "ゲ" => "ge", "ゴ" => "go",
        "ザ" => "za", "ジ" => "ji", "ズ" => "zu", "ゼ" => "ze", "ゾ" => "zo",
        "ダ" => "da", "ヂ" => "ji", "ヅ" => "zu", "デ" => "de", "ド" => "do",
        "バ" => "ba", "ビ" => "bi", "ブ" => "bu", "ベ" => "be", "ボ" => "bo",
        "パ" => "pa", "ピ" => "pi", "プ" => "pu", "ペ" => "pe", "ポ" => "po",
        "ヴ" => "vu",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furigana_sits_over_the_kanji_only() {
        let r = |t: &str, ruby: &str| Ruby { text: t.into(), ruby: ruby.into() };
        assert_eq!(split_okurigana("走る", "はしる"), vec![r("走", "はし"), r("る", "")]);
        assert_eq!(split_okurigana("お茶", "おちゃ"), vec![r("お", ""), r("茶", "ちゃ")]);
        assert_eq!(split_okurigana("運命", "うんめい"), vec![r("運命", "うんめい")]);
        assert_eq!(split_okurigana("取り扱い", "とりあつかい"), vec![r("取", "と"), r("り", ""), r("扱", "あつか"), r("い", "")], "kana inside line up too");
        assert_eq!(split_okurigana("抜け出", "ぬけだ"), vec![r("抜", "ぬ"), r("け", ""), r("出", "だ")]);
    }

    #[test]
    fn a_line_cut_into_words_keeps_its_text() {
        let line = "10月無口な君を忘れる";
        let words = line_to_ruby(line);
        assert_eq!(words.iter().map(|w| w.text.as_str()).collect::<String>(), line);
        assert!(words.iter().any(|w| w.text == "君" && w.ruby == "きみ"), "{words:?}");
        assert!(words.iter().any(|w| w.text == "忘" && w.ruby == "わす"), "{words:?}");
        assert!(words.iter().filter(|w| !w.ruby.is_empty()).all(|w| has_kanji(&w.text)), "furigana only over kanji");
        // The owner's line (平行線, 19 Nov 2026).
        let line = "太陽系を抜け出して平行線で交わろう";
        let words = line_to_ruby(line);
        assert_eq!(words.iter().map(|w| w.text.as_str()).collect::<String>(), line);
        let read: Vec<(String, String)> = words.iter().filter(|w| !w.ruby.is_empty()).map(|w| (w.text.clone(), w.ruby.clone())).collect();
        assert!(read.iter().any(|(t, r)| t == "抜" && r == "ぬ") && read.iter().any(|(t, r)| t == "出" && r == "だ"), "{read:?}");
        assert!(read.iter().any(|(t, r)| t == "交" && r == "まじ"), "{read:?}");
        assert!(read.iter().any(|(t, r)| t.contains("平行") && r.starts_with("へいこう")), "{read:?}");
    }

    #[test]
    fn kana_becomes_hepburn() {
        assert_eq!(kana_to_romaji("きょう"), "kyou");
        assert_eq!(kana_to_romaji("いっしょ"), "issho");
        assert_eq!(kana_to_romaji("マッチ"), "matchi");
        assert_eq!(kana_to_romaji("コーヒー"), "koohii");
        assert_eq!(kana_to_romaji("きんよう"), "kin'you");
        assert_eq!(kana_to_romaji("さよなら、"), "sayonara,");
        assert_eq!(kana_to_romaji("ABC 123"), "ABC 123");
    }

    #[test]
    fn only_japanese_lines_are_touched() {
        assert!(is_japanese("君の名は"));
        assert!(is_japanese("カタカナ"));
        assert!(!is_japanese("Hello, world!"));
        assert_eq!(line_to_romaji("Hello, world!"), "Hello, world!");
    }

    #[test]
    fn a_line_with_kanji_comes_out_in_romaji() {
        assert_eq!(line_to_romaji("私は東京に行きます"), "watashi wa toukyou ni ikimasu");
        assert_eq!(line_to_romaji("君を愛している"), "kimi o aishiteiru");
        assert_eq!(line_to_romaji("知らない世界へ連れてって"), "shiranai sekai e tsuretette");
        assert_eq!(line_to_romaji("箱の（ばこの）中で（ちゅうで）"), "hako no naka de");
    }

    #[test]
    fn readings_in_brackets_go_echoes_stay() {
        assert_eq!(strip_readings("箱の（ばこの）中で（ちゅうで）"), "箱の中で");
        assert_eq!(strip_readings("箱の(ばこの)中で"), "箱の中で");
        assert_eq!(strip_readings("見向きさえされなくても（されなくても）"), "見向きさえされなくても（されなくても）");
        assert_eq!(strip_readings("君と(Hey!)行こう"), "君と(Hey!)行こう");
        assert_eq!(strip_readings("開いたまま(ひら"), "開いたまま(ひら");
    }

    /// Prints a few lines of real lyrics: `cargo test -p island-lyrics -- --ignored --nocapture romaji_samples`
    #[test]
    #[ignore]
    fn romaji_samples() {
        for l in ["君が好きだと叫びたい", "運命の赤い糸", "ありがとう、さよなら", "夢を見ていた 遠い日の空", "もう一度だけ会いたいよ", "知らない世界へ連れてって"] {
            eprintln!("{l} => {}", line_to_romaji(l));
        }    }
}
