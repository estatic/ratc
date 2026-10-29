use atc_phraseology::pl::numbers::*;

#[test]
fn cardinals() {
    assert_eq!(cardinal(0), "zero");
    assert_eq!(cardinal(13), "trzynaście");
    assert_eq!(cardinal(25), "dwadzieścia pięć");
    assert_eq!(cardinal(118), "sto osiemnaście");
    assert_eq!(cardinal(250), "dwieście pięćdziesiąt");
    assert_eq!(cardinal(1800), "tysiąc osiemset");
    assert_eq!(cardinal(3400), "trzy tysiące czterysta");
    assert_eq!(cardinal(12_000), "dwanaście tysięcy");
}

#[test]
fn plural_forms() {
    assert_eq!(plural(1, "węzeł", "węzły", "węzłów"), "węzeł");
    assert_eq!(plural(3, "węzeł", "węzły", "węzłów"), "węzły");
    assert_eq!(plural(12, "węzeł", "węzły", "węzłów"), "węzłów");
    assert_eq!(plural(22, "węzeł", "węzły", "węzłów"), "węzły");
    assert_eq!(plural(25, "węzeł", "węzły", "węzłów"), "węzłów");
}

#[test]
fn doc_9432_pl_examples() {
    assert_eq!(qnh(1010), "dziesięć dziesięć");
    assert_eq!(qnh(1008), "dziesięć zero osiem");
    assert_eq!(qnh(1013), "dziesięć trzynaście");
    assert_eq!(qnh(998), "dziewięć dziewięć osiem");
    assert_eq!(qnh(1000), "tysiąc");
    assert_eq!(squawk(2400), "dwa cztery zero zero");
    assert_eq!(squawk(4203), "cztery dwa zero trzy");
    assert_eq!(squawk(7000), "siedem tysięcy");
    assert_eq!(frequency(118_100), "sto osiemnaście kropka jeden");
    assert_eq!(frequency(118_025), "sto osiemnaście kropka zero dwa pięć");
    assert_eq!(frequency(118_105), "sto osiemnaście kropka jeden zero pięć");
    assert_eq!(
        frequency(121_980),
        "sto dwadzieścia jeden kropka dziewięć osiem zero"
    );
    assert_eq!(
        frequency(123_255),
        "sto dwadzieścia trzy kropka dwa pięć pięć"
    );
    assert_eq!(wind(200, 25), "dwieście stopni dwadzieścia pięć węzłów");
    assert_eq!(wind(160, 18), "sto sześćdziesiąt stopni osiemnaście węzłów");
    assert_eq!(wind(250, 1), "dwieście pięćdziesiąt stopni jeden węzeł");
    assert_eq!(altitude_ft(1800), "tysiąc osiemset stóp");
    assert_eq!(digits(25, 2), "dwa pięć");
    assert_eq!(digits(7, 2), "zero siedem");
}
