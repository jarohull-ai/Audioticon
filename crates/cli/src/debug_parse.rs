
use audioticon_core::{EmojiDirector, DirectedItem};

const SCENE: &str = r#"
Oto demonstracja zaawansowanych efektów akustycznych Audioticon. 🛑

Narrator mówi czystym, studyjnym głosem. ⏸️
Ale nagle odbiera telefon. 🛑
📱 "Halo? Richter, słyszysz mnie? Mamy problem na punkcie kontrolnym!" 🛑

W tle odzywa się wojskowa radiostacja krótkofalowa. 🛑
📻 "Baza jeden do patrolu. Zgłaszam naruszenie perymetru w sektorze czwartym. Odbiór." 🛑

Dowódca bierze tubę i krzyczy przez megafon na placu: 🛑
📢 "Wszyscy natychmiast opuścić ten rejon! To nie są ćwiczenia!" 🛑

Adam cofa się w głąb starej, opuszczonej katedry. 🛑
🏛️ Kroki niosły się głuchym echem po kamiennych łukach świątyni. 🛑

Schodzi wąskimi schodami do wilgotnych lochów. 🛑
🏰 Ciemność była lepka, a woda kapała gdzieś w głębi podziemi. 🛑

Zatrzymał się. 🛑
💭 "Czy oni naprawdę myślą, że nie byłem na to przygotowany? Wszystko przewidziałem." 🛑

Nagle z mroku wyłonił się syntetyczny głos bojowego androida: 🛑
🤖 "Cel zidentyfikowany. Rozpoczynam procedurę neutralizacji." 🛑

I znowu zapadła śmiertelna cisza. 🛑
"#;

fn main() {
    let director = EmojiDirector::default();
    let script = director.parse(SCENE);
    for (i, item) in script.items.iter().enumerate() {
        match item {
            DirectedItem::Silence(ms) => println!("[{:02}] SILENCE: {}ms", i, ms),
            DirectedItem::Speech(seg) => println!("[{:02}] SPEECH: filter={:?}, vol={}, rate={}, pitch={}, text={:?}", i, seg.filter, seg.volume_mod, seg.rate_mod, seg.pitch_mod, seg.text),
        }
    }
}
