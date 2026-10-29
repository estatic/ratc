use atc_phraseology::{altitude_words, digits_words, number_words, runway_words, spell_char};

#[test]
fn altitude_is_spoken_in_thousands_and_hundreds() {
    assert_eq!(altitude_words(2500), "two thousand five hundred");
    assert_eq!(altitude_words(1000), "one thousand");
    assert_eq!(altitude_words(800), "eight hundred");
    assert_eq!(altitude_words(12000), "one two thousand");
    assert_eq!(altitude_words(0), "zero");
}

#[test]
fn altitude_rounds_down_to_hundreds() {
    assert_eq!(altitude_words(2599), "two thousand five hundred");
    assert_eq!(altitude_words(99), "zero");
}

#[test]
fn number_is_spoken_digit_by_digit() {
    assert_eq!(number_words(15), "one five");
    assert_eq!(number_words(8), "eight");
    assert_eq!(number_words(0), "zero");
    assert_eq!(number_words(-3), "minus three");
    assert_eq!(number_words(-12), "minus one two");
}

#[test]
fn shared_helpers_are_public() {
    assert_eq!(digits_words(1013, 4), "one zero one three");
    assert_eq!(runway_words("25"), "two five");
    assert_eq!(spell_char('A'), "Alpha");
}
